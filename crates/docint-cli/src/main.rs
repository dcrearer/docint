mod auth;

// Include build-time information (git commit, build date)
mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

use anyhow::{Context, Result};
use aws_sdk_bedrockagentcore::primitives::Blob;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::io::{self, BufRead, Write};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

/// Returns the long version string with git commit and build date
fn long_version() -> &'static str {
    let date = built_info::BUILT_TIME_UTC
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    Box::leak(
        format!(
            "{} (commit: {}, built: {})",
            built_info::PKG_VERSION,
            built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("unknown"),
            date
        )
        .into_boxed_str(),
    )
}

#[derive(Parser)]
#[command(
    name = "docint",
    about = "Document Intelligence agent",
    version,
    long_version = long_version(),
    long_about = "Interactive CLI for querying documents using Claude on AWS Bedrock with conversational memory powered by Amazon Bedrock AgentCore"
)]
struct Cli {
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

fn format_tool_call(xml: &str) -> Option<String> {
    // Extract all <invoke> blocks
    let mut tool_calls = Vec::new();
    let mut search_pos = 0;

    while let Some(invoke_start) = xml[search_pos..].find(r#"<invoke name=""#) {
        let abs_invoke_start = search_pos + invoke_start;
        let name_start = abs_invoke_start + r#"<invoke name=""#.len();
        let name_end = xml[name_start..].find('"')?;
        let tool_name = &xml[name_start..name_start + name_end];

        // Find the end of this invoke block
        let invoke_end = xml[abs_invoke_start..].find("</invoke>")?;
        let invoke_block = &xml[abs_invoke_start..abs_invoke_start + invoke_end];

        // Extract parameters within this invoke block
        let mut params = Vec::new();
        let mut param_pos = 0;
        while let Some(param_start) = invoke_block[param_pos..].find(r#"<parameter name=""#) {
            let abs_param_start = param_pos + param_start + r#"<parameter name=""#.len();
            let param_name_end = invoke_block[abs_param_start..].find('"')?;
            let param_name = &invoke_block[abs_param_start..abs_param_start + param_name_end];

            let value_start = abs_param_start + param_name_end + 2; // Skip ">
            let value_end = invoke_block[value_start..].find("</parameter>")?;
            let param_value = invoke_block[value_start..value_start + value_end].trim();

            params.push(format!("{param_name}={param_value}"));
            param_pos = value_start + value_end + 12; // Skip past </parameter>
        }

        let params_str = if params.is_empty() {
            String::new()
        } else {
            format!(" ({})", params.join(", "))
        };

        tool_calls.push(format!("{tool_name}{params_str}"));
        search_pos = abs_invoke_start + invoke_end + 9; // Skip past </invoke>
    }

    if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls.join(", "))
    }
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

    let mut in_tool_call = false;
    let mut tool_call_buffer = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        if let Some(mut text) = extract_sse_text(line.trim()) {
            // Detect start of tool call
            if text.contains("<function_calls>") || text.contains("<invoke") {
                in_tool_call = true;
                tool_call_buffer.clear();
            }

            // Buffer tool call XML
            if in_tool_call {
                tool_call_buffer.push_str(&text);

                // Check if tool call is complete
                if let Some(end_pos) = tool_call_buffer.find("</function_calls>") {
                    // Extract just the tool call XML
                    let tool_xml = &tool_call_buffer[..end_pos + 17]; // 17 = len("</function_calls>")

                    // Parse and display in friendly format
                    if let Some(formatted) = format_tool_call(tool_xml) {
                        if first_byte {
                            ttfb = t2.elapsed();
                            first_byte = false;
                        }
                        write!(stdout, "\n🔧 {formatted}\n")?;
                        stdout.flush()?;
                    }

                    // Continue with any text after the tool call
                    text = tool_call_buffer[end_pos + 17..].to_string();
                    in_tool_call = false;
                    tool_call_buffer.clear();

                    // If there's remaining text, fall through to display it
                    if text.is_empty() {
                        continue;
                    }
                } else {
                    // Tool call not complete yet, keep buffering
                    continue;
                }
            }

            // Filter out any XML fragments that leaked through
            let trimmed = text.trim();
            if trimmed.starts_with('<') && (
                trimmed.starts_with("<function") ||
                trimmed.starts_with("<invoke") ||
                trimmed.starts_with("<parameter") ||
                trimmed.starts_with("</function") ||
                trimmed.starts_with("</invoke") ||
                trimmed.starts_with("</parameter")
            ) {
                continue;
            }

            // Regular text output
            if first_byte {
                ttfb = t2.elapsed();
                first_byte = false;
                write!(stdout, "\n🤖 ")?;
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
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let agent_config = aws_sdk_bedrockagentcore::config::Builder::from(&config)
        .stalled_stream_protection(
            aws_sdk_bedrockagentcore::config::StalledStreamProtectionConfig::disabled(),
        )
        .build();
    let agent_client = aws_sdk_bedrockagentcore::Client::from_conf(agent_config);
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
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tool_call_with_single_parameter() {
        let xml = r#"<function_calls><invoke name="get-document-metadata"><parameter name="list_all">true</parameter></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, Some("get-document-metadata (list_all=true)".to_string()));
    }

    #[test]
    fn test_format_tool_call_with_multiple_parameters() {
        let xml = r#"<function_calls><invoke name="search-documents"><parameter name="query">rust lifetime</parameter><parameter name="limit">10</parameter></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, Some("search-documents (query=rust lifetime, limit=10)".to_string()));
    }

    #[test]
    fn test_format_tool_call_with_no_parameters() {
        let xml = r#"<function_calls><invoke name="get-document-metadata"></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, Some("get-document-metadata".to_string()));
    }

    #[test]
    fn test_format_tool_call_with_multiline_parameters() {
        let xml = r#"<function_calls><invoke name="compare-documents"><parameter name="query">memory safety</parameter><parameter name="document_id_a">doc1</parameter><parameter name="document_id_b">doc2</parameter></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, Some("compare-documents (query=memory safety, document_id_a=doc1, document_id_b=doc2)".to_string()));
    }

    #[test]
    fn test_format_tool_call_with_whitespace_in_values() {
        let xml = r#"<function_calls><invoke name="search-documents"><parameter name="query">  rust atomics  </parameter></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, Some("search-documents (query=rust atomics)".to_string()));
    }

    #[test]
    fn test_format_tool_call_malformed_missing_invoke_name() {
        let xml = r#"<function_calls><invoke><parameter name="query">test</parameter></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, None);
    }

    #[test]
    fn test_format_tool_call_incomplete_xml() {
        let xml = r#"<function_calls><invoke name="search-documents"><parameter name="query">test"#;
        let result = format_tool_call(xml);
        assert_eq!(result, None);
    }

    #[test]
    fn test_format_tool_call_multiple_invokes() {
        let xml = r#"<function_calls><invoke name="search-documents"><parameter name="query">rust data structures</parameter></invoke><invoke name="search-documents"><parameter name="query">rust enum struct</parameter></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, Some("search-documents (query=rust data structures), search-documents (query=rust enum struct)".to_string()));
    }

    #[test]
    fn test_format_tool_call_multiple_invokes_different_tools() {
        let xml = r#"<function_calls><invoke name="get-document-metadata"><parameter name="limit">5</parameter></invoke><invoke name="search-documents"><parameter name="query">test</parameter></invoke></function_calls>"#;
        let result = format_tool_call(xml);
        assert_eq!(result, Some("get-document-metadata (limit=5), search-documents (query=test)".to_string()));
    }

    #[test]
    fn test_extract_sse_text_valid() {
        let line = r#"data: "Hello, world!""#;
        let result = extract_sse_text(line);
        assert_eq!(result, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_extract_sse_text_with_escaped_quotes() {
        let line = r#"data: "She said \"hello\"""#;
        let result = extract_sse_text(line);
        assert_eq!(result, Some(r#"She said "hello""#.to_string()));
    }

    #[test]
    fn test_extract_sse_text_no_data_prefix() {
        let line = r#""Hello, world!""#;
        let result = extract_sse_text(line);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_sse_text_invalid_json() {
        let line = r#"data: not-json"#;
        let result = extract_sse_text(line);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_sse_text_empty() {
        let line = r#"data: """#;
        let result = extract_sse_text(line);
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn test_format_tool_call_with_newlines_between_tags() {
        // Simulates streaming where tags arrive in separate chunks
        let xml = "<function_calls>\n<invoke name=\"search-documents\">\n<parameter name=\"query\">test</parameter>\n</invoke>\n</function_calls>";
        let result = format_tool_call(xml);
        assert_eq!(result, Some("search-documents (query=test)".to_string()));
    }

    #[test]
    fn test_cli_has_version() {
        // Verify version info is available (clap reads from Cargo.toml)
        use clap::CommandFactory;
        let app = Cli::command();
        let version = app.get_version().expect("CLI should have version");
        assert!(!version.is_empty(), "Version should not be empty");
        assert!(version.contains('.'), "Version should be semver format");
    }
}
