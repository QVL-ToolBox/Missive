mod common;

use common::config;
use missive::{Environment, MailConfig, MailError, Mailer, TlsMode};
use std::num::NonZeroU64;

fn build(config: &MailConfig, environment: Environment) -> Result<(), MailError> {
    Mailer::new(config, environment).map(drop)
}

fn with_credentials(user: Option<&str>, password: Option<&str>) -> MailConfig {
    let mut credentials = config("smtp.example.com", TlsMode::StartTls);
    credentials.user = user.map(str::to_string);
    credentials.password = password.map(str::to_string);
    credentials
}

fn with_sender(from: &str) -> MailConfig {
    let mut sender = config("smtp.example.com", TlsMode::Tls);
    sender.from = from.to_string();
    sender
}

fn parse(json: &str) -> Result<MailConfig, serde_json::Error> {
    serde_json::from_str(json)
}

#[test]
fn plaintext_to_loopback_hosts_in_dev_is_accepted() {
    for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
        let result = build(&config(host, TlsMode::Plaintext), Environment::Dev);
        assert!(result.is_ok(), "host {host}: {:?}", result.err());
    }
}

#[test]
fn plaintext_to_remote_hosts_in_dev_is_rejected() {
    for host in ["smtp.example.com", "10.0.0.1"] {
        let result = build(&config(host, TlsMode::Plaintext), Environment::Dev);
        assert!(
            matches!(result, Err(MailError::PlaintextToRemoteHost)),
            "host {host}: {:?}",
            result.err()
        );
    }
}

#[test]
fn plaintext_in_production_is_rejected_even_on_loopback() {
    let result = build(&config("127.0.0.1", TlsMode::Plaintext), Environment::Prod);
    assert!(
        matches!(result, Err(MailError::PlaintextInProduction)),
        "{:?}",
        result.err()
    );
}

#[test]
fn encrypted_modes_to_remote_host_are_accepted_in_every_environment() {
    for tls in [TlsMode::Tls, TlsMode::StartTls] {
        for environment in [Environment::Dev, Environment::Prod] {
            let result = build(&config("smtp.example.com", tls), environment);
            assert!(
                result.is_ok(),
                "{tls:?} {environment:?}: {:?}",
                result.err()
            );
        }
    }
}

#[test]
fn partial_credentials_are_rejected() {
    let partial = [
        (Some("mailer-user"), None),
        (None, Some("mailer-secret")),
        (Some("mailer-user"), Some("   ")),
        (Some("mailer-user"), Some("")),
    ];
    for (user, password) in partial {
        let result = build(&with_credentials(user, password), Environment::Prod);
        assert!(
            matches!(result, Err(MailError::PartialCredentials)),
            "{user:?}/{password:?}: {:?}",
            result.err()
        );
    }
}

#[test]
fn complete_or_absent_credentials_are_accepted() {
    for (user, password) in [(Some("mailer-user"), Some("mailer-secret")), (None, None)] {
        let result = build(&with_credentials(user, password), Environment::Prod);
        assert!(result.is_ok(), "{user:?}/{password:?}: {:?}", result.err());
    }
}

#[test]
fn invalid_sender_is_rejected() {
    for from in [
        "",
        "not-an-address",
        "Nom <pas-une-adresse>",
        "a@b.test, c@d.test",
    ] {
        let result = build(&with_sender(from), Environment::Dev);
        assert!(
            matches!(result, Err(MailError::InvalidSender)),
            "from {from:?}: {:?}",
            result.err()
        );
    }
}

#[test]
fn valid_sender_forms_are_accepted() {
    for from in ["Nom <a@b.test>", "a@b.test"] {
        let result = build(&with_sender(from), Environment::Dev);
        assert!(result.is_ok(), "from {from:?}: {:?}", result.err());
    }
}

#[test]
fn zero_rate_limit_is_rejected() {
    let zero = config("smtp.example.com", TlsMode::Tls).with_max_per_minute(0);
    let result = build(&zero, Environment::Prod);
    assert!(
        matches!(result, Err(MailError::ZeroRateLimit)),
        "{:?}",
        result.err()
    );
}

#[test]
fn empty_or_blank_host_is_rejected() {
    for host in ["", "   "] {
        let result = build(&config(host, TlsMode::Tls), Environment::Dev);
        assert!(
            matches!(result, Err(MailError::MissingHost)),
            "host {host:?}: {:?}",
            result.err()
        );
    }
}

#[test]
fn rate_limit_defaults_to_sixty_when_absent() {
    let parsed = parse(r#"{"host":"smtp.example.com","tls":"tls","from":"a@b.test"}"#).unwrap();
    assert_eq!(parsed.max_per_minute, 60);
}

#[test]
fn known_tls_modes_are_deserialized() {
    let expected = [
        ("tls", TlsMode::Tls),
        ("starttls", TlsMode::StartTls),
        ("none", TlsMode::Plaintext),
    ];
    for (raw, mode) in expected {
        let parsed = parse(&format!(
            r#"{{"host":"h","tls":"{raw}","from":"a@b.test"}}"#
        ))
        .unwrap();
        assert_eq!(parsed.tls, mode);
    }
}

#[test]
fn unknown_tls_mode_is_a_deserialization_error() {
    for raw in ["ssl", "TLS", ""] {
        let result = parse(&format!(
            r#"{{"host":"h","tls":"{raw}","from":"a@b.test"}}"#
        ));
        assert!(result.is_err(), "{raw:?}");
    }
}

#[test]
fn missing_tls_mode_is_a_deserialization_error() {
    assert!(parse(r#"{"host":"h","from":"a@b.test"}"#).is_err());
}

#[test]
fn environment_is_deserialized_from_lowercase_names() {
    let dev: Environment = serde_json::from_str(r#""dev""#).unwrap();
    let prod: Environment = serde_json::from_str(r#""prod""#).unwrap();
    assert_eq!(dev, Environment::Dev);
    assert_eq!(prod, Environment::Prod);
}

#[test]
fn config_debug_hides_credentials() {
    let secret = with_credentials(Some("user-marker-7f3a"), Some("password-marker-9c1e"));
    let debug = format!("{secret:?}");
    assert!(!debug.contains("user-marker-7f3a"), "{debug}");
    assert!(!debug.contains("password-marker-9c1e"), "{debug}");
}

#[test]
fn mailer_is_built_and_dropped_without_async_runtime() {
    let configs = [
        config("smtp.example.com", TlsMode::Tls),
        config("smtp.example.com", TlsMode::StartTls),
        config("127.0.0.1", TlsMode::Plaintext),
    ];
    for mode in configs {
        let mailer = Mailer::new(&mode, Environment::Dev);
        assert!(mailer.is_ok(), "{:?}: {:?}", mode.tls, mailer.err());
        drop(mailer);
    }
}

#[test]
fn zero_port_is_a_deserialization_error() {
    let result = parse(r#"{"host":"h","port":0,"tls":"tls","from":"a@b.test"}"#);
    assert!(result.is_err());
}

#[test]
fn explicit_port_is_deserialized() {
    let parsed = parse(r#"{"host":"h","port":587,"tls":"starttls","from":"a@b.test"}"#).unwrap();
    assert_eq!(parsed.port.map(|port| port.get()), Some(587));
}

fn timeout(seconds: u64) -> NonZeroU64 {
    NonZeroU64::new(seconds).unwrap()
}

#[test]
fn zero_timeout_is_a_deserialization_error() {
    let result = parse(r#"{"host":"h","tls":"tls","from":"a@b.test","timeout_secs":0}"#);
    assert!(result.is_err());
}

#[test]
fn explicit_timeout_is_deserialized() {
    let parsed = parse(r#"{"host":"h","tls":"tls","from":"a@b.test","timeout_secs":5}"#).unwrap();
    assert_eq!(parsed.timeout_secs, Some(timeout(5)));
}

#[test]
fn timeout_is_absent_when_not_deserialized() {
    let parsed = parse(r#"{"host":"h","tls":"tls","from":"a@b.test"}"#).unwrap();
    assert_eq!(parsed.timeout_secs, None);
}

#[test]
fn new_config_has_no_explicit_timeout() {
    assert_eq!(config("smtp.example.com", TlsMode::Tls).timeout_secs, None);
}

#[test]
fn with_timeout_sets_explicit_timeout() {
    let configured = config("smtp.example.com", TlsMode::Tls).with_timeout(timeout(7));
    assert_eq!(configured.timeout_secs, Some(timeout(7)));
}

#[test]
fn mailer_is_built_with_explicit_timeout_in_every_tls_mode() {
    let configs = [
        config("smtp.example.com", TlsMode::Tls),
        config("smtp.example.com", TlsMode::StartTls),
        config("127.0.0.1", TlsMode::Plaintext),
    ];
    for mode in configs {
        let configured = mode.with_timeout(timeout(3));
        let result = build(&configured, Environment::Dev);
        assert!(result.is_ok(), "{:?}: {:?}", configured.tls, result.err());
    }
}

#[test]
fn config_debug_shows_timeout_and_hides_credentials() {
    let secret = with_credentials(Some("user-marker-7f3a"), Some("password-marker-9c1e"))
        .with_timeout(timeout(9));
    let debug = format!("{secret:?}");
    assert!(debug.contains("timeout_secs: Some(9)"), "{debug}");
    assert!(!debug.contains("user-marker-7f3a"), "{debug}");
    assert!(!debug.contains("password-marker-9c1e"), "{debug}");
}
