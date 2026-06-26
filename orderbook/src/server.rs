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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

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
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid order: {e:?}")))?;
    let sig = wire::parse_signature(&req.signature)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid signature: {e:?}")))?;

    let mut book = st.book.write().await;
    match book.submit(order, &sig, now) {
        Ok(hash) => Ok(Json(SubmitResponse {
            accepted: true,
            order_hash: format!("0x{}", hex::encode(hash)),
        })),
        Err(SubmitError::Duplicate) => Err((StatusCode::CONFLICT, "duplicate order".into())),
        Err(SubmitError::Verify(v)) => Err((StatusCode::UNPROCESSABLE_ENTITY, verify_msg(v))),
    }
}

fn verify_msg(v: VerifyError) -> String {
    match v {
        VerifyError::OrderExpired => "expired order".into(),
        VerifyError::SignerNotMaker => "signature does not match maker".into(),
        VerifyError::MalleableS => "malleable signature (high s)".into(),
        other => format!("signature rejected: {other:?}"),
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
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid maker: {e:?}")))?;
    let book = st.book.read().await;
    Ok(Json(book.matches_for(&maker, now_unix())))
}
