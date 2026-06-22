//! Binário do servidor do livro de ordens (Parte 5).
//! Toda a lógica HTTP vive em `orderbook::server`; aqui só fazemos o bind.

use orderbook::server::{build_router, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orderbook=info".into()),
        )
        .init();

    let app = build_router(AppState::new());

    let addr = std::env::var("KAEL_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Kael orderbook ouvindo em http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
