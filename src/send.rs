use crate::mailer::{Mailer, MailerError};
use crate::rate_limit::RateLimiter;
use crate::state::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

pub const MAX_BODY_BYTES: usize = 512 * 1024;

const INTERNAL_SECRET_HEADER: &str = "x-internal-secret";
const CHANNEL_EMAIL: &str = "email";
const CHANNEL_PUSH: &str = "push";

#[derive(Deserialize)]
pub struct SendRequest {
    pub channel: String,
    pub to: String,
    pub subject: String,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Serialize)]
pub struct SendResponse {
    status: &'static str,
}

pub(crate) enum SendError {
    Unauthorized,
    BadRequest,
    NotImplemented,
    TooManyRequests,
    UpstreamFailure,
    Internal,
}

impl IntoResponse for SendError {
    fn into_response(self) -> Response {
        let status = match self {
            SendError::Unauthorized => StatusCode::UNAUTHORIZED,
            SendError::BadRequest => StatusCode::BAD_REQUEST,
            SendError::NotImplemented => StatusCode::NOT_IMPLEMENTED,
            SendError::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            SendError::UpstreamFailure => StatusCode::BAD_GATEWAY,
            SendError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        status.into_response()
    }
}

struct EmailDispatch {
    to: String,
    subject: String,
    html: Option<String>,
    text: Option<String>,
}

pub async fn send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SendRequest>,
) -> Result<Json<SendResponse>, SendError> {
    authorize(&headers, &state.internal_api_secret)?;
    enforce_rate_limit(&state.rate_limiter)?;
    let dispatch = validate(request)?;
    deliver(&state.mailer, dispatch).await?;
    Ok(Json(SendResponse { status: "sent" }))
}

fn authorize(headers: &HeaderMap, expected: &str) -> Result<(), SendError> {
    let provided = headers
        .get(INTERNAL_SECRET_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if secret_matches(provided, expected) {
        Ok(())
    } else {
        Err(SendError::Unauthorized)
    }
}

fn secret_matches(provided: &str, expected: &str) -> bool {
    if provided.is_empty() {
        return false;
    }
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

fn enforce_rate_limit(limiter: &RateLimiter) -> Result<(), SendError> {
    if limiter.try_acquire() {
        Ok(())
    } else {
        Err(SendError::TooManyRequests)
    }
}

fn validate(request: SendRequest) -> Result<EmailDispatch, SendError> {
    match request.channel.as_str() {
        CHANNEL_EMAIL => {}
        CHANNEL_PUSH => return Err(SendError::NotImplemented),
        _ => return Err(SendError::BadRequest),
    }
    if request.to.contains(',') {
        return Err(SendError::BadRequest);
    }
    request
        .to
        .parse::<Mailbox>()
        .map_err(|_| SendError::BadRequest)?;
    if request.subject.contains(['\r', '\n']) {
        return Err(SendError::BadRequest);
    }
    let html = non_empty(request.html);
    let text = non_empty(request.text);
    if html.is_none() && text.is_none() {
        return Err(SendError::BadRequest);
    }
    Ok(EmailDispatch {
        to: request.to,
        subject: request.subject,
        html,
        text,
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|content| !content.is_empty())
}

async fn deliver(mailer: &Mailer, dispatch: EmailDispatch) -> Result<(), SendError> {
    mailer
        .send(
            &dispatch.to,
            &dispatch.subject,
            dispatch.html.as_deref(),
            dispatch.text.as_deref(),
        )
        .await
        .map_err(map_mailer_error)
}

fn map_mailer_error(error: MailerError) -> SendError {
    match error {
        MailerError::Send(_) => SendError::UpstreamFailure,
        _ => SendError::Internal,
    }
}
