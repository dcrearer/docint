//! Integration tests for lambda-search handler.
//!
//! Tests verify the search logic using actual database operations.
//! Since handler() requires Lambda runtime and Bedrock credentials,
//! we test the underlying store methods directly.
//!
//! Run with: cargo test --test handler_tests -- --ignored

mod common;

use common::{seed_test_data, setup_test_db};
use docint_core::{db, store::VectorStore};

#[tokio::test]
#[ignore]
async fn test_handler_returns_search_results() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-search-1";

    // Seed test data
    seed_test_data(&pool, tenant_id, "Search Test").await.unwrap();

    // Perform hybrid search (simulating what handler does)
    let query = "Search Test";
    let embedding = vec![0.1; 1024]; // Same as seeded data
    let limit = 5;

    let results = db::with_tenant(&pool, tenant_id, move |tx| {
        let tenant_id = tenant_id.to_string();
        Box::pin(async move {
            VectorStore::hybrid_search_tx(tx, &embedding, query, &tenant_id, limit).await
        })
    })
    .await
    .unwrap();

    // Should return results
    assert!(!results.is_empty(), "Expected search results");
    assert!(results.len() <= 5, "Results should respect limit");

    // Verify result structure
    let first = &results[0];
    assert!(first.content.contains("Search Test"));
    assert!(first.title.contains("Search Test"));
}

#[tokio::test]
#[ignore]
async fn test_handler_respects_limit() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-limit-test";

    // Seed test data (creates 4 chunks)
    seed_test_data(&pool, tenant_id, "Limit Test").await.unwrap();

    let query = "Limit Test";
    let embedding = vec![0.1; 1024];

    // Test with limit of 2
    let embedding_clone = embedding.clone();
    let results = db::with_tenant(&pool, tenant_id, move |tx| {
        let tenant_id = tenant_id.to_string();
        Box::pin(async move {
            VectorStore::hybrid_search_tx(tx, &embedding_clone, query, &tenant_id, 2).await
        })
    })
    .await
    .unwrap();

    assert_eq!(results.len(), 2, "Should return exactly 2 results");

    // Test with limit of 10 (should return all 4)
    let results = db::with_tenant(&pool, tenant_id, move |tx| {
        let tenant_id = tenant_id.to_string();
        Box::pin(async move {
            VectorStore::hybrid_search_tx(tx, &embedding, query, &tenant_id, 10).await
        })
    })
    .await
    .unwrap();

    assert_eq!(results.len(), 4, "Should return all 4 chunks");
}

#[tokio::test]
#[ignore]
async fn test_handler_requires_tenant_id() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-isolation-1";
    let wrong_tenant = "tenant-isolation-2";

    // Seed data for tenant 1
    seed_test_data(&pool, tenant_id, "Tenant 1 Data").await.unwrap();

    let query = "Tenant 1 Data";
    let embedding = vec![0.1; 1024];
    let limit = 5;

    // Search with correct tenant_id - should find results
    let embedding_clone = embedding.clone();
    let results = db::with_tenant(&pool, tenant_id, move |tx| {
        let tenant_id = tenant_id.to_string();
        Box::pin(async move {
            VectorStore::hybrid_search_tx(tx, &embedding_clone, query, &tenant_id, limit).await
        })
    })
    .await
    .unwrap();

    assert!(!results.is_empty(), "Should find results for correct tenant");

    // Search with wrong tenant_id - should find nothing (RLS isolation)
    let results = db::with_tenant(&pool, wrong_tenant, move |tx| {
        let tenant_id = wrong_tenant.to_string();
        Box::pin(async move {
            VectorStore::hybrid_search_tx(tx, &embedding, query, &tenant_id, limit).await
        })
    })
    .await
    .unwrap();

    assert!(results.is_empty(), "Should not find results for wrong tenant");
}

#[tokio::test]
#[ignore]
async fn test_handler_empty_query_returns_results() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-empty-query";

    // Seed test data
    seed_test_data(&pool, tenant_id, "Empty Query Test").await.unwrap();

    // Search with empty query string (vector search still works)
    let query = "";
    let embedding = vec![0.1; 1024];
    let limit = 5;

    let results = db::with_tenant(&pool, tenant_id, move |tx| {
        let tenant_id = tenant_id.to_string();
        Box::pin(async move {
            VectorStore::hybrid_search_tx(tx, &embedding, query, &tenant_id, limit).await
        })
    })
    .await
    .unwrap();

    // Empty query should still return results via vector search
    // (full-text search returns nothing, but vector search provides results)
    assert!(!results.is_empty(), "Vector search should work with empty query");
}
