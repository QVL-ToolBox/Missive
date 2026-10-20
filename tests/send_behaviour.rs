mod common;

use common::{refusing_local_config, silent_local_config, LogCapture};
use std::num::{NonZeroU16, NonZeroU64};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use missive::{Body, Environment, MailConfig, MailError, Mailer, TlsMode};

const VALID_RECIPIENT: &str = "someone@x.test";
const VALID_SUBJECT: &str = "Hello";
const VALID_TEXT: Body<'static> = Body::Text("Body");

const SUBJECT_MARKER: &str = "subject-marker-a91f";
const BODY_MARKER: &str = "body-marker-b72e";
const HTML_MARKER: &str = "html-marker-c53d";
const LOCAL_PART_MARKER: &str = "localpart-marker-d34c";
const USER_MARKER: &str = "user-marker-e15b";
const PASSWORD_MARKER: &str = "password-marker-f06a";
const RECIPIENT_DOMAIN: &str = "leak-check.test";

const MARKERS: [&str; 6] = [
    SUBJECT_MARKER,
    BODY_MARKER,
    HTML_MARKER,
    LOCAL_PART_MARKER,
    USER_MARKER,
    PASSWORD_MARKER,
];

fn refusing_mailer(max_per_minute: u32) -> Mailer {
    Mailer::new(&refusing_local_config(max_per_minute), Environment::Dev).unwrap()
}

fn marked_recipient() -> String {
    format!("{LOCAL_PART_MARKER}@{RECIPIENT_DOMAIN}")
}

fn marked_config() -> MailConfig {
    refusing_local_config(60).with_credentials(USER_MARKER, PASSWORD_MARKER)
}

fn marked_mailer(max_per_minute: u32) -> Mailer {
    let config = marked_config().with_max_per_minute(max_per_minute);
    Mailer::new(&config, Environment::Dev).unwrap()
}

fn marked_config_with(change: impl FnOnce(&mut MailConfig)) -> MailConfig {
    let mut config = marked_config();
    change(&mut config);
    config
}

fn startup_error(config: MailConfig, environment: Environment) -> MailError {
    Mailer::new(&config, environment).err().unwrap()
}

fn assert_no_marker(haystack: &str) {
    for marker in MARKERS {
        assert!(
            !haystack.contains(marker),
            "marker {marker} leaked in: {haystack}"
        );
    }
}

#[tokio::test]
async fn multiple_or_malformed_recipients_are_rejected() {
    let mailer = refusing_mailer(60);
    for to in [
        "a@x.test,b@x.test",
        "invalide",
        "",
        "Nom <a@x.test>, b@x.test",
    ] {
        let result = mailer.send(to, VALID_SUBJECT, VALID_TEXT).await;
        assert!(
            matches!(result, Err(MailError::InvalidRecipient)),
            "to {to:?}: {result:?}"
        );
    }
}

#[tokio::test]
async fn subject_with_line_break_is_rejected() {
    let mailer = refusing_mailer(60);
    for subject in ["Hello\rBcc: x@y.test", "Hello\nBcc: x@y.test", "Hello\r\n"] {
        let result = mailer.send(VALID_RECIPIENT, subject, VALID_TEXT).await;
        assert!(
            matches!(result, Err(MailError::InvalidSubject)),
            "subject {subject:?}: {result:?}"
        );
    }
}

#[tokio::test]
async fn empty_or_missing_body_is_rejected() {
    let mailer = refusing_mailer(60);
    let bodies = [
        Body::Text(""),
        Body::Html(""),
        Body::Both { text: "", html: "" },
        Body::Both {
            text: "Body",
            html: "",
        },
        Body::Both {
            text: "",
            html: "<p>Hi</p>",
        },
    ];
    for body in bodies {
        let result = mailer.send(VALID_RECIPIENT, VALID_SUBJECT, body).await;
        assert!(
            matches!(result, Err(MailError::EmptyBody)),
            "{body:?}: {result:?}"
        );
    }
}

#[tokio::test]
async fn html_only_body_reaches_smtp_layer() {
    let mailer = refusing_mailer(60);
    let result = mailer
        .send(VALID_RECIPIENT, VALID_SUBJECT, Body::Html("<p>Hi</p>"))
        .await;
    assert!(matches!(result, Err(MailError::Send { .. })), "{result:?}");
}

#[tokio::test]
async fn send_beyond_rate_limit_is_refused_without_smtp_call() {
    let limit: u32 = 3;
    let (logs, _guard) = LogCapture::install();
    let mailer = refusing_mailer(limit);

    for attempt in 1..=limit {
        let result = mailer
            .send(VALID_RECIPIENT, VALID_SUBJECT, VALID_TEXT)
            .await;
        assert!(
            matches!(result, Err(MailError::Send { .. })),
            "attempt {attempt}: {result:?}"
        );
    }
    let attempts_before_limit = logs.smtp_attempt_lines().len();

    let over_limit = mailer
        .send(VALID_RECIPIENT, VALID_SUBJECT, VALID_TEXT)
        .await;

    assert!(
        matches!(over_limit, Err(MailError::RateLimited)),
        "{over_limit:?}"
    );
    assert_eq!(attempts_before_limit, limit as usize, "{}", logs.text());
    assert_eq!(
        logs.smtp_attempt_lines().len(),
        limit as usize,
        "{}",
        logs.text()
    );
}

#[tokio::test]
async fn invalid_sends_do_not_consume_quota() {
    let mailer = refusing_mailer(1);

    let rejected = [
        mailer.send("invalide", VALID_SUBJECT, VALID_TEXT).await,
        mailer.send(VALID_RECIPIENT, "a\nb", VALID_TEXT).await,
        mailer
            .send(VALID_RECIPIENT, VALID_SUBJECT, Body::Text(""))
            .await,
    ];
    let first_valid = mailer
        .send(VALID_RECIPIENT, VALID_SUBJECT, VALID_TEXT)
        .await;
    let second_valid = mailer
        .send(VALID_RECIPIENT, VALID_SUBJECT, VALID_TEXT)
        .await;

    assert!(rejected.iter().all(Result::is_err), "{rejected:?}");
    assert!(
        matches!(first_valid, Err(MailError::Send { .. })),
        "{first_valid:?}"
    );
    assert!(
        matches!(second_valid, Err(MailError::RateLimited)),
        "{second_valid:?}"
    );
}

#[tokio::test]
async fn failed_send_logs_contain_no_sensitive_data() {
    let (logs, _guard) = LogCapture::install();
    let mailer = marked_mailer(60);

    let result = mailer
        .send(
            &marked_recipient(),
            SUBJECT_MARKER,
            Body::Both {
                text: BODY_MARKER,
                html: HTML_MARKER,
            },
        )
        .await;

    assert!(matches!(result, Err(MailError::Send { .. })), "{result:?}");
    let output = logs.text();
    assert!(!output.is_empty());
    assert_no_marker(&output);
    assert!(output.contains("recipient_domain"), "{output}");
    assert!(output.contains(RECIPIENT_DOMAIN), "{output}");
    assert!(output.contains("latency_ms"), "{output}");
}

#[tokio::test]
async fn smtp_failure_warning_reports_code_and_permanence() {
    let (logs, _guard) = LogCapture::install();
    let mailer = refusing_mailer(60);

    let result = mailer
        .send(VALID_RECIPIENT, VALID_SUBJECT, VALID_TEXT)
        .await;

    assert!(matches!(result, Err(MailError::Send { .. })), "{result:?}");
    let warnings: Vec<String> = logs
        .lines()
        .into_iter()
        .filter(|line| line.contains(r#""level":"WARN""#))
        .collect();
    assert_eq!(warnings.len(), 1, "{}", logs.text());
    assert!(warnings[0].contains(r#""smtp_code":"#), "{}", warnings[0]);
    assert!(warnings[0].contains("421"), "{}", warnings[0]);
    assert!(
        warnings[0].contains(r#""permanent":false"#),
        "{}",
        warnings[0]
    );
}

#[tokio::test]
async fn send_error_debug_contains_no_sensitive_data() {
    let mailer = marked_mailer(60);
    let body = Body::Both {
        text: BODY_MARKER,
        html: HTML_MARKER,
    };

    let result = mailer.send(&marked_recipient(), SUBJECT_MARKER, body).await;

    let error = result.err().unwrap();
    assert!(matches!(error, MailError::Send { .. }), "{error:?}");
    assert_no_marker(&format!("{error:?}"));
}

#[test]
fn body_debug_contains_no_content() {
    let bodies = [
        Body::Text(BODY_MARKER),
        Body::Html(HTML_MARKER),
        Body::Both {
            text: BODY_MARKER,
            html: HTML_MARKER,
        },
    ];
    for body in bodies {
        assert_no_marker(&format!("{body:?}"));
    }
}

#[tokio::test]
async fn rejected_and_rate_limited_send_logs_contain_no_sensitive_data() {
    let (logs, _guard) = LogCapture::install();
    let mailer = marked_mailer(1);
    let recipient = marked_recipient();
    let injected_subject = format!("{SUBJECT_MARKER}\n");

    let results = [
        mailer
            .send(&recipient, &injected_subject, Body::Text(BODY_MARKER))
            .await,
        mailer
            .send(&recipient, SUBJECT_MARKER, Body::Text(BODY_MARKER))
            .await,
        mailer
            .send(&recipient, SUBJECT_MARKER, Body::Text(BODY_MARKER))
            .await,
    ];

    assert!(results.iter().all(Result::is_err), "{results:?}");
    assert_no_marker(&logs.text());
}

#[tokio::test]
async fn error_display_never_contains_sensitive_data() {
    let mut errors = vec![
        startup_error(
            marked_config_with(|config| config.password = None),
            Environment::Dev,
        ),
        startup_error(
            marked_config_with(|config| config.from = format!("{LOCAL_PART_MARKER}-{USER_MARKER}")),
            Environment::Dev,
        ),
        startup_error(marked_config(), Environment::Prod),
        startup_error(
            marked_config_with(|config| config.host = "smtp.example.com".to_string()),
            Environment::Dev,
        ),
        startup_error(marked_config().with_max_per_minute(0), Environment::Dev),
        startup_error(
            marked_config_with(|config| config.host = " ".to_string()),
            Environment::Dev,
        ),
    ];

    let mailer = marked_mailer(1);
    let recipient = marked_recipient();
    let malformed_recipient = format!("{LOCAL_PART_MARKER}-{SUBJECT_MARKER}");
    let injected_subject = format!("{SUBJECT_MARKER}\r\n{BODY_MARKER}");
    let both_markers = Body::Both {
        text: BODY_MARKER,
        html: HTML_MARKER,
    };
    let attempts = [
        mailer
            .send(
                &malformed_recipient,
                SUBJECT_MARKER,
                Body::Text(BODY_MARKER),
            )
            .await,
        mailer
            .send(&recipient, &injected_subject, Body::Text(BODY_MARKER))
            .await,
        mailer
            .send(&recipient, SUBJECT_MARKER, Body::Text(""))
            .await,
        mailer.send(&recipient, SUBJECT_MARKER, both_markers).await,
        mailer.send(&recipient, SUBJECT_MARKER, both_markers).await,
    ];
    errors.extend(attempts.into_iter().map(|result| result.err().unwrap()));

    let kinds: Vec<String> = errors.iter().map(|error| format!("{error:?}")).collect();
    assert_eq!(errors.len(), 11, "{kinds:?}");
    for error in &errors {
        assert_no_marker(&error.to_string());
    }
}

type TimedSend = (Result<(), MailError>, Duration, String);

fn send_with_deadline(config: MailConfig, deadline: Duration) -> Option<TimedSend> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let (logs, _guard) = LogCapture::install();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mailer = Mailer::new(&config, Environment::Dev).unwrap();
        let started = Instant::now();
        let result = runtime.block_on(mailer.send(VALID_RECIPIENT, VALID_SUBJECT, VALID_TEXT));
        let _ = sender.send((result, started.elapsed(), logs.text()));
    });
    receiver.recv_timeout(deadline).ok()
}

#[test]
fn silent_server_send_fails_within_configured_timeout() {
    let config = silent_local_config().with_timeout(NonZeroU64::new(1).unwrap());

    let outcome = send_with_deadline(config, Duration::from_secs(5));

    let (result, elapsed, logs) = outcome.expect("send still pending after 5s with a 1s timeout");
    assert!(matches!(result, Err(MailError::Send { .. })), "{result:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert!(logs.contains(r#""timed_out":true"#), "{logs}");
}

#[test]
#[ignore]
fn silent_server_send_fails_within_default_timeout() {
    let outcome = send_with_deadline(silent_local_config(), Duration::from_secs(20));

    let (result, elapsed, _) =
        outcome.expect("send still pending after 20s with the default timeout");
    assert!(matches!(result, Err(MailError::Send { .. })), "{result:?}");
    assert!(elapsed >= Duration::from_secs(14), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
}

#[tokio::test]
#[ignore]
async fn successful_send_through_local_mailpit() {
    let config =
        common::config("localhost", TlsMode::Plaintext).with_port(NonZeroU16::new(1025).unwrap());
    let mailer = Mailer::new(&config, Environment::Dev).unwrap();

    let result = mailer
        .send(
            "recipient@missive.test",
            "Missive v2 mailpit check",
            Body::Both {
                text: "Hello from Missive",
                html: "<p>Hello</p>",
            },
        )
        .await;

    assert!(result.is_ok(), "{result:?}");
}
