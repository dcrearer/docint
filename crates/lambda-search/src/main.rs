//! Lambda: hybrid search over documents.
//! Accepts a text query, embeds it via Bedrock Titan, then runs
//! hybrid (vector + full-text) search with RRF ranking.

use docint_core::{lambda_init, store::VectorStore};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Deserialize)]
struct Request {
    query: String,
    #[serde(default = "default_tenant_id")]
    tenant_id: String,
    limit: Option<i64>,
}

fn default_tenant_id() -> String {
    std::env::var("DEFAULT_TENANT_ID").unwrap_or_else(|_| "default-tenant".to_string())
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

/// Shared state initialized once on cold start, reused across invocations.
/// OnceCell ensures the DB pool and embedder are created exactly once,
/// even if multiple requests arrive concurrently during init.
static STATE: OnceCell<lambda_init::AppState> = OnceCell::const_new();

async fn get_state() -> &'static lambda_init::AppState {
    STATE
        .get_or_init(|| async {
            lambda_init::init_app_state()
                .await
                .expect("Failed to initialize app state")
        })
        .await
}

async fn handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    use docint_core::db;

    let req = event.payload;
    let state = get_state().await;

    let embedding = state.embedder.embed(&req.query).await?;
    let limit = req.limit.unwrap_or(5).clamp(1, 50);
    let query = req.query.clone();
    let tenant_id = req.tenant_id.clone();

    let results = db::with_tenant(state.store.pool(), &req.tenant_id, move |tx| {
        Box::pin(async move {
            VectorStore::hybrid_search_tx(tx, &embedding, &query, &tenant_id, limit).await
        })
    })
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
    lambda_init::setup_tracing();
    lambda_runtime::run(service_fn(handler)).await
}
