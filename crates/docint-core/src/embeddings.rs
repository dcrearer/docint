//! Bedrock Titan Text Embeddings v2 client.
//! Converts text into 1024-dimensional vectors for similarity search.

use anyhow::{Context, Result};
use aws_sdk_bedrockruntime::Client;
use aws_sdk_bedrockruntime::primitives::Blob;
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
            .context(format!(
                "Bedrock InvokeModel failed (text_len={})",
                text.len()
            ))?;

        let parsed: TitanResponse = serde_json::from_slice(resp.body().as_ref())
            .context("Failed to parse Titan response")?;

        Ok(parsed.embedding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titan_request_serializes_correctly() {
        let req = TitanRequest {
            input_text: "test text",
            dimensions: 1024,
            normalize: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"inputText\":\"test text\""));
        assert!(json.contains("\"dimensions\":1024"));
        assert!(json.contains("\"normalize\":true"));
    }

    #[test]
    fn titan_request_uses_correct_dimensions() {
        let req = TitanRequest {
            input_text: "",
            dimensions: DIMENSIONS,
            normalize: true,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["dimensions"], 1024);
    }

    #[test]
    fn titan_response_deserializes_1024_dimensions() {
        let response_json = r#"{"embedding": [0.1, 0.2]}"#;
        let resp: TitanResponse = serde_json::from_str(response_json).unwrap();
        assert_eq!(resp.embedding.len(), 2);
        assert_eq!(resp.embedding[0], 0.1);
        assert_eq!(resp.embedding[1], 0.2);
    }

    #[test]
    fn titan_response_deserializes_full_vector() {
        // Build a JSON string with 1024 embedding values
        let mut values = Vec::with_capacity(1024);
        for i in 0..1024 {
            values.push(format!("{}", (i as f32) / 1024.0));
        }
        let json = format!(r#"{{"embedding": [{}]}}"#, values.join(","));

        let parsed: TitanResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.embedding.len(), 1024);
        assert_eq!(parsed.embedding[0], 0.0);
        assert!((parsed.embedding[1023] - (1023.0 / 1024.0)).abs() < 0.0001);
    }

    #[test]
    fn titan_request_handles_empty_text() {
        let req = TitanRequest {
            input_text: "",
            dimensions: DIMENSIONS,
            normalize: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"inputText\":\"\""));
    }

    #[test]
    fn titan_request_handles_unicode() {
        let req = TitanRequest {
            input_text: "Hello 世界 🌍",
            dimensions: DIMENSIONS,
            normalize: true,
        };
        let result = serde_json::to_string(&req);
        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("Hello"));
    }

    #[test]
    fn model_id_is_titan_v2() {
        assert_eq!(MODEL_ID, "amazon.titan-embed-text-v2:0");
    }

    #[test]
    fn dimensions_matches_postgres_schema() {
        // Must match vector(1024) in PostgreSQL schema
        assert_eq!(DIMENSIONS, 1024);
    }
}
