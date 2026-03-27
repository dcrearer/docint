use anyhow::{Context, Result};
use aws_sdk_bedrockagentcore::primitives::Blob;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::io::{self, Write};
use tokio::io::AsyncReadExt;

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

fn stream_unescape(raw: &[u8], stdout: &mut impl Write) -> io::Result<()> {
    let s = std::str::from_utf8(raw).unwrap_or("");
    let s = s.strip_prefix('"').unwrap_or(s);
    let s = s.strip_suffix('"').unwrap_or(s);

    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => { chars.next(); write!(stdout, "\n")?; }
                Some('"') => { chars.next(); write!(stdout, "\"")?; }
                Some('\\') => { chars.next(); write!(stdout, "\\")?; }
                _ => write!(stdout, "\\")?,
            }
        } else {
            write!(stdout, "{c}")?;
        }
    }
    stdout.flush()
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Thinking...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

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

    spinner.finish_and_clear();

    let mut stdout = io::stdout().lock();
    let mut stream = resp.response.into_async_read();
    let mut buf = [0u8; 256];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        stream_unescape(&buf[..n], &mut stdout)?;
    }
    writeln!(stdout)?;

    Ok(())
}
