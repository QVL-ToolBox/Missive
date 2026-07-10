mod config;
mod health;
mod mailer;

use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::process::ExitCode;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> ExitCode {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = match config::load() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "démarrage refusé : configuration invalide");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = mailer::Mailer::from_config(&config) {
        tracing::error!(%error, "démarrage refusé : transport SMTP inconstruisible");
        return ExitCode::FAILURE;
    }

    let addr = SocketAddr::new(config.bind_addr, config.port);
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%addr, %error, "démarrage refusé : écoute impossible");
            return ExitCode::FAILURE;
        }
    };

    let app = Router::new().route("/health", get(health::health));

    tracing::info!(%addr, version = env!("CARGO_PKG_VERSION"), "Missive démarré");
    if let Err(error) = axum::serve(listener, app).await {
        tracing::error!(%error, "arrêt du serveur sur erreur");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .init();
}
