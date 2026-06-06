//! Business logic tests for VectorStore methods.
//!
//! Tests verify:
//! - Insert/upsert document logic
//! - Chunk insertion with embeddings
//! - Vector similarity search scoring
//! - Hybrid search RRF algorithm
//! - Metadata retrieval
//! - Document listing
//! - Document-scoped search
//!
//! Run with: cargo test --test store_business_logic_tests -- --ignored

mod common;

use common::{setup_test_db, seed_test_data};
use docint_core::{db, store::VectorStore};
use anyhow::Context;

#[tokio::test]
#[ignore]
async fn test_insert_document_creates_new_document() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-insert-1";

    let doc = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Test Document",
                "s3://bucket/test.pdf",
            )
            .await
        })
    })
    .await
    .unwrap();

    assert_eq!(doc.title, "Test Document");
    assert_eq!(doc.source_key, "s3://bucket/test.pdf");
    assert_eq!(doc.tenant_id, tenant_id);
}

#[tokio::test]
#[ignore]
async fn test_insert_document_upserts_on_conflict() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-upsert-1";

    // Insert document first time
    let doc1 = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Original Title",
                "s3://bucket/doc.pdf",
            )
            .await
        })
    })
    .await
    .unwrap();

    // Insert same source_key again (should upsert)
    let doc2 = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Updated Title",
                "s3://bucket/doc.pdf",
            )
            .await
        })
    })
    .await
    .unwrap();

    // Should have same ID (upsert, not new insert)
    assert_eq!(doc1.id, doc2.id);
    // Title should be updated
    assert_eq!(doc2.title, "Updated Title");
}

#[tokio::test]
#[ignore]
async fn test_insert_document_deletes_old_chunks_on_upsert() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-upsert-chunks";

    // Insert document with chunks
    let (_doc_id, chunk1_id) = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            let doc = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Document v1",
                "s3://bucket/doc.pdf",
            )
            .await?;

            let chunk = VectorStore::insert_chunk_tx(
                tx,
                doc.id,
                "Old content",
                0,
                &vec![0.1; 1024],
            )
            .await?;

            Ok::<_, anyhow::Error>((doc.id, chunk.id))
        })
    })
    .await
    .unwrap();

    // Upsert same document (should delete old chunks)
    let chunk_count = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Document v2",
                "s3://bucket/doc.pdf",
            )
            .await?;

            // Check if old chunk was deleted
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM chunks WHERE id = $1"
            )
            .bind(chunk1_id)
            .fetch_one(&mut **tx)
            .await
            .context("Failed to count chunks")?;

            Ok::<_, anyhow::Error>(count)
        })
    })
    .await
    .unwrap();

    assert_eq!(chunk_count, 0, "Old chunks should be deleted on upsert");
}

#[tokio::test]
#[ignore]
async fn test_insert_chunk_stores_embedding() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-chunk-1";

    let embedding = vec![0.5; 1024];

    let chunk = db::with_tenant(&pool, tenant_id, |tx| {
        let embedding = embedding.clone();
        Box::pin(async move {
            let doc = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Test Doc",
                "s3://bucket/test.pdf",
            )
            .await?;

            VectorStore::insert_chunk_tx(
                tx,
                doc.id,
                "Test content",
                0,
                &embedding,
            )
            .await
        })
    })
    .await
    .unwrap();

    assert_eq!(chunk.content, "Test content");
    assert_eq!(chunk.chunk_index, 0);
}

#[tokio::test]
#[ignore]
async fn test_similarity_search_returns_results_ordered_by_distance() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-similarity-1";

    // Seed test data (2 docs, 4 chunks total)
    seed_test_data(&pool, tenant_id, "Test").await.unwrap();

    // Search with arbitrary embedding
    let query_embedding = vec![0.5; 1024];

    let results = db::with_tenant(&pool, tenant_id, |tx| {
        let embedding = query_embedding.clone();
        Box::pin(async move {
            VectorStore::similarity_search_tx(tx, &embedding, tenant_id, 10).await
        })
    })
    .await
    .unwrap();

    // Should return all 4 chunks
    assert_eq!(results.len(), 4);

    // Results should be ordered by distance (ascending)
    for i in 0..(results.len() - 1) {
        assert!(
            results[i].distance <= results[i + 1].distance,
            "Results should be ordered by distance: {} <= {}",
            results[i].distance,
            results[i + 1].distance
        );
    }

    // All results should have a distance value
    for result in &results {
        assert!(result.distance >= 0.0, "Distance should be non-negative");
    }
}

#[tokio::test]
#[ignore]
async fn test_similarity_search_respects_limit() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-limit-1";

    // Seed 4 chunks
    seed_test_data(&pool, tenant_id, "Test").await.unwrap();

    let results = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::similarity_search_tx(tx, &vec![0.1; 1024], tenant_id, 2).await
        })
    })
    .await
    .unwrap();

    assert_eq!(results.len(), 2, "Should respect limit parameter");
}

#[tokio::test]
#[ignore]
async fn test_hybrid_search_combines_vector_and_fts() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-hybrid-1";

    // Create chunks with different characteristics
    db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            let doc = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Test Doc",
                "s3://bucket/test.pdf",
            )
            .await?;

            // Matches vector search only (no FTS keywords)
            VectorStore::insert_chunk_tx(
                tx,
                doc.id,
                "This content has no keyword match",
                0,
                &vec![1.0; 1024],
            )
            .await?;

            // Matches FTS only (contains "database query")
            VectorStore::insert_chunk_tx(
                tx,
                doc.id,
                "The database query optimization technique",
                1,
                &vec![0.0; 1024],
            )
            .await?;

            // Matches both vector + FTS (same keywords, similar embedding)
            VectorStore::insert_chunk_tx(
                tx,
                doc.id,
                "Database query performance",
                2,
                &vec![0.9; 1024],
            )
            .await?;

            Ok::<_, anyhow::Error>(())
        })
    })
    .await
    .unwrap();

    // Search for "database query" with query embedding similar to vec![1.0; 1024]
    let results = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::hybrid_search_tx(
                tx,
                &vec![1.0; 1024],
                "database query",
                tenant_id,
                10,
            )
            .await
        })
    })
    .await
    .unwrap();

    // Should return all 3 results (2 match FTS, 1 matches vector only)
    assert!(results.len() >= 2, "Should have at least 2 FTS matches");

    // Verify that chunks matching "database query" are in results
    let has_fts_matches = results.iter().any(|r| r.content.contains("database"));
    assert!(has_fts_matches, "Should include FTS keyword matches");

    // The chunk matching BOTH criteria should rank highly (top 2)
    let hybrid_match_rank = results.iter().position(|r|
        r.content.contains("Database query performance")
    );
    assert!(
        hybrid_match_rank.is_some() && hybrid_match_rank.unwrap() < 2,
        "Hybrid match (vector + FTS) should rank in top 2 via RRF"
    );
}

#[tokio::test]
#[ignore]
async fn test_hybrid_search_handles_no_fts_matches() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-hybrid-nofts";

    seed_test_data(&pool, tenant_id, "Test").await.unwrap();

    // Search with query that won't match FTS
    let results = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::hybrid_search_tx(
                tx,
                &vec![0.1; 1024],
                "xyznonexistentquery",
                tenant_id,
                10,
            )
            .await
        })
    })
    .await
    .unwrap();

    // Should still return vector results even with no FTS matches
    assert!(!results.is_empty(), "Should fall back to vector-only results");
}

#[tokio::test]
#[ignore]
async fn test_get_metadata_returns_chunk_count() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-meta-1";

    let (doc_id, expected_count) = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            let doc = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Test Doc",
                "s3://bucket/test.pdf",
            )
            .await?;

            // Insert 3 chunks
            for i in 0..3 {
                VectorStore::insert_chunk_tx(
                    tx,
                    doc.id,
                    &format!("Chunk {}", i),
                    i,
                    &vec![0.1; 1024],
                )
                .await?;
            }

            Ok::<_, anyhow::Error>((doc.id, 3))
        })
    })
    .await
    .unwrap();

    let meta = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::get_metadata_tx(tx, doc_id, tenant_id).await
        })
    })
    .await
    .unwrap()
    .expect("Metadata should exist");

    assert_eq!(meta.chunk_count, expected_count);
    assert_eq!(meta.title, "Test Doc");
}

#[tokio::test]
#[ignore]
async fn test_get_metadata_returns_none_for_nonexistent_document() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-meta-none";

    let meta = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::get_metadata_tx(
                tx,
                uuid::Uuid::new_v4(),
                tenant_id,
            )
            .await
        })
    })
    .await
    .unwrap();

    assert!(meta.is_none(), "Should return None for non-existent document");
}

#[tokio::test]
#[ignore]
async fn test_list_documents_orders_by_newest_first() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-list-1";

    seed_test_data(&pool, tenant_id, "Test").await.unwrap();

    let docs = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::list_documents_tx(tx, tenant_id, 100).await
        })
    })
    .await
    .unwrap();

    assert_eq!(docs.len(), 2);

    // Verify ordered by created_at DESC (newest first)
    assert!(
        docs[0].created_at >= docs[1].created_at,
        "Documents should be ordered newest first"
    );
}

#[tokio::test]
#[ignore]
async fn test_list_documents_respects_limit() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-list-limit";

    seed_test_data(&pool, tenant_id, "Test").await.unwrap();

    let docs = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::list_documents_tx(tx, tenant_id, 1).await
        })
    })
    .await
    .unwrap();

    assert_eq!(docs.len(), 1, "Should respect limit");
}

#[tokio::test]
#[ignore]
async fn test_search_within_document_only_searches_target_document() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-doc-search";

    let (doc1_id, _doc2_id) = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            let doc1 = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Doc 1",
                "s3://bucket/doc1.pdf",
            )
            .await?;

            VectorStore::insert_chunk_tx(
                tx,
                doc1.id,
                "Doc1 content",
                0,
                &vec![1.0; 1024],
            )
            .await?;

            let doc2 = VectorStore::insert_document_tx(
                tx,
                tenant_id,
                "Doc 2",
                "s3://bucket/doc2.pdf",
            )
            .await?;

            VectorStore::insert_chunk_tx(
                tx,
                doc2.id,
                "Doc2 content",
                0,
                &vec![0.5; 1024],
            )
            .await?;

            Ok::<_, anyhow::Error>((doc1.id, doc2.id))
        })
    })
    .await
    .unwrap();

    // Search within doc1 only
    let results = db::with_tenant(&pool, tenant_id, |tx| {
        Box::pin(async move {
            VectorStore::search_within_document_tx(
                tx,
                &vec![1.0; 1024],
                doc1_id,
                tenant_id,
                10,
            )
            .await
        })
    })
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].document_id, doc1_id);
    assert_eq!(results[0].content, "Doc1 content");
}
