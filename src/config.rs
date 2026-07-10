use lettre::message::Mailbox;
use std::net::{IpAddr, Ipv4Addr};

pub const DEFAULT_PORT: u16 = 8184;
pub const MIN_INTERNAL_API_SECRET_BYTES: usize = 32;
pub const DEFAULT_RATE_LIMIT_PER_MINUTE: u32 = 60;

const DEFAULT_BIND_ADDR: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("variable d'environnement requise manquante ou vide : {0}")]
    MissingVar(&'static str),
    #[error("valeur invalide pour {0} : {1}")]
    InvalidValue(&'static str, String),
    #[error("MAIL_FROM invalide : {0}")]
    InvalidMailFrom(String),
    #[error(
        "INTERNAL_API_SECRET trop court : {0} octets (minimum {MIN_INTERNAL_API_SECRET_BYTES})"
    )]
    WeakInternalApiSecret(usize),
    #[error("bind non-loopback {0} refusé sans ALLOW_EXTERNAL_BIND=true")]
    ExternalBindRefused(IpAddr),
}

#[derive(Clone)]
pub struct Config {
    pub bind_addr: IpAddr,
    pub port: u16,
    pub smtp: SmtpConfig,
    pub mail_from: Mailbox,
    pub internal_api_secret: String,
    pub rate_limit_per_minute: u32,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("bind_addr", &self.bind_addr)
            .field("port", &self.port)
            .field("smtp", &self.smtp)
            .field("mail_from", &self.mail_from)
            .field("internal_api_secret", &"***")
            .field("rate_limit_per_minute", &self.rate_limit_per_minute)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: Option<u16>,
    pub credentials: Option<SmtpCredentials>,
}

#[derive(Clone)]
pub struct SmtpCredentials {
    pub user: String,
    pub password: String,
}

impl std::fmt::Debug for SmtpCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpCredentials")
            .field("user", &"***")
            .field("password", &"***")
            .finish()
    }
}

pub fn load() -> Result<Config, ConfigError> {
    Ok(Config {
        internal_api_secret: load_internal_api_secret()?,
        bind_addr: load_bind_addr()?,
        port: load_port()?,
        smtp: load_smtp()?,
        mail_from: load_mail_from()?,
        rate_limit_per_minute: load_rate_limit_per_minute()?,
    })
}

fn load_internal_api_secret() -> Result<String, ConfigError> {
    let secret = require("INTERNAL_API_SECRET")?;
    let len = secret.len();
    if len < MIN_INTERNAL_API_SECRET_BYTES {
        return Err(ConfigError::WeakInternalApiSecret(len));
    }
    Ok(secret)
}

fn load_rate_limit_per_minute() -> Result<u32, ConfigError> {
    match optional("RATE_LIMIT_PER_MINUTE") {
        None => Ok(DEFAULT_RATE_LIMIT_PER_MINUTE),
        Some(raw) => raw
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or(ConfigError::InvalidValue("RATE_LIMIT_PER_MINUTE", raw)),
    }
}

fn load_bind_addr() -> Result<IpAddr, ConfigError> {
    let addr = match optional("BIND_ADDR") {
        None => DEFAULT_BIND_ADDR,
        Some(raw) => raw
            .parse::<IpAddr>()
            .map_err(|_| ConfigError::InvalidValue("BIND_ADDR", raw))?,
    };
    if !addr.is_loopback() && !allow_external_bind() {
        return Err(ConfigError::ExternalBindRefused(addr));
    }
    Ok(addr)
}

fn allow_external_bind() -> bool {
    optional("ALLOW_EXTERNAL_BIND")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn load_port() -> Result<u16, ConfigError> {
    match optional("PORT") {
        None => Ok(DEFAULT_PORT),
        Some(raw) => raw
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or(ConfigError::InvalidValue("PORT", raw)),
    }
}

fn load_smtp() -> Result<SmtpConfig, ConfigError> {
    Ok(SmtpConfig {
        host: require("SMTP_HOST")?,
        port: parse_optional_port("SMTP_PORT")?,
        credentials: load_smtp_credentials(),
    })
}

fn load_smtp_credentials() -> Option<SmtpCredentials> {
    match (optional("SMTP_USER"), optional("SMTP_PASSWORD")) {
        (Some(user), Some(password)) => Some(SmtpCredentials { user, password }),
        _ => None,
    }
}

fn parse_optional_port(name: &'static str) -> Result<Option<u16>, ConfigError> {
    match optional(name) {
        None => Ok(None),
        Some(raw) => raw
            .parse::<u16>()
            .map(Some)
            .map_err(|_| ConfigError::InvalidValue(name, raw)),
    }
}

fn load_mail_from() -> Result<Mailbox, ConfigError> {
    let raw = require("MAIL_FROM")?;
    raw.parse::<Mailbox>()
        .map_err(|_| ConfigError::InvalidMailFrom(raw))
}

fn require(name: &'static str) -> Result<String, ConfigError> {
    optional(name).ok_or(ConfigError::MissingVar(name))
}

fn optional(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
