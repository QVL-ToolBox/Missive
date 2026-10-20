use crate::error::MailError;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use serde::Deserialize;
use std::net::IpAddr;
use std::num::{NonZeroU16, NonZeroU64};
use std::time::Duration;

const DEFAULT_MAX_PER_MINUTE: u32 = 60;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const LOCALHOST: &str = "localhost";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Environment {
    Dev,
    Prod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub enum TlsMode {
    #[serde(rename = "tls")]
    Tls,
    #[serde(rename = "starttls")]
    StartTls,
    #[serde(rename = "none")]
    Plaintext,
}

#[derive(Clone, Deserialize)]
#[non_exhaustive]
pub struct MailConfig {
    pub host: String,
    pub port: Option<NonZeroU16>,
    pub tls: TlsMode,
    pub user: Option<String>,
    pub password: Option<String>,
    pub from: String,
    #[serde(default = "default_max_per_minute")]
    pub max_per_minute: u32,
    pub timeout_secs: Option<NonZeroU64>,
}

impl std::fmt::Debug for MailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("user", &self.user.as_ref().map(|_| "***"))
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("from", &self.from)
            .field("max_per_minute", &self.max_per_minute)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

pub(crate) struct ValidatedMailConfig {
    pub(crate) host: String,
    pub(crate) port: Option<NonZeroU16>,
    pub(crate) tls: TlsMode,
    pub(crate) credentials: Option<Credentials>,
    pub(crate) from: Mailbox,
    pub(crate) max_per_minute: u32,
    pub(crate) timeout: Duration,
}

impl MailConfig {
    pub fn new(host: impl Into<String>, tls: TlsMode, from: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: None,
            tls,
            user: None,
            password: None,
            from: from.into(),
            max_per_minute: DEFAULT_MAX_PER_MINUTE,
            timeout_secs: None,
        }
    }

    pub fn with_port(mut self, port: NonZeroU16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn with_credentials(
        mut self,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.user = Some(user.into());
        self.password = Some(password.into());
        self
    }

    pub fn with_max_per_minute(mut self, max_per_minute: u32) -> Self {
        self.max_per_minute = max_per_minute;
        self
    }

    pub fn with_timeout(mut self, timeout_secs: NonZeroU64) -> Self {
        self.timeout_secs = Some(timeout_secs);
        self
    }

    pub(crate) fn validate(
        &self,
        environment: Environment,
    ) -> Result<ValidatedMailConfig, MailError> {
        let host = self.validated_host()?;
        ensure_tls_mode_allowed(self.tls, &host, environment)?;
        Ok(ValidatedMailConfig {
            port: self.port,
            tls: self.tls,
            credentials: self.credentials()?,
            from: self.sender()?,
            max_per_minute: self.validated_max_per_minute()?,
            timeout: self.timeout(),
            host,
        })
    }

    fn validated_host(&self) -> Result<String, MailError> {
        let host = self.host.trim();
        if host.is_empty() {
            return Err(MailError::MissingHost);
        }
        Ok(host.to_owned())
    }

    fn credentials(&self) -> Result<Option<Credentials>, MailError> {
        match (
            non_blank(self.user.as_deref()),
            non_blank(self.password.as_deref()),
        ) {
            (Some(user), Some(password)) => {
                Ok(Some(Credentials::new(user.to_owned(), password.to_owned())))
            }
            (None, None) => Ok(None),
            _ => Err(MailError::PartialCredentials),
        }
    }

    fn sender(&self) -> Result<Mailbox, MailError> {
        self.from
            .parse::<Mailbox>()
            .map_err(|_| MailError::InvalidSender)
    }

    fn validated_max_per_minute(&self) -> Result<u32, MailError> {
        if self.max_per_minute == 0 {
            return Err(MailError::ZeroRateLimit);
        }
        Ok(self.max_per_minute)
    }

    fn timeout(&self) -> Duration {
        self.timeout_secs
            .map_or(DEFAULT_TIMEOUT, |secs| Duration::from_secs(secs.get()))
    }
}

fn ensure_tls_mode_allowed(
    tls: TlsMode,
    host: &str,
    environment: Environment,
) -> Result<(), MailError> {
    if tls != TlsMode::Plaintext {
        return Ok(());
    }
    if environment == Environment::Prod {
        return Err(MailError::PlaintextInProduction);
    }
    if !is_loopback_host(host) {
        return Err(MailError::PlaintextToRemoteHost);
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case(LOCALHOST)
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.filter(|content| !content.trim().is_empty())
}

fn default_max_per_minute() -> u32 {
    DEFAULT_MAX_PER_MINUTE
}
