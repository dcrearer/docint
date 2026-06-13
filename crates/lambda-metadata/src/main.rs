//! Lambda: document metadata.
//! Lists all documents for a tenant, or returns details for a specific document.
//! Includes chunk count for each document.

use docint_core::{lambda_init, store::VectorStore};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use uuid::Uuid;

#[derive(Deserialize)]
struct Request {
    #[serde(default = "default_tenant_id")]
    tenant_id: String,
    document_id: Option<Uuid>,
    limit: Option<i64>,
}

fn default_tenant_id() -> String {
    // HOTFIX: Allow tools to work without tenant_id parameter
    // RLS at database level still enforces tenant isolation
    std::env::var("DEFAULT_TENANT_ID").unwrap_or_else(|_| "default-tenant".to_string())
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
            lambda_init::init_store()
                .await
                .expect("Failed to initialize store")
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
    use docint_core::db;

    let req = event.payload;
    let store = get_store().await;
    let tenant_id = req.tenant_id.clone();
    let document_id = req.document_id;
    let limit = req.limit.unwrap_or(20).clamp(1, 100);

    let documents = db::with_tenant(store.pool(), &req.tenant_id, move |tx| {
        Box::pin(async move {
            if let Some(doc_id) = document_id {
                match VectorStore::get_metadata_tx(tx, doc_id, &tenant_id).await? {
                    Some(m) => Ok(vec![to_doc_info(m)]),
                    None => Ok(vec![]),
                }
            } else {
                let docs = VectorStore::list_documents_tx(tx, &tenant_id, limit).await?;
                Ok(docs.into_iter().map(to_doc_info).collect())
            }
        })
    })
    .await?;

    Ok(Response { documents })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda_init::setup_tracing();
    lambda_runtime::run(service_fn(handler)).await
}
