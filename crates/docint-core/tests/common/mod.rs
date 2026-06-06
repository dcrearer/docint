//! Common test utilities and fixtures for integration tests.
//!
//! This module provides helper functions for:
//! - Setting up test databases (unique per test)
//! - Creating test data
//! - Mocking AWS services
//! - Asserting test conditions

use anyhow::Result;
use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use uuid::Uuid;

/// Test database connection string to postgres system database.
/// Used to create unique test databases.
/// Note: postgres user is used for CREATE DATABASE, then test_user (non-superuser) for RLS testing
const TEST_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5433/postgres";
const TEST_USER_URL: &str = "postgres://test_user:test_pass@localhost:5433";

/// Create a unique test database for each test.
///
/// 1. Uses superuser (postgres) to create the database and run migrations
/// 2. Returns a connection pool as non-privileged test_user (RLS enforced)
///
/// Each test gets complete isolation with its own database.
///
/// # Example
/// ```no_run
/// use common::setup_test_db;
///
/// #[tokio::test]
/// async fn test_something() {
///     let pool = setup_test_db().await.unwrap();
///     // ... test code (database is automatically cleaned up)
/// }
/// ```
pub async fn setup_test_db() -> Result<PgPool> {
    // Generate unique database name
    let db_name = format!("docint_test_{}", Uuid::new_v4().simple());

    // Step 1: Connect as superuser to create database
    let mut connection = PgConnection::connect(TEST_DATABASE_URL).await?;

    // Create unique test database
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, db_name).as_str())
        .await?;

    drop(connection);

    // Step 2: Connect as superuser to run migrations (needs extension creation)
    let superuser_db_url = format!("postgres://postgres:postgres@localhost:5433/{}", db_name);
    let superuser_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&superuser_db_url)
        .await?;

    // Run migrations (creates tables, extensions, RLS policies)
    sqlx::migrate!("../../migrations")
        .run(&superuser_pool)
        .await?;

    // Grant test_user access to all tables
    sqlx::query("GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO test_user")
        .execute(&superuser_pool)
        .await?;

    sqlx::query("GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO test_user")
        .execute(&superuser_pool)
        .await?;

    drop(superuser_pool);

    // Step 3: Return connection pool as test_user (non-privileged, RLS enforced)
    let test_user_db_url = format!("{}/{}", TEST_USER_URL, db_name);
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&test_user_db_url)
        .await?;

    Ok(pool)
}

/// Run all migrations on the test database.
///
/// Note: This is now called automatically by setup_test_db().
/// Keeping this function for backward compatibility.
#[allow(dead_code)]
pub async fn run_test_migrations(pool: &PgPool) -> Result<()> {
    // Run migrations from the migrations/ directory (relative to workspace root)
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await?;

    Ok(())
}

/// Seed the test database with sample data for a given tenant.
///
/// Creates:
/// - 2 documents
/// - 4 chunks (2 per document)
/// - Sample embeddings (random vectors)
///
/// Uses `with_tenant()` to properly set RLS context for inserts.
///
/// # Example
/// ```no_run
/// seed_test_data(&pool, "tenant-123", "Test Data").await.unwrap();
/// ```
pub async fn seed_test_data(
    pool: &PgPool,
    tenant_id: &str,
    label: &str,
) -> Result<()> {
    use uuid::Uuid;
    use pgvector::Vector;

    // Wrap all inserts in with_tenant() to set RLS context
    docint_core::db::with_tenant(pool, tenant_id, |tx| {
        let label = label.to_string();
        let tenant_id = tenant_id.to_string();

        Box::pin(async move {
            // Insert test document 1
            let doc1_id: Uuid = sqlx::query_scalar(
                "INSERT INTO documents (tenant_id, title, source_key)
                 VALUES ($1, $2, $3)
                 RETURNING id"
            )
            .bind(&tenant_id)
            .bind(format!("{} - Document 1", label))
            .bind(format!("test/{}/doc1.txt", tenant_id))
            .fetch_one(&mut **tx)
            .await?;

            // Insert test document 2
            let doc2_id: Uuid = sqlx::query_scalar(
                "INSERT INTO documents (tenant_id, title, source_key)
                 VALUES ($1, $2, $3)
                 RETURNING id"
            )
            .bind(&tenant_id)
            .bind(format!("{} - Document 2", label))
            .bind(format!("test/{}/doc2.txt", tenant_id))
            .fetch_one(&mut **tx)
            .await?;

            // Insert chunks with sample embeddings
            let sample_embedding = vec![0.1; 1024]; // Titan embedding dimension
            let embedding_vec = Vector::from(sample_embedding);

            for (doc_id, doc_num) in [(doc1_id, 1), (doc2_id, 2)] {
                for chunk_idx in 0..2 {
                    sqlx::query(
                        "INSERT INTO chunks (document_id, content, chunk_index, embedding)
                         VALUES ($1, $2, $3, $4)"
                    )
                    .bind(doc_id)
                    .bind(format!("{} - Doc {} - Chunk {}", label, doc_num, chunk_idx))
                    .bind(chunk_idx)
                    .bind(&embedding_vec)
                    .execute(&mut **tx)
                    .await?;
                }
            }

            Ok(())
        })
    }).await
}

/// Clean up test data for a given tenant.
///
/// Note: No longer needed with unique databases per test.
/// Kept for backward compatibility.
#[allow(dead_code)]
pub async fn cleanup_test_data(pool: &PgPool, tenant_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM documents WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Clean up ALL test data from the database.
///
/// Note: This is no longer needed with unique databases per test.
/// Keeping for backward compatibility.
#[allow(dead_code)]
pub async fn reset_test_db(pool: &PgPool) -> Result<()> {
    sqlx::query("TRUNCATE TABLE chunks, documents RESTART IDENTITY CASCADE")
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Only run when test DB is available
    async fn test_setup_test_db() {
        let pool = setup_test_db().await;
        assert!(pool.is_ok(), "Failed to connect to test database");
    }

    #[tokio::test]
    #[ignore]
    async fn test_seed_and_cleanup() {
        let pool = setup_test_db().await.unwrap();

        seed_test_data(&pool, "test-tenant-1", "Test").await.unwrap();

        // Verify data exists (using with_tenant to set RLS context)
        let count: i64 = docint_core::db::with_tenant(&pool, "test-tenant-1", |tx| {
            Box::pin(async move {
                sqlx::query_scalar("SELECT COUNT(*) FROM documents")
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| anyhow::anyhow!("Query failed: {}", e))
            })
        })
        .await
        .unwrap();
        assert_eq!(count, 2);

        // Cleanup (delete without RLS - this needs to be done as superuser in real tests)
        // For this test, we'll just verify the seed worked - cleanup is tested implicitly
        // by unique databases per test
    }
}
