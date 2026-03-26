//! Lambda: document metadata.
//! Lists all documents for a tenant, or returns details for a specific document.
//! Includes chunk count for each document.

use docint_core::{db, store::VectorStore};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use uuid::Uuid;

#[derive(Deserialize)]
struct Request {
    tenant_id: String,
    document_id: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct DocInfo {
    id: String,
    title: String,
    source_key: String,
    created_at: String,
    metadata: serde_json::Value,
    chunk_count: i32,
}

#[derive(Serialize)]
struct Response {
    documents: Vec<DocInfo>,
}

static STORE: OnceCell<VectorStore> = OnceCell::const_new();

async fn get_store() -> &'static VectorStore {
    STORE
        .get_or_init(|| async {
            let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
            let pool = db::create_pool(&url).await.expect("Failed to connect");
            VectorStore::new(pool)
        })
        .await
}

fn to_doc_info(m: docint_core::models::DocumentMetadata) -> DocInfo {
    DocInfo {
        id: m.id.to_string(),
        title: m.title,
        source_key: m.source_key,
        created_at: m.created_at.to_rfc3339(),
        metadata: m.metadata,
        chunk_count: m.chunk_count,
    }
}

async fn handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let req = event.payload;
    let store = get_store().await;
    store.set_tenant(&req.tenant_id).await?;

    let documents = if let Some(doc_id) = req.document_id {
        match store.get_metadata(doc_id, &req.tenant_id).await? {
            Some(m) => vec![to_doc_info(m)],
            None => vec![],
        }
    } else {
        store
            .list_documents(&req.tenant_id, req.limit.unwrap_or(20))
            .await?
            .into_iter()
            .map(to_doc_info)
            .collect()
    };

    Ok(Response { documents })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
