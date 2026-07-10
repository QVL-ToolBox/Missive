#![allow(dead_code)]

use crate::config::{Config, SmtpConfig};
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::Error as SmtpError;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum MailerError {
    #[error("configuration SMTP invalide")]
    Transport(#[source] SmtpError),
    #[error("destinataire invalide")]
    InvalidRecipient,
    #[error("message invalide")]
    Build(#[source] lettre::error::Error),
    #[error("envoi SMTP en échec")]
    Send(#[source] SmtpError),
}

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Mailer {
    pub fn from_config(config: &Config) -> Result<Self, MailerError> {
        Ok(Self {
            transport: build_transport(&config.smtp)?,
            from: config.mail_from.clone(),
        })
    }

    pub async fn send(&self, to: &str, subject: &str, body: String) -> Result<(), MailerError> {
        let recipient: Mailbox = to.parse().map_err(|_| MailerError::InvalidRecipient)?;
        let message = Message::builder()
            .from(self.from.clone())
            .to(recipient)
            .subject(subject)
            .body(body)
            .map_err(MailerError::Build)?;

        let started = Instant::now();
        let response = self
            .transport
            .send(message)
            .await
            .map_err(MailerError::Send)?;

        tracing::info!(
            recipient_domain = domain_of(to),
            smtp_code = ?response.code(),
            latency_ms = started.elapsed().as_millis() as u64,
            "email transmis au serveur SMTP"
        );
        Ok(())
    }
}

fn build_transport(smtp: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, MailerError> {
    let mut builder = match &smtp.credentials {
        Some(credentials) => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
            .map_err(MailerError::Transport)?
            .credentials(Credentials::new(
                credentials.user.clone(),
                credentials.password.clone(),
            )),
        None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host),
    };
    if let Some(port) = smtp.port {
        builder = builder.port(port);
    }
    Ok(builder.build())
}

fn domain_of(address: &str) -> &str {
    address.rsplit('@').next().unwrap_or(address)
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    use crate::config::SmtpCredentials;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tracing_subscriber::fmt::MakeWriter;

    const MAILPIT_HOST: &str = "127.0.0.1";
    const MAILPIT_SMTP_PORT: u16 = 1025;
    const SERVER_FROM: &str = "Missive Test <missive-from@missive.test>";
    const SECRET_TOKEN: &str = "INTERNALSECRETLEAKMARKER0123456789ABCDEF";
    const CREDENTIAL_USER_TOKEN: &str = "SMTPUSERLEAKMARKER";
    const CREDENTIAL_PASSWORD_TOKEN: &str = "SMTPPASSWORDLEAKMARKER";

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

    impl CapturedLogs {
        fn as_text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    impl std::io::Write for CapturedLogs {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for CapturedLogs {
        type Writer = CapturedLogs;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }

    fn unique_token(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{prefix}{nanos}")
    }

    fn config_without_credentials() -> Config {
        Config {
            bind_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8184,
            smtp: SmtpConfig {
                host: MAILPIT_HOST.to_string(),
                port: Some(MAILPIT_SMTP_PORT),
                credentials: None,
            },
            mail_from: SERVER_FROM.parse().unwrap(),
        }
    }

    fn config_with_credentials() -> Config {
        Config {
            bind_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8184,
            smtp: SmtpConfig {
                host: MAILPIT_HOST.to_string(),
                port: Some(MAILPIT_SMTP_PORT),
                credentials: Some(SmtpCredentials {
                    user: CREDENTIAL_USER_TOKEN.to_string(),
                    password: CREDENTIAL_PASSWORD_TOKEN.to_string(),
                }),
            },
            mail_from: SERVER_FROM.parse().unwrap(),
        }
    }

    fn capture_send(config: Config, to: &str, subject: &str, body: String) -> (bool, String) {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(logs.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();

        let succeeded = tracing::subscriber::with_default(subscriber, || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let mailer = Mailer::from_config(&config).unwrap();
                mailer.send(to, subject, body).await.is_ok()
            })
        });

        (succeeded, logs.as_text())
    }

    #[test]
    fn successful_send_log_omits_subject_body_secret_and_credentials() {
        let subject = unique_token("SUBJECTLEAKMARKER");
        let body = unique_token("BODYLEAKMARKER");
        let recipient = format!("{}@recipient.example", unique_token("rcpt"));

        let (succeeded, logs) = capture_send(
            config_without_credentials(),
            &recipient,
            &subject,
            body.clone(),
        );

        assert!(succeeded, "l'envoi vers Mailpit doit réussir");
        assert!(!logs.is_empty(), "un log de succès doit être émis");

        assert!(
            !logs.contains(&subject),
            "le subject ne doit jamais apparaître dans les logs"
        );
        assert!(
            !logs.contains(&body),
            "le corps ne doit jamais apparaître dans les logs"
        );
        assert!(
            !logs.contains(SECRET_TOKEN),
            "aucun secret ne doit apparaître dans les logs"
        );
        assert!(
            !logs.contains(CREDENTIAL_USER_TOKEN) && !logs.contains(CREDENTIAL_PASSWORD_TOKEN),
            "aucun credential SMTP ne doit apparaître dans les logs"
        );
    }

    #[test]
    fn successful_send_log_exposes_only_tolerated_fields() {
        let subject = unique_token("SUBJECTFIELDMARKER");
        let body = unique_token("BODYFIELDMARKER");
        let recipient = format!("{}@fields.example", unique_token("rcpt"));

        let (succeeded, logs) =
            capture_send(config_without_credentials(), &recipient, &subject, body);

        assert!(succeeded, "l'envoi vers Mailpit doit réussir");
        assert!(
            logs.contains("recipient_domain"),
            "recipient_domain est toléré et attendu"
        );
        assert!(
            logs.contains("smtp_code"),
            "smtp_code est toléré et attendu"
        );
        assert!(
            logs.contains("latency_ms"),
            "latency_ms est toléré et attendu"
        );
        assert!(
            logs.contains("fields.example"),
            "le domaine du destinataire est toléré"
        );
    }

    #[test]
    fn failed_send_with_credentials_does_not_leak_credentials() {
        let subject = unique_token("SUBJECTCREDMARKER");
        let body = unique_token("BODYCREDMARKER");
        let recipient = format!("{}@cred.example", unique_token("rcpt"));

        let (_succeeded, logs) = capture_send(
            config_with_credentials(),
            &recipient,
            &subject,
            body.clone(),
        );

        assert!(
            !logs.contains(CREDENTIAL_USER_TOKEN),
            "le user SMTP ne doit jamais apparaître dans les logs"
        );
        assert!(
            !logs.contains(CREDENTIAL_PASSWORD_TOKEN),
            "le password SMTP ne doit jamais apparaître dans les logs"
        );
        assert!(
            !logs.contains(&subject),
            "le subject ne doit jamais apparaître dans les logs"
        );
        assert!(
            !logs.contains(&body),
            "le corps ne doit jamais apparaître dans les logs"
        );
    }
}
