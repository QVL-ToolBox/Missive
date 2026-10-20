use crate::config::{Environment, MailConfig, TlsMode, ValidatedMailConfig};
use crate::error::MailError;
use crate::rate_limit::RateLimiter;
use lettre::message::{Mailbox, MessageBuilder, MultiPart, SinglePart};
use lettre::transport::smtp::AsyncSmtpTransportBuilder;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::time::{Duration, Instant};

type SmtpTransport = AsyncSmtpTransport<Tokio1Executor>;

#[derive(Clone, Copy)]
pub enum Body<'a> {
    Text(&'a str),
    Html(&'a str),
    Both { text: &'a str, html: &'a str },
}

impl std::fmt::Debug for Body<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Body::Text(_) => "Text",
            Body::Html(_) => "Html",
            Body::Both { .. } => "Both",
        };
        f.write_str(kind)
    }
}

impl Body<'_> {
    fn has_empty_part(&self) -> bool {
        match self {
            Body::Text(content) | Body::Html(content) => content.is_empty(),
            Body::Both { text, html } => text.is_empty() || html.is_empty(),
        }
    }
}

pub struct Mailer {
    transport: SmtpTransport,
    from: Mailbox,
    rate_limiter: RateLimiter,
    timeout: Duration,
}

impl Mailer {
    pub fn new(config: &MailConfig, environment: Environment) -> Result<Self, MailError> {
        let validated = config.validate(environment)?;
        let transport = build_transport(&validated)?;
        Ok(Self {
            transport,
            from: validated.from,
            rate_limiter: RateLimiter::per_minute(validated.max_per_minute),
            timeout: validated.timeout,
        })
    }

    pub async fn send(&self, to: &str, subject: &str, body: Body<'_>) -> Result<(), MailError> {
        let recipient = parse_single_recipient(to)?;
        let recipient_domain = recipient.email.domain().to_owned();
        let message = self.compose(recipient, subject, body)?;
        self.acquire_quota()?;
        self.deliver(message, &recipient_domain).await
    }

    fn compose(
        &self,
        recipient: Mailbox,
        subject: &str,
        body: Body<'_>,
    ) -> Result<Message, MailError> {
        ensure_single_line(subject)?;
        if body.has_empty_part() {
            return Err(MailError::EmptyBody);
        }
        let builder = Message::builder()
            .from(self.from.clone())
            .to(recipient)
            .subject(subject);
        attach_body(builder, body)
    }

    fn acquire_quota(&self) -> Result<(), MailError> {
        if self.rate_limiter.try_acquire() {
            Ok(())
        } else {
            Err(MailError::RateLimited)
        }
    }

    async fn deliver(&self, message: Message, recipient_domain: &str) -> Result<(), MailError> {
        let started = Instant::now();
        match tokio::time::timeout(self.timeout, self.transport.send(message)).await {
            Ok(Ok(response)) => {
                tracing::info!(
                    recipient_domain,
                    smtp_code = u16::from(response.code()),
                    latency_ms = started.elapsed().as_millis() as u64,
                    "email transmis au serveur SMTP"
                );
                Ok(())
            }
            Ok(Err(error)) => Err(SmtpFailure::rejected(&error).report(recipient_domain, started)),
            Err(_) => Err(SmtpFailure::TIMED_OUT.report(recipient_domain, started)),
        }
    }
}

struct SmtpFailure {
    permanent: bool,
    code: Option<u16>,
    timed_out: bool,
}

impl SmtpFailure {
    const TIMED_OUT: Self = Self {
        permanent: false,
        code: None,
        timed_out: true,
    };

    fn rejected(error: &lettre::transport::smtp::Error) -> Self {
        Self {
            permanent: error.is_permanent(),
            code: error.status().map(u16::from),
            timed_out: false,
        }
    }

    fn report(self, recipient_domain: &str, started: Instant) -> MailError {
        tracing::warn!(
            recipient_domain,
            smtp_code = ?self.code,
            permanent = self.permanent,
            timed_out = self.timed_out,
            latency_ms = started.elapsed().as_millis() as u64,
            "echec de transmission au serveur SMTP"
        );
        MailError::Send {
            permanent: self.permanent,
            code: self.code,
        }
    }
}

fn parse_single_recipient(to: &str) -> Result<Mailbox, MailError> {
    if to.contains(',') {
        return Err(MailError::InvalidRecipient);
    }
    to.parse::<Mailbox>()
        .map_err(|_| MailError::InvalidRecipient)
}

fn ensure_single_line(subject: &str) -> Result<(), MailError> {
    if subject.contains(['\r', '\n']) {
        return Err(MailError::InvalidSubject);
    }
    Ok(())
}

fn attach_body(builder: MessageBuilder, body: Body<'_>) -> Result<Message, MailError> {
    let message = match body {
        Body::Text(text) => builder.singlepart(SinglePart::plain(text.to_owned())),
        Body::Html(html) => builder.singlepart(SinglePart::html(html.to_owned())),
        Body::Both { text, html } => builder.multipart(MultiPart::alternative_plain_html(
            text.to_owned(),
            html.to_owned(),
        )),
    };
    message.map_err(|_| MailError::Build)
}

fn build_transport(config: &ValidatedMailConfig) -> Result<SmtpTransport, MailError> {
    let mut builder = transport_builder(config.tls, &config.host)?;
    if let Some(port) = config.port {
        builder = builder.port(port.get());
    }
    if let Some(credentials) = &config.credentials {
        builder = builder.credentials(credentials.clone());
    }
    Ok(builder.build())
}

fn transport_builder(tls: TlsMode, host: &str) -> Result<AsyncSmtpTransportBuilder, MailError> {
    let builder = match tls {
        TlsMode::Tls => SmtpTransport::relay(host),
        TlsMode::StartTls => SmtpTransport::starttls_relay(host),
        TlsMode::Plaintext => Ok(SmtpTransport::builder_dangerous(host)),
    };
    builder.map_err(|_| MailError::Transport)
}
