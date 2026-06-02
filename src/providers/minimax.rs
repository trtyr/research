use std::env;
use std::io::Write;
use std::process::{Command as ProcessCommand, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::mcp::collect_minimax_hits;
use crate::{dedupe_hits, SearchHit};

pub(crate) fn search(
    query: &str,
    max_sources: u32,
    api_key: Option<&str>,
    api_host: Option<&str>,
    command: Option<&str>,
    args: Option<&[String]>,
) -> Result<Vec<SearchHit>> {
    let num_results = max_sources.min(10) as usize;
    let api_key = api_key
        .map(str::to_string)
        .or_else(|| env::var("MINIMAX_API_KEY").ok())
        .context(
            "minimax_api_key is required in research config when minimax or hybrid search is used",
        )?;
    let api_host = api_host
        .or(Some("https://api.minimaxi.com"))
        .unwrap()
        .to_string();
    let command = command.unwrap_or("uvx");
    let args = args
        .map(|items| items.to_vec())
        .unwrap_or_else(|| vec!["minimax-coding-plan-mcp".to_string(), "-y".to_string()]);
    let mut child = ProcessCommand::new(command)
        .args(args)
        .env("MINIMAX_API_KEY", api_key)
        .env("MINIMAX_API_HOST", api_host)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start MiniMax MCP command: {command}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .context("failed to open MiniMax MCP stdin")?;
        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "research-cli", "version": env!("CARGO_PKG_VERSION") }
                }
            })
        )
        .context("Failed to write to Minimax MCP subprocess stdin")?;
        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            })
        )
        .context("Failed to write to Minimax MCP subprocess stdin")?;
        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "web_search",
                    "arguments": {
                        "query": query.chars().take(120).collect::<String>()
                    }
                }
            })
        )
        .context("Failed to write to Minimax MCP subprocess stdin")?;
    }
    let output = child
        .wait_with_output()
        .context("failed to wait for MiniMax MCP command")?;
    if !output.status.success() {
        bail!(
            "MiniMax MCP command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("id").and_then(Value::as_i64) == Some(2))
        .context("MiniMax MCP did not return a tools/call response")?;
    if let Some(error) = value.get("error") {
        bail!("MiniMax MCP web_search returned error: {error}");
    }
    let mut hits = Vec::new();
    collect_minimax_hits(&value, query, &mut hits);
    if hits.is_empty() {
        bail!("MiniMax MCP web_search returned no structured search hits: {stdout}");
    }
    Ok(dedupe_hits(hits).into_iter().take(num_results).collect())
}
