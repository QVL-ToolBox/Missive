#![allow(dead_code)]

use std::io::Write;
use std::net::TcpListener;
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex};

use missive::{MailConfig, TlsMode};
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::fmt::MakeWriter;

const SERVICE_UNAVAILABLE_GREETING: &[u8] = b"421 missive.test Service not available\r\n";
pub const VALID_SENDER: &str = "Missive <noreply@missive.test>";

pub fn config(host: &str, tls: TlsMode) -> MailConfig {
    MailConfig::new(host, tls, VALID_SENDER)
}

pub fn spawn_refusing_smtp_server() -> NonZeroU16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut connection in listener.incoming().flatten() {
            let _ = connection.write_all(SERVICE_UNAVAILABLE_GREETING);
        }
    });
    NonZeroU16::new(port).unwrap()
}

pub fn spawn_silent_smtp_server() -> NonZeroU16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let held_open: Vec<_> = listener.incoming().flatten().collect();
        drop(held_open);
    });
    NonZeroU16::new(port).unwrap()
}

pub fn silent_local_config() -> MailConfig {
    config("127.0.0.1", TlsMode::Plaintext).with_port(spawn_silent_smtp_server())
}

pub fn refusing_local_config(max_per_minute: u32) -> MailConfig {
    config("127.0.0.1", TlsMode::Plaintext)
        .with_port(spawn_refusing_smtp_server())
        .with_max_per_minute(max_per_minute)
}

#[derive(Clone, Default)]
pub struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    pub fn install() -> (Self, DefaultGuard) {
        let capture = Self::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::TRACE)
            .with_current_span(true)
            .with_span_list(true)
            .with_writer(capture.clone())
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        (capture, guard)
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    pub fn lines(&self) -> Vec<String> {
        self.text().lines().map(str::to_string).collect()
    }

    pub fn smtp_attempt_lines(&self) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|line| line.contains("latency_ms"))
            .collect()
    }
}

impl Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
