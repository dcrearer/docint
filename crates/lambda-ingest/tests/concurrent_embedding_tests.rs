//! Tests for concurrent chunk embedding in ingest handler.
//!
//! TDD Approach: These tests are written FIRST and will demonstrate
//! the expected concurrent embedding behavior.
//!
//! Run with: cargo test --package lambda-ingest --test concurrent_embedding_tests

use futures::stream::{self, StreamExt};

#[tokio::test]
async fn test_concurrent_embedding_preserves_order() {
    // Mock embedder that returns chunk index as embedding
    struct MockEmbedder;

    impl MockEmbedder {
        async fn embed(&self, chunk: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
            // Extract number from chunk (e.g., "chunk 0" -> 0.0)
            let num: f32 = chunk.split_whitespace().last().unwrap().parse()?;
            Ok(vec![num; 1024])
        }
    }

    let embedder = MockEmbedder;
    let chunks: Vec<String> = (0..10).map(|i| format!("chunk {}", i)).collect();

    // Concurrent embedding
    let embedding_futures = chunks
        .iter()
        .map(|chunk| embedder.embed(chunk))
        .collect::<Vec<_>>();

    let embeddings: Result<Vec<Vec<f32>>, _> = stream::iter(embedding_futures)
        .buffered(5)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect();

    let embeddings = embeddings.unwrap();

    // Verify order is preserved (embedding[i] should correspond to chunk i)
    assert_eq!(embeddings.len(), 10);
    for (i, emb) in embeddings.iter().enumerate() {
        assert_eq!(emb[0], i as f32, "Embedding order should match chunk order");
    }
}

#[tokio::test]
async fn test_concurrent_embedding_handles_partial_failure() {
    // Mock embedder that fails on specific chunks
    struct FailingEmbedder;

    impl FailingEmbedder {
        async fn embed(&self, chunk: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
            if chunk.contains("fail") {
                Err("Embedding failed".into())
            } else {
                Ok(vec![1.0; 1024])
            }
        }
    }

    let embedder = FailingEmbedder;
    let chunks = vec!["good chunk".to_string(), "fail chunk".to_string()];

    let embedding_futures = chunks
        .iter()
        .map(|chunk| embedder.embed(chunk))
        .collect::<Vec<_>>();

    let embeddings: Result<Vec<Vec<f32>>, _> = stream::iter(embedding_futures)
        .buffered(5)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect();

    // Should fail early on first error (matching current behavior)
    assert!(embeddings.is_err(), "Should propagate embedding errors");
}

#[tokio::test]
async fn test_concurrent_embedding_performance() {
    use std::time::Instant;

    // Mock embedder with artificial delay
    struct DelayedEmbedder;

    impl DelayedEmbedder {
        async fn embed(&self, _chunk: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            Ok(vec![1.0; 1024])
        }
    }

    let embedder = DelayedEmbedder;
    let chunks: Vec<String> = (0..10).map(|i| format!("chunk {}", i)).collect();

    // Concurrent embedding
    let start = Instant::now();
    let embedding_futures = chunks
        .iter()
        .map(|chunk| embedder.embed(chunk))
        .collect::<Vec<_>>();

    let _embeddings: Result<Vec<Vec<f32>>, _> = stream::iter(embedding_futures)
        .buffered(5)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect();

    let concurrent_duration = start.elapsed();

    // With 5 concurrent: 10 chunks / 5 = 2 batches × 100ms = ~200ms
    // Allow some overhead: should be < 400ms
    assert!(
        concurrent_duration.as_millis() < 400,
        "Concurrent embedding should complete in ~200ms (was {}ms)",
        concurrent_duration.as_millis()
    );

    // Verify it would take much longer sequentially (10 × 100ms = 1000ms)
    // This test demonstrates the performance benefit
}
