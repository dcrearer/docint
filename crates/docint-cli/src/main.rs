mod auth;

use anyhow::{Context, Result};
use aws_sdk_bedrockagentcore::primitives::Blob;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::io::{self, BufRead, Write};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "docint", about = "Document Intelligence agent")]
struct Cli {
    /// Interactive chat mode
    #[arg(long)]
    chat: bool,

    /// Agent runtime ARN
    #[arg(long, env("DOCINT_RUNTIME_ARN"))]
    runtime_arn: String,

    /// Cognito App Client ID
    #[arg(long, env("DOCINT_CLIENT_ID"))]
    client_id: String,

    /// Endpoint qualifier
    #[arg(long, env("DOCINT_ENDPOINT"), default_value = "docint_agent_endpoint")]
    endpoint: String,

    /// Show timing breakdown
    #[arg(long)]
    timing: bool,
}

#[derive(Serialize)]
struct Request {
    prompt: String,
    tenant_id: String,
    actor_id: String,
    session_id: String,
}

fn extract_sse_text(line: &str) -> Option<String> {
    let payload = line.strip_prefix("data: ")?;
    serde_json::from_str::<String>(payload).ok()
}

async fn send_query(
    client: &aws_sdk_bedrockagentcore::Client,
    runtime_arn: &str,
    endpoint: &str,
    request: &Request,
    timing: bool,
) -> Result<()> {
    let total_start = Instant::now();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Thinking...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let payload = serde_json::to_vec(request)?;

    let t1 = Instant::now();
    let resp = client
        .invoke_agent_runtime()
        .agent_runtime_arn(runtime_arn)
        .qualifier(endpoint)
        .payload(Blob::new(payload))
        .send()
        .await
        .context("Failed to invoke agent")?;
    let agent_latency = t1.elapsed();

    spinner.finish_and_clear();

    let t2 = Instant::now();
    let mut stdout = io::stdout().lock();
    let mut reader = BufReader::new(resp.response.into_async_read());
    let mut line = String::new();
    let mut first_byte = true;
    let mut ttfb = std::time::Duration::ZERO;

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        if let Some(text) = extract_sse_text(line.trim()) {
            if first_byte {
                ttfb = t2.elapsed();
                first_byte = false;
                write!(stdout, "🤖 ")?;
            }
            write!(stdout, "{text}")?;
            stdout.flush()?;
        }
    }
    let stream_time = t2.elapsed();
    writeln!(stdout)?;

    if timing {
        let total = total_start.elapsed();
        eprintln!();
        eprintln!("--- Timing ---");
        eprintln!(
            "  Agent response: {:>7.1}ms",
            agent_latency.as_secs_f64() * 1000.0
        );
        eprintln!("  Stream TTFB:    {:>7.1}ms", ttfb.as_secs_f64() * 1000.0);
        eprintln!(
            "  Stream total:   {:>7.1}ms",
            stream_time.as_secs_f64() * 1000.0
        );
        eprintln!("  Total:          {:>7.1}ms", total.as_secs_f64() * 1000.0);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let agent_client = aws_sdk_bedrockagentcore::Client::new(&config);
    let cognito_client = aws_sdk_cognitoidentityprovider::Client::new(&config);

    // --- Authentication ---
    eprintln!("Welcome to docint\n");

    let session = match auth::try_restore_session(&cognito_client, &cli.client_id).await {
        Some(s) => {
            eprintln!("✓ Logged in as {} (tenant: {})\n", s.username, s.tenant_id);
            s
        }
        None => {
            let choices = &["Login", "Sign up", "Quit"];
            let selection = dialoguer::Select::new()
                .items(choices)
                .default(0)
                .interact()?;

            let s = match selection {
                0 => auth::login(&cognito_client, &cli.client_id).await?,
                1 => auth::signup(&cognito_client, &cli.client_id).await?,
                _ => return Ok(()),
            };
            eprintln!("✓ Logged in as {} (tenant: {})\n", s.username, s.tenant_id);
            s
        }
    };

    // --- Chat loop ---
    let session_id = Uuid::new_v4().to_string();
    eprintln!("docint chat (session {session_id}) — type 'quit' to exit\n");

    let stdin = io::stdin().lock();
    let mut lines = stdin.lines();
    loop {
        eprint!("🏗️  ");
        io::stderr().flush()?;
        let Some(Ok(line)) = lines.next() else { break };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == "exit" {
            break;
        }
        if line == "logout" {
            auth::logout()?;
            eprintln!("✓ Logged out");
            break;
        }
        let request = Request {
            prompt: line,
            tenant_id: session.tenant_id.clone(),
            actor_id: session.tenant_id.clone(),
            session_id: session_id.clone(),
        };
        if let Err(e) = send_query(
            &agent_client,
            &cli.runtime_arn,
            &cli.endpoint,
            &request,
            cli.timing,
        )
        .await
        {
            eprintln!("Error: {e}");
        }
        eprintln!();
    }

    Ok(())
}
