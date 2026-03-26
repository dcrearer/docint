//! Data types shared across the application.
//! Each struct derives `sqlx::FromRow` so it can be loaded directly from SQL queries.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A document record. Represents a source file (PDF, text, etc).
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Document {
    pub id: Uuid,
    pub tenant_id: String,
    pub title: String,
    pub source_key: String, // S3 key where the original file lives
    pub created_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

/// A chunk of text from a document, with its embedding stored separately in pgvector.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Chunk {
    pub id: Uuid,
    pub document_id: Uuid,
    pub content: String,
    pub chunk_index: i32,
    pub created_at: DateTime<Utc>,
}

/// A search result combining chunk content with its distance score and parent document title.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct SearchResult {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    pub content: String,
    pub distance: f64, // cosine distance: 0 = identical, 2 = opposite
    pub title: String,
}

/// Document metadata with chunk count. Used by the metadata Lambda.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DocumentMetadata {
    pub id: Uuid,
    pub tenant_id: String,
    pub title: String,
    pub source_key: String,
    pub created_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub chunk_count: i32,
}
