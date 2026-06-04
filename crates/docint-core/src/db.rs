//! Database connection pool and tenant context for row-level security.

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use sqlx::{PgPool, Postgres, Transaction};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

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
