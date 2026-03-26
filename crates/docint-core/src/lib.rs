//! docint-core: shared library used by all Lambda functions and the CLI.
//! Contains the vector store, embeddings client, text chunker, and data models.

pub mod chunker;
pub mod db;
pub mod embeddings;
pub mod models;
pub mod store;
