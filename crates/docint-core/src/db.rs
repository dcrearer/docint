//! Database connection pool and tenant context for row-level security.

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

/// Create a connection pool. Called once on Lambda cold start via OnceCell.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(database_url)
        .await
}

/// Set the PostgreSQL session variable used by RLS policies.
/// Must be called before any query that touches RLS-protected tables.
/// Uses `set_config(..., false)` so it persists for the session, not just the transaction.
pub async fn set_tenant(pool: &PgPool, tenant_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.tenant_id', $1, false)")
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(())
}
