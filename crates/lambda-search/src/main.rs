//! Lambda: hybrid search over documents.
//! Accepts a text query, embeds it via Bedrock Titan, then runs
//! hybrid (vector + full-text) search with RRF ranking.

use docint_core::{db, embeddings::Embedder, store::VectorStore};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Deserialize)]
struct Request {
    query: String,
    tenant_id: String,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct Response {
    results: Vec<SearchHit>,
}

#[derive(Serialize)]
struct SearchHit {
    chunk_id: String,
    document_id: String,
    title: String,
    content: String,
    distance: f64,
}

struct AppState {
    store: VectorStore,
    embedder: Embedder,
}

/// Shared state initialized once on cold start, reused across invocations.
/// OnceCell ensures the DB pool and embedder are created exactly once,
/// even if multiple requests arrive concurrently during init.
static STATE: OnceCell<AppState> = OnceCell::const_new();

async fn get_state() -> &'static AppState {
    STATE
        .get_or_init(|| async {
            let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
            let pool = db::create_pool(&url)
                .await
                .expect("Failed to connect to database");
            AppState {
                store: VectorStore::new(pool),
                embedder: Embedder::new().await,
            }
        })
        .await
}

async fn handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let req = event.payload;
    let state = get_state().await;
    state.store.set_tenant(&req.tenant_id).await?;

    let embedding = state.embedder.embed(&req.query).await?;
    let results = state
        .store
        .hybrid_search(&embedding, &req.query, &req.tenant_id, req.limit.unwrap_or(5))
        .await?;

    Ok(Response {
        results: results
            .into_iter()
            .map(|r| SearchHit {
                chunk_id: r.chunk_id.to_string(),
                document_id: r.document_id.to_string(),
                title: r.title,
                content: r.content,
                distance: r.distance,
            })
            .collect(),
    })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
