use crate::mailer::Mailer;
use crate::rate_limit::RateLimiter;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub mailer: Arc<Mailer>,
    pub internal_api_secret: Arc<str>,
    pub rate_limiter: Arc<RateLimiter>,
}
