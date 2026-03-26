//! Lambda: document comparison.
//! Takes a query and two document IDs, finds the most relevant chunks
//! from each document, and returns them side-by-side for comparison.

use docint_core::{db, embeddings::Embedder, store::VectorStore};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use uuid::Uuid;

#[derive(Deserialize)]
struct Request {
    query: String,
    document_id_a: Uuid,
    document_id_b: Uuid,
    tenant_id: String,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct ChunkHit {
    chunk_id: String,
    content: String,
    distance: f64,
}

#[derive(Serialize)]
struct DocResult {
    document_id: String,
    title: String,
    matches: Vec<ChunkHit>,
}

#[derive(Serialize)]
struct Response {
    query: String,
    document_a: DocResult,
    document_b: DocResult,
}

struct AppState {
    store: VectorStore,
    embedder: Embedder,
}

static STATE: OnceCell<AppState> = OnceCell::const_new();

async fn get_state() -> &'static AppState {
    STATE
        .get_or_init(|| async {
            let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
            let pool = db::create_pool(&url).await.expect("Failed to connect");
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
    let limit = req.limit.unwrap_or(3);
    state.store.set_tenant(&req.tenant_id).await?;

    let embedding = state.embedder.embed(&req.query).await?;

    let (results_a, results_b) = tokio::try_join!(
        state.store.search_within_document(&embedding, req.document_id_a, &req.tenant_id, limit),
        state.store.search_within_document(&embedding, req.document_id_b, &req.tenant_id, limit),
    )?;

    let to_doc_result = |results: Vec<docint_core::models::SearchResult>| -> DocResult {
        let title = results.first().map(|r| r.title.clone()).unwrap_or_default();
        let doc_id = results.first().map(|r| r.document_id.to_string()).unwrap_or_default();
        DocResult {
            document_id: doc_id,
            title,
            matches: results
                .into_iter()
                .map(|r| ChunkHit {
                    chunk_id: r.chunk_id.to_string(),
                    content: r.content,
                    distance: r.distance,
                })
                .collect(),
        }
    };

    Ok(Response {
        query: req.query,
        document_a: to_doc_result(results_a),
        document_b: to_doc_result(results_b),
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
