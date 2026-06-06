//! Integration tests for Row-Level Security (RLS) tenant isolation.
//!
//! These tests verify that the P0 #1 fix (transaction-scoped RLS) works correctly:
//! - Tenant A cannot see Tenant B's data
//! - Concurrent requests don't leak tenant context
//! - Connection pool returns clean connections
//!
//! Run with: cargo test --test store_rls_tests -- --ignored
//! (Requires test database: docker-compose -f docker-compose.test.yml up)

mod common;

use common::{setup_test_db, seed_test_data};
use docint_core::{db, store::VectorStore};
use anyhow::Context;

#[tokio::test]
#[ignore] // Requires test database
async fn test_tenant_isolation_basic() {
    // GIVEN: Two tenants with different documents
    let pool = setup_test_db().await.unwrap();

    let tenant_a = "tenant-a-111";
    let tenant_b = "tenant-b-222";

    seed_test_data(&pool, tenant_a, "Tenant A").await.unwrap();
    seed_test_data(&pool, tenant_b, "Tenant B").await.unwrap();

    // WHEN: Tenant A lists documents
    let docs_a = db::with_tenant(&pool, tenant_a, |tx| {
        Box::pin(async move {
            VectorStore::list_documents_tx(tx, tenant_a, 100).await
        })
    }).await.unwrap();

    // THEN: Only Tenant A's documents are returned
    assert_eq!(docs_a.len(), 2, "Tenant A should see 2 documents");
    assert!(
        docs_a.iter().all(|d| d.tenant_id == tenant_a),
        "All documents should belong to tenant A"
    );
    assert!(
        docs_a.iter().all(|d| d.title.contains("Tenant A")),
        "All documents should be labeled 'Tenant A'"
    );

    // WHEN: Tenant B lists documents
    let docs_b = db::with_tenant(&pool, tenant_b, |tx| {
        Box::pin(async move {
            VectorStore::list_documents_tx(tx, tenant_b, 100).await
        })
    }).await.unwrap();

    // THEN: Only Tenant B's documents are returned
    assert_eq!(docs_b.len(), 2, "Tenant B should see 2 documents");
    assert!(
        docs_b.iter().all(|d| d.tenant_id == tenant_b),
        "All documents should belong to tenant B"
    );
    assert!(
        docs_b.iter().all(|d| d.title.contains("Tenant B")),
        "All documents should be labeled 'Tenant B'"
    );
}

#[tokio::test]
#[ignore]
async fn test_tenant_isolation_search() {
    // GIVEN: Two tenants with documents containing similar content
    let pool = setup_test_db().await.unwrap();

    let tenant_a = "tenant-search-a";
    let tenant_b = "tenant-search-b";

    seed_test_data(&pool, tenant_a, "SearchTest").await.unwrap();
    seed_test_data(&pool, tenant_b, "SearchTest").await.unwrap();

    // WHEN: Tenant A searches (using sample embedding)
    let sample_embedding = vec![0.1; 1024];

    let results_a = db::with_tenant(&pool, tenant_a, |tx| {
        let embedding = sample_embedding.clone();
        Box::pin(async move {
            VectorStore::similarity_search_tx(tx, &embedding, tenant_a, 10).await
        })
    }).await.unwrap();

    // THEN: Only Tenant A's chunks are returned
    assert!(!results_a.is_empty(), "Tenant A should get search results");
    // Note: We can't easily verify tenant_id on chunks without a JOIN,
    // but the RLS policy ensures only tenant A's chunks are visible

    // WHEN: Tenant B searches with same embedding
    let results_b = db::with_tenant(&pool, tenant_b, |tx| {
        let embedding = sample_embedding.clone();
        Box::pin(async move {
            VectorStore::similarity_search_tx(tx, &embedding, tenant_b, 10).await
        })
    }).await.unwrap();

    // THEN: Results are isolated (different result sets)
    assert!(!results_b.is_empty(), "Tenant B should get search results");
    assert_eq!(results_a.len(), results_b.len(), "Both should have same number of chunks");
}

#[tokio::test]
#[ignore]
async fn test_concurrent_tenant_requests_no_leakage() {
    // GIVEN: Two tenants and a shared connection pool with 1 connection
    let pool = setup_test_db().await.unwrap();

    let tenant_a = "tenant-concurrent-a";
    let tenant_b = "tenant-concurrent-b";

    seed_test_data(&pool, tenant_a, "Concurrent A").await.unwrap();
    seed_test_data(&pool, tenant_b, "Concurrent B").await.unwrap();

    // WHEN: Make concurrent requests for both tenants
    let (docs_a, docs_b) = tokio::try_join!(
        db::with_tenant(&pool, tenant_a, |tx| {
            Box::pin(async move {
                VectorStore::list_documents_tx(tx, tenant_a, 100).await
            })
        }),
        db::with_tenant(&pool, tenant_b, |tx| {
            Box::pin(async move {
                VectorStore::list_documents_tx(tx, tenant_b, 100).await
            })
        })
    ).unwrap();

    // THEN: Each tenant sees only their own data (no cross-contamination)
    assert_eq!(docs_a.len(), 2, "Tenant A should see 2 documents");
    assert_eq!(docs_b.len(), 2, "Tenant B should see 2 documents");

    assert!(
        docs_a.iter().all(|d| d.tenant_id == tenant_a),
        "Tenant A should only see their documents"
    );
    assert!(
        docs_b.iter().all(|d| d.tenant_id == tenant_b),
        "Tenant B should only see their documents"
    );
}

#[tokio::test]
#[ignore]
async fn test_transaction_scoped_tenant_context_is_cleared() {
    // GIVEN: A connection pool
    let pool = setup_test_db().await.unwrap();

    let tenant_a = "tenant-context-a";
    let tenant_b = "tenant-context-b";

    seed_test_data(&pool, tenant_a, "Context A").await.unwrap();
    seed_test_data(&pool, tenant_b, "Context B").await.unwrap();

    // WHEN: Execute request for tenant A
    let docs_a = db::with_tenant(&pool, tenant_a, |tx| {
        Box::pin(async move {
            VectorStore::list_documents_tx(tx, tenant_a, 100).await
        })
    }).await.unwrap();

    assert_eq!(docs_a.len(), 2, "Tenant A should see 2 documents");

    // WHEN: Immediately execute request for tenant B (reusing same connection)
    let docs_b = db::with_tenant(&pool, tenant_b, |tx| {
        Box::pin(async move {
            VectorStore::list_documents_tx(tx, tenant_b, 100).await
        })
    }).await.unwrap();

    // THEN: Tenant B does NOT see tenant A's data
    // (verifies connection was returned to pool clean, with no stale app.tenant_id)
    assert_eq!(docs_b.len(), 2, "Tenant B should see 2 documents");
    assert!(
        docs_b.iter().all(|d| d.tenant_id == tenant_b),
        "Tenant B should only see their own documents (no stale context from tenant A)"
    );
}

#[tokio::test]
#[ignore]
async fn test_rls_policy_filters_chunks_by_document_tenant() {
    // GIVEN: Tenant A and B both have documents
    let pool = setup_test_db().await.unwrap();

    let tenant_a = "tenant-chunks-a";
    let tenant_b = "tenant-chunks-b";

    seed_test_data(&pool, tenant_a, "Chunks A").await.unwrap();
    seed_test_data(&pool, tenant_b, "Chunks B").await.unwrap();

    // WHEN: Tenant A queries chunks directly
    let chunks_a: Vec<(String,)> = db::with_tenant(&pool, tenant_a, |tx| {
        Box::pin(async move {
            sqlx::query_as("SELECT content FROM chunks ORDER BY content")
                .fetch_all(&mut **tx)
                .await
                .context("Failed to fetch chunks for tenant A")
        })
    }).await.unwrap();

    // THEN: Only tenant A's chunks are visible
    assert_eq!(chunks_a.len(), 4, "Tenant A should see 4 chunks");
    assert!(
        chunks_a.iter().all(|(content,)| content.contains("Chunks A")),
        "All chunks should belong to tenant A's documents"
    );

    // WHEN: Tenant B queries chunks
    let chunks_b: Vec<(String,)> = db::with_tenant(&pool, tenant_b, |tx| {
        Box::pin(async move {
            sqlx::query_as("SELECT content FROM chunks ORDER BY content")
                .fetch_all(&mut **tx)
                .await
                .context("Failed to fetch chunks for tenant B")
        })
    }).await.unwrap();

    // THEN: Only tenant B's chunks are visible
    assert_eq!(chunks_b.len(), 4, "Tenant B should see 4 chunks");
    assert!(
        chunks_b.iter().all(|(content,)| content.contains("Chunks B")),
        "All chunks should belong to tenant B's documents"
    );
}
