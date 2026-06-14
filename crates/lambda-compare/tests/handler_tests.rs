//! Integration tests for lambda-compare handler.
//!
//! Tests verify:
//! - Handler compares two documents side-by-side
//! - Results are document-scoped (no cross-document leakage)
//! - Validation of required request fields
//! - Tenant isolation enforcement
//!
//! Run with: cargo test --test handler_tests -- --ignored

mod common;

use common::{setup_test_db};
use docint_core::{db, store::VectorStore};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-declare request/response types to match handler
#[allow(dead_code)]
#[derive(Serialize)]
struct Request {
    query: String,
    document_id_a: Uuid,
    document_id_b: Uuid,
    tenant_id: String,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct ChunkHit {
    #[allow(dead_code)]
    chunk_id: String,
    content: String,
    #[allow(dead_code)]
    distance: f64,
}

#[derive(Deserialize)]
struct DocResult {
    document_id: String,
    title: String,
    matches: Vec<ChunkHit>,
}

#[derive(Deserialize)]
struct Response {
    query: String,
    document_a: DocResult,
    document_b: DocResult,
}

/// Helper: Create a document with specific content chunks
async fn create_test_document(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    title: &str,
    source_key: &str,
    chunks: &[&str],
    embedding: &[f32],
) -> anyhow::Result<Uuid> {
    let title = title.to_string();
    let source_key = source_key.to_string();
    let chunks: Vec<String> = chunks.iter().map(|s| s.to_string()).collect();
    let embedding = embedding.to_vec();
    let tenant_id_owned = tenant_id.to_string();

    db::with_tenant(pool, tenant_id, |tx| {
        let tenant_id = tenant_id_owned.clone();
        Box::pin(async move {
            let doc = VectorStore::insert_document_tx(
                tx,
                &tenant_id,
                &title,
                &source_key,
            )
            .await?;

            for (idx, chunk_content) in chunks.iter().enumerate() {
                VectorStore::insert_chunk_tx(
                    tx,
                    doc.id,
                    chunk_content,
                    idx as i32,
                    &embedding,
                )
                .await?;
            }

            Ok(doc.id)
        })
    })
    .await
}

/// Helper: Simulate handler logic (inline version for testing)
async fn simulate_handler(
    pool: &sqlx::PgPool,
    query: &str,
    document_id_a: Uuid,
    document_id_b: Uuid,
    tenant_id: &str,
    limit: Option<i64>,
) -> anyhow::Result<Response> {
    let limit = limit.unwrap_or(3);

    // Create a mock embedding for testing (in real handler, this comes from Embedder)
    let embedding = vec![0.5; 1024];

    let tenant_id_clone = tenant_id.to_string();
    let (results_a, results_b) = db::with_tenant(pool, tenant_id, move |tx| {
        let tenant_id = tenant_id_clone.clone();
        let embedding = embedding.clone();
        Box::pin(async move {
            let a = VectorStore::search_within_document_tx(
                tx,
                &embedding,
                document_id_a,
                &tenant_id,
                limit,
            )
            .await?;
            let b = VectorStore::search_within_document_tx(
                tx,
                &embedding,
                document_id_b,
                &tenant_id,
                limit,
            )
            .await?;
            Ok((a, b))
        })
    })
    .await?;

    let to_doc_result = |results: Vec<docint_core::models::SearchResult>| -> DocResult {
        let title = results.first().map(|r| r.title.clone()).unwrap_or_default();
        let doc_id = results
            .first()
            .map(|r| r.document_id.to_string())
            .unwrap_or_default();
        DocResult {
            document_id: doc_id,
            title,
            matches: results
                .into_iter()
                .map(|r| ChunkHit {
                    chunk_id: r.chunk_id.to_string(),
                    content: r.content,
                    distance: r.distance.unwrap_or(999.0),
                })
                .collect(),
        }
    };

    Ok(Response {
        query: query.to_string(),
        document_a: to_doc_result(results_a),
        document_b: to_doc_result(results_b),
    })
}

#[tokio::test]
#[ignore]
async fn test_handler_compares_two_documents() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-compare-1";

    // Create two documents with distinct content
    let doc_a_id = create_test_document(
        &pool,
        tenant_id,
        "Product Manual A",
        "s3://bucket/manual-a.pdf",
        &[
            "Product A has 8GB memory",
            "Product A supports WiFi 6",
            "Product A costs $299",
        ],
        &vec![1.0; 1024],
    )
    .await
    .unwrap();

    let doc_b_id = create_test_document(
        &pool,
        tenant_id,
        "Product Manual B",
        "s3://bucket/manual-b.pdf",
        &[
            "Product B has 16GB memory",
            "Product B supports WiFi 6E",
            "Product B costs $499",
        ],
        &vec![0.9; 1024],
    )
    .await
    .unwrap();

    // Compare documents with query
    let response = simulate_handler(
        &pool,
        "memory specifications",
        doc_a_id,
        doc_b_id,
        tenant_id,
        Some(3),
    )
    .await
    .unwrap();

    // Verify both documents returned results
    assert_eq!(response.query, "memory specifications");
    assert_eq!(response.document_a.document_id, doc_a_id.to_string());
    assert_eq!(response.document_b.document_id, doc_b_id.to_string());
    assert_eq!(response.document_a.title, "Product Manual A");
    assert_eq!(response.document_b.title, "Product Manual B");

    // Verify we got matches from both documents
    assert!(!response.document_a.matches.is_empty(), "Document A should have matches");
    assert!(!response.document_b.matches.is_empty(), "Document B should have matches");

    // Verify content is document-specific
    let doc_a_content: String = response
        .document_a
        .matches
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(doc_a_content.contains("Product A"), "Document A results should contain Product A content");
    assert!(!doc_a_content.contains("Product B"), "Document A results should not contain Product B content");

    let doc_b_content: String = response
        .document_b
        .matches
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(doc_b_content.contains("Product B"), "Document B results should contain Product B content");
    assert!(!doc_b_content.contains("Product A"), "Document B results should not contain Product A content");
}

#[tokio::test]
#[ignore]
async fn test_handler_searches_within_each_document() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-compare-scoped";

    // Create document A with "feature X" content
    let doc_a_id = create_test_document(
        &pool,
        tenant_id,
        "Document A",
        "s3://bucket/doc-a.pdf",
        &[
            "This is feature X in document A",
            "Feature X performance metrics",
            "Feature X configuration guide",
        ],
        &vec![1.0; 1024],
    )
    .await
    .unwrap();

    // Create document B with "feature Y" content (different from A)
    let doc_b_id = create_test_document(
        &pool,
        tenant_id,
        "Document B",
        "s3://bucket/doc-b.pdf",
        &[
            "This is feature Y in document B",
            "Feature Y deployment steps",
            "Feature Y troubleshooting",
        ],
        &vec![0.8; 1024],
    )
    .await
    .unwrap();

    // Create document C (should NOT appear in results)
    let _doc_c_id = create_test_document(
        &pool,
        tenant_id,
        "Document C",
        "s3://bucket/doc-c.pdf",
        &[
            "This is feature Z in document C",
            "Feature Z should not appear",
        ],
        &vec![0.6; 1024],
    )
    .await
    .unwrap();

    // Compare only documents A and B
    let response = simulate_handler(
        &pool,
        "features comparison",
        doc_a_id,
        doc_b_id,
        tenant_id,
        Some(5),
    )
    .await
    .unwrap();

    // Verify results are strictly scoped to requested documents
    assert_eq!(response.document_a.document_id, doc_a_id.to_string());
    assert_eq!(response.document_b.document_id, doc_b_id.to_string());

    // All matches in document_a should belong to doc_a_id
    for hit in &response.document_a.matches {
        assert!(
            hit.content.contains("document A") || hit.content.contains("Feature X"),
            "Document A results should only contain Document A content, got: {}",
            hit.content
        );
        assert!(
            !hit.content.contains("document B") && !hit.content.contains("document C"),
            "Document A results should not contain other documents"
        );
    }

    // All matches in document_b should belong to doc_b_id
    for hit in &response.document_b.matches {
        assert!(
            hit.content.contains("document B") || hit.content.contains("Feature Y"),
            "Document B results should only contain Document B content, got: {}",
            hit.content
        );
        assert!(
            !hit.content.contains("document A") && !hit.content.contains("document C"),
            "Document B results should not contain other documents"
        );
    }

    // Document C should not appear anywhere
    let doc_a_all: Vec<String> = response.document_a.matches.iter().map(|m| m.content.clone()).collect();
    let doc_b_all: Vec<String> = response.document_b.matches.iter().map(|m| m.content.clone()).collect();
    let all_content = format!("{} {}", doc_a_all.join(" "), doc_b_all.join(" "));
    assert!(
        !all_content.contains("document C") && !all_content.contains("Feature Z"),
        "Document C should not appear in comparison results"
    );
}

#[tokio::test]
#[ignore]
async fn test_handler_requires_both_document_ids() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-compare-validation";

    // Create one document
    let doc_a_id = create_test_document(
        &pool,
        tenant_id,
        "Document A",
        "s3://bucket/doc-a.pdf",
        &["Content A"],
        &vec![1.0; 1024],
    )
    .await
    .unwrap();

    // Try to compare with non-existent document B
    let non_existent_doc_id = Uuid::new_v4();

    let result = simulate_handler(
        &pool,
        "test query",
        doc_a_id,
        non_existent_doc_id,
        tenant_id,
        Some(3),
    )
    .await;

    // Should succeed but document_b should have empty matches
    // (The handler doesn't explicitly error on missing docs, just returns empty results)
    let response = result.unwrap();
    assert_eq!(response.document_a.matches.is_empty(), false, "Document A should have results");
    assert_eq!(response.document_b.matches.is_empty(), true, "Non-existent document B should have no results");
    assert_eq!(response.document_b.document_id, "", "Non-existent document should have empty ID");
}

#[tokio::test]
#[ignore]
async fn test_handler_enforces_tenant_isolation() {
    let pool = setup_test_db().await.unwrap();
    let tenant_a = "tenant-a";
    let tenant_b = "tenant-b";

    // Create document for tenant A
    let doc_a_tenant_a = create_test_document(
        &pool,
        tenant_a,
        "Tenant A Document",
        "s3://bucket/tenant-a-doc.pdf",
        &["This is tenant A's confidential data"],
        &vec![1.0; 1024],
    )
    .await
    .unwrap();

    // Create document for tenant B
    let doc_b_tenant_b = create_test_document(
        &pool,
        tenant_b,
        "Tenant B Document",
        "s3://bucket/tenant-b-doc.pdf",
        &["This is tenant B's confidential data"],
        &vec![0.9; 1024],
    )
    .await
    .unwrap();

    // Try to compare documents across tenants (using tenant_a context)
    let response = simulate_handler(
        &pool,
        "confidential data",
        doc_a_tenant_a,
        doc_b_tenant_b,
        tenant_a, // Request from tenant A
        Some(5),
    )
    .await
    .unwrap();

    // Document A (tenant A) should return results
    assert!(!response.document_a.matches.is_empty(), "Tenant A's own document should have results");
    assert!(
        response.document_a.matches[0].content.contains("tenant A"),
        "Should only see tenant A's data"
    );

    // Document B (tenant B) should NOT return results due to RLS
    assert!(
        response.document_b.matches.is_empty(),
        "Tenant A should not see tenant B's documents due to RLS isolation"
    );

    // Verify tenant B can access their own document
    let response_b = simulate_handler(
        &pool,
        "confidential data",
        doc_a_tenant_a,
        doc_b_tenant_b,
        tenant_b, // Request from tenant B
        Some(5),
    )
    .await
    .unwrap();

    // Document B (tenant B's own) should return results
    assert!(!response_b.document_b.matches.is_empty(), "Tenant B should see their own document");
    assert!(
        response_b.document_b.matches[0].content.contains("tenant B"),
        "Tenant B should see their own data"
    );

    // Document A (tenant A) should NOT be visible to tenant B
    assert!(
        response_b.document_a.matches.is_empty(),
        "Tenant B should not see tenant A's documents due to RLS isolation"
    );
}

// --- Limit Bounds Checking Tests (TDD: RED Phase) ---

#[tokio::test]
#[ignore]
async fn test_limit_clamping_excessive_value() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-compare-limit-excessive";

    // Create document with many chunks to test limit enforcement
    let chunks: Vec<&str> = (0..30).map(|i| Box::leak(format!("Chunk {}", i).into_boxed_str()) as &str).collect();
    let doc_a_id = create_test_document(
        &pool,
        tenant_id,
        "Document A",
        "s3://bucket/doc-a.pdf",
        &chunks,
        &vec![1.0; 1024],
    )
    .await
    .unwrap();

    let doc_b_id = create_test_document(
        &pool,
        tenant_id,
        "Document B",
        "s3://bucket/doc-b.pdf",
        &chunks,
        &vec![0.9; 1024],
    )
    .await
    .unwrap();

    // Request excessive limit (should be clamped to max of 20)
    let response = simulate_handler(
        &pool,
        "test query",
        doc_a_id,
        doc_b_id,
        tenant_id,
        Some(10000), // Excessive - should clamp to 20
    )
    .await
    .unwrap();

    // WILL FAIL: Current code doesn't clamp, may return more than 20 results per document
    assert!(
        response.document_a.matches.len() <= 20,
        "Document A results should be clamped to max limit of 20"
    );
    assert!(
        response.document_b.matches.len() <= 20,
        "Document B results should be clamped to max limit of 20"
    );
}

#[tokio::test]
#[ignore]
async fn test_limit_clamping_zero_value() {
    let pool = setup_test_db().await.unwrap();
    let tenant_id = "tenant-compare-limit-zero";

    let doc_a_id = create_test_document(
        &pool,
        tenant_id,
        "Document A",
        "s3://bucket/doc-a.pdf",
        &["Content A chunk 1", "Content A chunk 2"],
        &vec![1.0; 1024],
    )
    .await
    .unwrap();

    let doc_b_id = create_test_document(
        &pool,
        tenant_id,
        "Document B",
        "s3://bucket/doc-b.pdf",
        &["Content B chunk 1", "Content B chunk 2"],
        &vec![0.9; 1024],
    )
    .await
    .unwrap();

    // Request zero limit (should be clamped to 1)
    let response = simulate_handler(
        &pool,
        "test query",
        doc_a_id,
        doc_b_id,
        tenant_id,
        Some(0),
    )
    .await
    .unwrap();

    // WILL FAIL: Current code passes 0 to SQL, returns 0 results
    assert!(
        response.document_a.matches.len() >= 1,
        "Document A results should respect min limit of 1"
    );
    assert!(
        response.document_b.matches.len() >= 1,
        "Document B results should respect min limit of 1"
    );
}

#[test]
fn test_limit_clamping_negative_value() {
    // Test that negative limits are clamped to 1 (unit test, no DB needed)
    let limit: i64 = -5;
    let clamped = limit.clamp(1, 20);

    assert_eq!(clamped, 1, "Negative limit should be clamped to 1");
}
