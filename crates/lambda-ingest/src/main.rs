//! Lambda: document ingestion pipeline.
//! Fetches a text file from S3, splits it into chunks, generates
//! embeddings for each chunk via Bedrock Titan, and stores everything
//! in PostgreSQL + pgvector.

use aws_sdk_s3::Client as S3Client;
use docint_core::{chunker::Chunker, db, embeddings::Embedder, store::VectorStore};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Deserialize)]
struct Request {
    bucket: String,
    key: String,
    tenant_id: String,
    title: Option<String>,
}

#[derive(Serialize)]
struct Response {
    document_id: String,
    chunks_created: usize,
}

struct AppState {
    store: VectorStore,
    embedder: Embedder,
    s3: S3Client,
    chunker: Chunker,
}

static STATE: OnceCell<AppState> = OnceCell::const_new();

async fn get_state() -> &'static AppState {
    STATE
        .get_or_init(|| async {
            let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
            let pool = db::create_pool(&url).await.expect("Failed to connect");
            let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            AppState {
                store: VectorStore::new(pool),
                embedder: Embedder::new().await,
                s3: S3Client::new(&config),
                chunker: Chunker::default(),
            }
        })
        .await
}

async fn handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let req = event.payload;
    let state = get_state().await;
    state.store.set_tenant(&req.tenant_id).await?;

    // Fetch text from S3
    let obj = state
        .s3
        .get_object()
        .bucket(&req.bucket)
        .key(&req.key)
        .send()
        .await?;
    let bytes = obj.body.collect().await?.into_bytes();
    let text = String::from_utf8_lossy(&bytes);

    // Create document
    let title = req.title.unwrap_or_else(|| req.key.clone());
    let doc = state
        .store
        .insert_document(&req.tenant_id, &title, &req.key)
        .await?;

    // Chunk → embed → store
    let chunks = state.chunker.chunk(&text);
    for (i, chunk) in chunks.iter().enumerate() {
        let emb = state.embedder.embed(chunk).await?;
        state
            .store
            .insert_chunk(doc.id, chunk, i as i32, &emb)
            .await?;
    }

    tracing::info!(
        document_id = %doc.id,
        chunks = chunks.len(),
        key = %req.key,
        "Ingested document"
    );

    Ok(Response {
        document_id: doc.id.to_string(),
        chunks_created: chunks.len(),
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
