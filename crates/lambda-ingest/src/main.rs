//! Lambda: document ingestion pipeline.
//! Triggered by S3 events or direct invocation.
//! Fetches a text file from S3, splits it into chunks, generates
//! embeddings for each chunk via Bedrock Titan, and stores everything
//! in PostgreSQL + pgvector.

use aws_sdk_s3::Client as S3Client;
use docint_core::{chunker::Chunker, db, embeddings::Embedder, store::VectorStore};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

/// Direct invocation payload.
#[derive(Deserialize)]
struct DirectRequest {
    bucket: String,
    key: String,
    tenant_id: String,
    title: Option<String>,
}

/// S3 event notification payload.
#[derive(Deserialize)]
struct S3Event {
    #[serde(rename = "Records")]
    records: Vec<S3Record>,
}

#[derive(Deserialize)]
struct S3Record {
    s3: S3Info,
}

#[derive(Deserialize)]
struct S3Info {
    bucket: S3Bucket,
    object: S3Object,
}

#[derive(Deserialize)]
struct S3Bucket {
    name: String,
}

#[derive(Deserialize)]
struct S3Object {
    key: String,
}

/// Unified input: try S3 event first, fall back to direct invocation.
#[derive(Deserialize)]
#[serde(untagged)]
enum IngestEvent {
    S3(S3Event),
    Direct(DirectRequest),
}

#[derive(Serialize)]
struct Response {
    documents: Vec<IngestResult>,
}

#[derive(Serialize)]
struct IngestResult {
    document_id: String,
    chunks_created: usize,
    key: String,
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

/// Ingest a single file from S3.
async fn ingest_file(
    state: &AppState,
    bucket: &str,
    key: &str,
    tenant_id: &str,
    title: &str,
) -> Result<IngestResult, Error> {
    state.store.set_tenant(tenant_id).await?;

    let obj = state.s3.get_object().bucket(bucket).key(key).send().await?;
    let bytes = obj.body.collect().await?.into_bytes();
    let text = String::from_utf8_lossy(&bytes);

    let doc = state.store.insert_document(tenant_id, title, key).await?;

    let chunks = state.chunker.chunk(&text);
    for (i, chunk) in chunks.iter().enumerate() {
        let emb = state.embedder.embed(chunk).await?;
        state.store.insert_chunk(doc.id, chunk, i as i32, &emb).await?;
    }

    tracing::info!(document_id = %doc.id, chunks = chunks.len(), key, "Ingested document");

    Ok(IngestResult {
        document_id: doc.id.to_string(),
        chunks_created: chunks.len(),
        key: key.to_string(),
    })
}

/// Derive a title from the S3 key (filename without extension).
fn title_from_key(key: &str) -> String {
    std::path::Path::new(key)
        .file_stem()
        .map(|s| s.to_string_lossy().replace('-', " "))
        .unwrap_or_else(|| key.to_string())
}

/// Derive tenant_id from S3 key prefix: "tenant-2/docs/file.md" → "tenant-2".
/// Falls back to default if key has no prefix or prefix is empty.
fn tenant_from_key<'a>(key: &'a str, default: &'a str) -> &'a str {
    key.split('/')
        .next()
        .filter(|s| !s.is_empty() && !s.contains('.'))
        .unwrap_or(default)
}

async fn handler(event: LambdaEvent<IngestEvent>) -> Result<Response, Error> {
    let state = get_state().await;
    let default_tenant = std::env::var("DEFAULT_TENANT_ID").unwrap_or_else(|_| "tenant-1".into());

    let results = match event.payload {
        IngestEvent::Direct(req) => {
            let title = req.title.unwrap_or_else(|| title_from_key(&req.key));
            let r = ingest_file(state, &req.bucket, &req.key, &req.tenant_id, &title).await?;
            vec![r]
        }
        IngestEvent::S3(s3_event) => {
            let mut results = Vec::new();
            for record in s3_event.records {
                // URL-decode the key (S3 encodes spaces as +)
                let key = record.s3.object.key.replace('+', " ");
                let key = urlencoding::decode(&key).map(|s| s.into_owned()).unwrap_or(key);
                let tenant_id = tenant_from_key(&key, &default_tenant);
                let title = title_from_key(&key);
                match ingest_file(state, &record.s3.bucket.name, &key, tenant_id, &title).await {
                    Ok(r) => results.push(r),
                    Err(e) => tracing::error!(key = %key, error = %e, "Failed to ingest"),
                }
            }
            results
        }
    };

    Ok(Response { documents: results })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
