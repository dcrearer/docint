//! Vector store: the main data access layer.
//! Handles document/chunk CRUD and all search operations against pgvector.

use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Chunk, Document, DocumentMetadata, SearchResult};

pub struct VectorStore {
    pool: PgPool,
}

impl VectorStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Set the RLS tenant context. Must be called before any query.
    pub async fn set_tenant(&self, tenant_id: &str) -> Result<()> {
        crate::db::set_tenant(&self.pool, tenant_id).await?;
        Ok(())
    }

    /// Create or replace a document record. On re-ingest of the same S3 key,
    /// deletes old chunks (via CASCADE) and returns the refreshed row.
    pub async fn insert_document(&self, tenant_id: &str, title: &str, source_key: &str) -> Result<Document> {
        let doc = sqlx::query_as::<_, Document>(
            "INSERT INTO documents (tenant_id, title, source_key)
             VALUES ($1, $2, $3)
             ON CONFLICT (tenant_id, source_key) DO UPDATE
                SET title = EXCLUDED.title, created_at = now()
             RETURNING *"
        )
        .bind(tenant_id)
        .bind(title)
        .bind(source_key)
        .fetch_one(&self.pool)
        .await?;

        // Delete old chunks so re-ingest starts fresh
        sqlx::query("DELETE FROM chunks WHERE document_id = $1")
            .bind(doc.id)
            .execute(&self.pool)
            .await?;

        Ok(doc)
    }

    /// Insert a chunk with its embedding. The embedding is stored as a pgvector column.
    #[tracing::instrument(skip(self, content, embedding))]
    pub async fn insert_chunk(&self, document_id: Uuid, content: &str, chunk_index: i32, embedding: &[f32]) -> Result<Chunk> {
        let chunk = sqlx::query_as::<_, Chunk>(
            "INSERT INTO chunks (document_id, content, chunk_index, embedding) VALUES ($1, $2, $3, $4) RETURNING id, document_id, content, chunk_index, created_at"
        )
        .bind(document_id)
        .bind(content)
        .bind(chunk_index)
        .bind(Vector::from(embedding.to_vec()))
        .fetch_one(&self.pool)
        .await?;

        Ok(chunk)
    }

    /// Pure vector similarity search using cosine distance (<=> operator).
    #[tracing::instrument(skip(self, embedding))]
    pub async fn similarity_search(&self, embedding: &[f32], tenant_id: &str, limit: i64) -> Result<Vec<SearchResult>> {
        let results = sqlx::query_as::<_, SearchResult>(
            "SELECT c.id AS chunk_id, c.document_id, c.content, c.embedding <=> $1 AS distance, d.title
             FROM chunks c
             JOIN documents d ON d.id = c.document_id
             WHERE d.tenant_id = $2
             ORDER BY c.embedding <=> $1
             LIMIT $3"
        )
        .bind(Vector::from(embedding.to_vec()))
        .bind(tenant_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }

    /// Hybrid search: combines vector similarity + PostgreSQL full-text search
    /// using Reciprocal Rank Fusion (RRF). This gives better recall than either
    /// method alone — vector catches semantic matches, FTS catches keyword matches.
    ///
    /// How RRF works:
    ///   1. Run vector search → rank results 1..N
    ///   2. Run full-text search → rank results 1..M
    ///   3. For each result: score = 1/(60+vector_rank) + 1/(60+fts_rank)
    ///   4. Sort by combined score
    #[tracing::instrument(skip(self, embedding))]
    pub async fn hybrid_search(&self, embedding: &[f32], query: &str, tenant_id: &str, limit: i64) -> Result<Vec<SearchResult>> {
        let results = sqlx::query_as::<_, SearchResult>(
            "WITH vector_ranked AS (
                SELECT c.id, c.document_id, c.content, d.title,
                       c.embedding <=> $1 AS distance,
                       ROW_NUMBER() OVER (ORDER BY c.embedding <=> $1) AS rank
                FROM chunks c
                JOIN documents d ON d.id = c.document_id
                WHERE d.tenant_id = $2
                ORDER BY c.embedding <=> $1
                LIMIT 50
            ),
            fts_ranked AS (
                SELECT c.id, c.document_id, c.content, d.title,
                       c.embedding <=> $1 AS distance,
                       ROW_NUMBER() OVER (ORDER BY ts_rank(c.tsv, websearch_to_tsquery('english', $3)) DESC) AS rank
                FROM chunks c
                JOIN documents d ON d.id = c.document_id
                WHERE d.tenant_id = $2 AND c.tsv @@ websearch_to_tsquery('english', $3)
                LIMIT 50
            ),
            combined AS (
                SELECT COALESCE(v.id, f.id) AS chunk_id,
                       COALESCE(v.document_id, f.document_id) AS document_id,
                       COALESCE(v.content, f.content) AS content,
                       COALESCE(v.distance, f.distance) AS distance,
                       COALESCE(v.title, f.title) AS title,
                       COALESCE(1.0 / (60 + v.rank), 0.0) + COALESCE(1.0 / (60 + f.rank), 0.0) AS rrf_score
                FROM vector_ranked v
                FULL OUTER JOIN fts_ranked f ON v.id = f.id
            )
            SELECT chunk_id, document_id, content, distance, title
            FROM combined
            ORDER BY rrf_score DESC
            LIMIT $4"
        )
        .bind(Vector::from(embedding.to_vec()))
        .bind(tenant_id)
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }

    /// Get metadata for a single document, including chunk count.
    pub async fn get_metadata(&self, document_id: Uuid, tenant_id: &str) -> Result<Option<DocumentMetadata>> {
        let meta = sqlx::query_as::<_, DocumentMetadata>(
            "SELECT d.id, d.tenant_id, d.title, d.source_key, d.created_at, d.metadata,
                    COUNT(c.id)::int AS chunk_count
             FROM documents d
             LEFT JOIN chunks c ON c.document_id = d.id
             WHERE d.id = $1 AND d.tenant_id = $2
             GROUP BY d.id"
        )
        .bind(document_id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(meta)
    }

    /// List all documents for a tenant, ordered by newest first.
    pub async fn list_documents(&self, tenant_id: &str, limit: i64) -> Result<Vec<DocumentMetadata>> {
        let docs = sqlx::query_as::<_, DocumentMetadata>(
            "SELECT d.id, d.tenant_id, d.title, d.source_key, d.created_at, d.metadata,
                    COUNT(c.id)::int AS chunk_count
             FROM documents d
             LEFT JOIN chunks c ON c.document_id = d.id
             WHERE d.tenant_id = $1
             GROUP BY d.id
             ORDER BY d.created_at DESC
             LIMIT $2"
        )
        .bind(tenant_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(docs)
    }

    /// Search within a single document. Used by the compare Lambda
    /// to find relevant chunks in each of two documents separately.
    #[tracing::instrument(skip(self, embedding))]
    pub async fn search_within_document(&self, embedding: &[f32], document_id: Uuid, tenant_id: &str, limit: i64) -> Result<Vec<SearchResult>> {
        let results = sqlx::query_as::<_, SearchResult>(
            "SELECT c.id AS chunk_id, c.document_id, c.content, c.embedding <=> $1 AS distance, d.title
             FROM chunks c
             JOIN documents d ON d.id = c.document_id
             WHERE c.document_id = $2 AND d.tenant_id = $3
             ORDER BY c.embedding <=> $1
             LIMIT $4"
        )
        .bind(Vector::from(embedding.to_vec()))
        .bind(document_id)
        .bind(tenant_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(results)
    }
}
