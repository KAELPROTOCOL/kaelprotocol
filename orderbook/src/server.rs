//! Casca HTTP do livro (Parte 5). O router vive no lib para que tanto o binário
//! quanto os testes de integração usem EXATAMENTE o mesmo código de produção.
//!
//! INVARIANTE: o servidor só INFORMA. Nunca custodia, executa, altera nem
//! prioriza. O relógio do sistema entra só aqui (na casca), nunca na lógica pura.

use crate::book::{Book, Eip712Verifier, MatchPair, SubmitError};
use crate::eip712::VerifyError;
use crate::wire::{self, SubmitRequest, SubmitResponse};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

pub type SharedBook = Arc<RwLock<Book<Eip712Verifier>>>;

#[derive(Clone)]
pub struct AppState {
    pub book: SharedBook,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            book: Arc::new(RwLock::new(Book::new(Eip712Verifier))),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// Sobe o servidor numa porta efêmera (127.0.0.1:0) e devolve a base URL.
/// Útil para testes de integração e demonstrações do fluxo completo.
pub async fn spawn_ephemeral() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_router(AppState::new());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/orders", post(submit_order))
        .route("/matches", get(get_matches))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn submit_order(
    State(st): State<AppState>,
    Json(req): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, (StatusCode, String)> {
    let now = now_unix();
    let order = req
        .order
        .into_order(now)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("ordem inválida: {e:?}")))?;
    let sig = wire::parse_signature(&req.signature)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("assinatura inválida: {e:?}")))?;

    let mut book = st.book.write().await;
    match book.submit(order, &sig, now) {
        Ok(hash) => Ok(Json(SubmitResponse {
            accepted: true,
            order_hash: format!("0x{}", hex::encode(hash)),
        })),
        Err(SubmitError::Duplicate) => Err((StatusCode::CONFLICT, "ordem duplicada".into())),
        Err(SubmitError::Verify(v)) => Err((StatusCode::UNPROCESSABLE_ENTITY, verify_msg(v))),
    }
}

fn verify_msg(v: VerifyError) -> String {
    match v {
        VerifyError::OrderExpired => "ordem expirada".into(),
        VerifyError::SignerNotMaker => "assinatura não corresponde ao maker".into(),
        VerifyError::MalleableS => "assinatura maleável (s alto)".into(),
        other => format!("assinatura rejeitada: {other:?}"),
    }
}

#[derive(Deserialize)]
struct MatchesQuery {
    maker: String,
}

async fn get_matches(
    State(st): State<AppState>,
    Query(q): Query<MatchesQuery>,
) -> Result<Json<Vec<MatchPair>>, (StatusCode, String)> {
    let maker = wire::parse_address(&q.maker)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("maker inválido: {e:?}")))?;
    let book = st.book.read().await;
    Ok(Json(book.matches_for(&maker, now_unix())))
}
