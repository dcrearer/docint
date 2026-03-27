use anyhow::{Context, Result};
use aws_sdk_bedrockagentcore::primitives::Blob;
use clap::Parser;
use serde::Serialize;

#[derive(Parser)]
#[command(name = "docint", about = "Query the Document Intelligence agent")]
struct Cli {
    /// The question to ask the agent
    prompt: String,

    /// Tenant ID for multi-tenant isolation
    #[arg(short, long, default_value = "tenant-1")]
    tenant: String,

    /// Agent runtime ARN
    #[arg(long, env("DOCINT_RUNTIME_ARN"))]
    runtime_arn: String,

    /// Endpoint qualifier
    #[arg(long, env("DOCINT_ENDPOINT"), default_value = "docint_agent_endpoint")]
    endpoint: String,
}

#[derive(Serialize)]
struct Request {
    prompt: String,
    tenant_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = aws_sdk_bedrockagentcore::Client::new(&config);

    let payload = serde_json::to_vec(&Request {
        prompt: cli.prompt,
        tenant_id: cli.tenant,
    })?;

    let resp = client
        .invoke_agent_runtime()
        .agent_runtime_arn(&cli.runtime_arn)
        .qualifier(&cli.endpoint)
        .payload(Blob::new(payload))
        .send()
        .await
        .context("Failed to invoke agent")?;

    let bytes = resp.response.collect().await?.into_bytes();
    let output = String::from_utf8_lossy(&bytes);
    // Strip surrounding quotes if present
    let output = output.trim().trim_matches('"');
    println!("{output}");

    Ok(())
}
