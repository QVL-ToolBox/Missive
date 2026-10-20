mod config;
mod error;
mod mailer;
mod rate_limit;

pub use config::{Environment, MailConfig, TlsMode};
pub use error::MailError;
pub use mailer::{Body, Mailer};
