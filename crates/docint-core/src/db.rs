//! Database connection pool and tenant context for row-level security.

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;

/// Resolve database credentials from AWS Secrets Manager and build connection URL.
/// Called once on Lambda cold start before creating the pool.
pub async fn resolve_database_url() -> Result<String> {
    let secret_arn = std::env::var("DB_SECRET_ARN")
        .context("DB_SECRET_ARN environment variable not set")?;
    let db_host = std::env::var("DB_HOST")
        .context("DB_HOST environment variable not set")?;
    let db_port = std::env::var("DB_PORT")
        .unwrap_or_else(|_| "5432".to_string());
    let db_name = std::env::var("DB_NAME")
        .unwrap_or_else(|_| "docint".to_string());

    // Fetch secret from Secrets Manager
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = aws_sdk_secretsmanager::Client::new(&config);

    let response = client
        .get_secret_value()
        .secret_id(&secret_arn)
        .send()
        .await
        .context("Failed to retrieve secret from Secrets Manager")?;

    let secret_string = response
        .secret_string()
        .context("Secret does not contain a string value")?;

    // Parse JSON secret (Aurora format: {"username": "...", "password": "..."})
    let secret: serde_json::Value = serde_json::from_str(secret_string)
        .context("Failed to parse secret JSON")?;

    let username = secret["username"]
        .as_str()
        .context("Missing 'username' field in secret")?;
    let password = secret["password"]
        .as_str()
        .context("Missing 'password' field in secret")?;

    // Build PostgreSQL connection URL
    let database_url = format!(
        "postgres://{}:{}@{}:{}/{}",
        username, password, db_host, db_port, db_name
    );

    Ok(database_url)
}

/// Create a connection pool. Called once on Lambda cold start via OnceCell.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(database_url)
        .await
}

/// Execute a closure within a transaction with tenant context set.
/// This ensures:
/// 1. set_config and all queries run on the SAME connection
/// 2. The tenant_id setting is transaction-scoped (cleared on commit)
/// 3. Connection returns to pool with no stale session state
///
/// # Example
/// ```ignore
/// let results = with_tenant(&pool, tenant_id, |tx| {
///     Box::pin(async move {
///         store.search_with_tx(tx, query).await
///     })
/// }).await?;
/// ```
pub async fn with_tenant<F, T>(pool: &PgPool, tenant_id: &str, f: F) -> Result<T>
where
    F: for<'a> FnOnce(&'a mut Transaction<'_, Postgres>) -> BoxFuture<'a, Result<T>>,
{
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;

    // Third parameter TRUE = transaction-scoped (cleared on COMMIT/ROLLBACK)
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .context("Failed to set tenant context")?;

    let result = f(&mut tx).await?;

    tx.commit().await.context("Failed to commit transaction")?;

    Ok(result)
}
