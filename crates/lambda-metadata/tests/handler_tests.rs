//! Integration tests for lambda-metadata handler.
//!
//! These tests verify:
//! - Valid document_id returns metadata
//! - Invalid UUID returns empty response
//! - Response includes correct chunk count
//! - Tenant isolation is enforced
//!
//! Run with: cargo test -p lambda-metadata --test handler_tests -- --ignored
//! (Requires test database: docker-compose -f docker-compose.test.yml up)

// Import common test utilities from docint-core
#[path = "../../docint-core/tests/common/mod.rs"]
mod common;

use common::{setup_test_db, seed_test_data};
use docint_core::{db, store::VectorStore};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-declare the types from main.rs for testing
#[derive(Serialize)]
struct Request {
    tenant_id: String,
    document_id: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct DocInfo {
    id: String,
    title: String,
    source_key: String,
    created_at: String,
    metadata: serde_json::Value,
    chunk_count: i32,
}

#[derive(Deserialize, Debug)]
struct Response {
    documents: Vec<DocInfo>,
}

/// Helper to create a test handler that uses a specific pool
async fn call_handler(
    pool: &sqlx::PgPool,
    request: Request,
) -> Result<Response, anyhow::Error> {
    let store = VectorStore::new(pool.clone());
    let tenant_id = request.tenant_id.clone();
    let document_id = request.document_id;
    let limit = request.limit.unwrap_or(20);

    let documents = db::with_tenant(&store.pool(), &request.tenant_id, move |tx| {
        let tenant_id = tenant_id.clone();
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

#[tokio::test]
#[ignore] // Requires test database
async fn test_handler_returns_document_metadata() {
    // GIVEN: A document with chunks exists for a tenant
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-handler-metadata";

    let doc_id = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            let doc = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Test Document",
                "s3://bucket/test-doc.pdf",
            )
            .await?;

            // Insert 3 chunks
            for i in 0..3 {
                VectorStore::insert_chunk_tx(
                    tx,
                    doc.id,
                    &format!("Test chunk {}", i),
                    i,
                    &vec![0.1; 1024],
                )
                .await?;
            }

            Ok::<_, anyhow::Error>(doc.id)
        })
    })
    .await
    .unwrap();

    // WHEN: Request metadata for this specific document
    let request = Request {
        tenant_id: tenant_id.to_string(),
        document_id: Some(doc_id),
        limit: None,
    };

    let response = call_handler(&pool, request).await.unwrap();

    // THEN: Response contains the document with correct metadata
    assert_eq!(response.documents.len(), 1);
    let doc_info = &response.documents[0];
    assert_eq!(doc_info.id, doc_id.to_string());
    assert_eq!(doc_info.title, "Test Document");
    assert_eq!(doc_info.source_key, "s3://bucket/test-doc.pdf");
    assert_eq!(doc_info.chunk_count, 3);
}

#[tokio::test]
#[ignore] // Requires test database
async fn test_handler_returns_none_for_nonexistent_document() {
    // GIVEN: A test database with a tenant
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-handler-nonexistent";

    // Seed some data to verify we're not accidentally returning everything
    seed_test_data(&pool, tenant_id, "Test").await.unwrap();

    // WHEN: Request metadata for a non-existent document
    let nonexistent_id = Uuid::new_v4();
    let request = Request {
        tenant_id: tenant_id.to_string(),
        document_id: Some(nonexistent_id),
        limit: None,
    };

    let response = call_handler(&pool, request).await.unwrap();

    // THEN: Response contains empty documents array
    assert_eq!(
        response.documents.len(),
        0,
        "Should return empty array for non-existent document"
    );
}

#[tokio::test]
#[ignore] // Requires test database
async fn test_handler_includes_chunk_count() {
    // GIVEN: Multiple documents with different chunk counts
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-handler-chunk-count";

    db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            // Document 1: 2 chunks
            let doc1 = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Doc with 2 chunks",
                "s3://bucket/doc1.pdf",
            )
            .await?;

            for i in 0..2 {
                VectorStore::insert_chunk_tx(
                    tx,
                    doc1.id,
                    &format!("Chunk {}", i),
                    i,
                    &vec![0.1; 1024],
                )
                .await?;
            }

            // Document 2: 5 chunks
            let doc2 = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Doc with 5 chunks",
                "s3://bucket/doc2.pdf",
            )
            .await?;

            for i in 0..5 {
                VectorStore::insert_chunk_tx(
                    tx,
                    doc2.id,
                    &format!("Chunk {}", i),
                    i,
                    &vec![0.2; 1024],
                )
                .await?;
            }

            // Document 3: 0 chunks
            VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Doc with 0 chunks",
                "s3://bucket/doc3.pdf",
            )
            .await?;

            Ok::<_, anyhow::Error>(())
        })
    })
    .await
    .unwrap();

    // WHEN: Request all documents for the tenant
    let request = Request {
        tenant_id: tenant_id.to_string(),
        document_id: None,
        limit: Some(100),
    };

    let response = call_handler(&pool, request).await.unwrap();

    // THEN: Response includes correct chunk counts for each document
    assert_eq!(response.documents.len(), 3);

    // Find each document by title and verify chunk count
    let doc_with_2 = response
        .documents
        .iter()
        .find(|d| d.title == "Doc with 2 chunks")
        .expect("Should find doc with 2 chunks");
    assert_eq!(doc_with_2.chunk_count, 2);

    let doc_with_5 = response
        .documents
        .iter()
        .find(|d| d.title == "Doc with 5 chunks")
        .expect("Should find doc with 5 chunks");
    assert_eq!(doc_with_5.chunk_count, 5);

    let doc_with_0 = response
        .documents
        .iter()
        .find(|d| d.title == "Doc with 0 chunks")
        .expect("Should find doc with 0 chunks");
    assert_eq!(doc_with_0.chunk_count, 0);
}

#[tokio::test]
#[ignore] // Requires test database
async fn test_handler_enforces_tenant_isolation() {
    // GIVEN: Two tenants with different documents
    let pool = setup_test_db().await.unwrap();
    let tenant_a = "tenant-handler-isolation-a";
    let tenant_b = "tenant-handler-isolation-b";

    seed_test_data(&pool, tenant_a, "Tenant A").await.unwrap();
    seed_test_data(&pool, tenant_b, "Tenant B").await.unwrap();

    // Get a document ID from tenant A
    let tenant_a_doc_id = db::with_tenant(&pool, tenant_a, |tx| {
        Box::pin(async move {
            let docs = VectorStore::list_documents_tx(tx, tenant_a, 1).await?;
            Ok::<_, anyhow::Error>(docs[0].id)
        })
    })
    .await
    .unwrap();

    // WHEN: Tenant B tries to access Tenant A's document
    let request = Request {
        tenant_id: tenant_b.to_string(),
        document_id: Some(tenant_a_doc_id),
        limit: None,
    };

    let response = call_handler(&pool, request).await.unwrap();

    // THEN: Response is empty (tenant B cannot see tenant A's document)
    assert_eq!(
        response.documents.len(),
        0,
        "Tenant B should not be able to access Tenant A's document"
    );

    // Verify Tenant A CAN access their own document
    let request_a = Request {
        tenant_id: tenant_a.to_string(),
        document_id: Some(tenant_a_doc_id),
        limit: None,
    };

    let response_a = call_handler(&pool, request_a).await.unwrap();
    assert_eq!(
        response_a.documents.len(),
        1,
        "Tenant A should be able to access their own document"
    );
    assert_eq!(response_a.documents[0].id, tenant_a_doc_id.to_string());
}
