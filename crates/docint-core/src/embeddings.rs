//! Bedrock Titan Text Embeddings v2 client.
//! Converts text into 1024-dimensional vectors for similarity search.

use anyhow::{Context, Result};
use aws_sdk_bedrockruntime::primitives::Blob;
use aws_sdk_bedrockruntime::Client;
use serde::{Deserialize, Serialize};

const MODEL_ID: &str = "amazon.titan-embed-text-v2:0";
const DIMENSIONS: u32 = 1024; // Must match the vector(1024) column in PostgreSQL

/// Request body sent to the Titan embeddings API.
#[derive(Serialize)]
struct TitanRequest<'a> {
    #[serde(rename = "inputText")]
    input_text: &'a str,
    dimensions: u32,
    normalize: bool,
}

/// Response body from the Titan embeddings API.
#[derive(Deserialize)]
struct TitanResponse {
    embedding: Vec<f32>,
}

/// Standalone embedder — not coupled to VectorStore so it can be
/// shared between ingestion (writes) and search (reads).
pub struct Embedder {
    client: Client,
}

impl Embedder {
    /// Create an embedder using default AWS credentials (env/profile/IMDS).
    pub async fn new() -> Self {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        Self {
            client: Client::new(&config),
        }
    }

    /// Create an embedder from a pre-configured Bedrock Runtime client.
    pub fn from_client(client: Client) -> Self {
        Self { client }
    }

    /// Convert text to a 1024-dim embedding vector via Bedrock Titan v2.
    #[tracing::instrument(skip(self, text), fields(text_len = text.len()))]
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let body = serde_json::to_vec(&TitanRequest {
            input_text: text,
            dimensions: DIMENSIONS,
            normalize: true,
        })?;

        let resp = self
            .client
            .invoke_model()
            .model_id(MODEL_ID)
            .body(Blob::new(body))
            .send()
            .await
            .context(format!("Bedrock InvokeModel failed (text_len={})", text.len()))?;

        let parsed: TitanResponse =
            serde_json::from_slice(resp.body().as_ref()).context("Failed to parse Titan response")?;

        Ok(parsed.embedding)
    }
}
