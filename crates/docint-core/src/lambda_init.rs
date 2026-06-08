//! Shared initialization helpers for Lambda handlers.
//!
//! This module provides common initialization logic for all Lambda functions,
//! reducing duplication across handlers and ensuring consistent setup.

use crate::{db, store::VectorStore, embeddings::Embedder};
use anyhow::Result;

/// Application state for Lambdas that need both DB and embeddings
/// (search, compare, ingest)
pub struct AppState {
    pub store: VectorStore,
    pub embedder: Embedder,
}

/// Initialize full app state (DB + embedder)
pub async fn init_app_state() -> Result<AppState> {
    let url = db::resolve_database_url().await?;
    let pool = db::create_pool(&url).await?;
    Ok(AppState {
        store: VectorStore::new(pool),
        embedder: Embedder::new().await,
    })
}

/// Initialize DB-only state (for metadata Lambda)
pub async fn init_store() -> Result<VectorStore> {
    let url = db::resolve_database_url().await?;
    let pool = db::create_pool(&url).await?;
    Ok(VectorStore::new(pool))
}

/// Configure tracing for Lambda (JSON format)
pub fn setup_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}
