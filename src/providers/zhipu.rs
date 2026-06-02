use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::json;

use crate::mcp::{collect_zhipu_hits, parse_mcp_sse_json, zhipu_tool_arguments};
use crate::{dedupe_hits, SearchHit};
use crate::utils::retry;

pub(crate) fn search(
    query: &str,
    max_sources: u32,
    mcp_url: Option<&str>,
    api_key: Option<&str>,
    tool: Option<&str>,
    content_size: Option<&str>,
    timeout_secs: u64,
) -> Result<Vec<SearchHit>> {
    let mcp_url = mcp_url.context(
        "zhipu_mcp_url is required in research config when zhipu or hybrid search is used",
    )?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()?;
    let num_results = max_sources.min(10) as usize;
    let initialize = zhipu_request(client.post(mcp_url), api_key)
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "research-cli", "version": env!("CARGO_PKG_VERSION") }
            }
        }))
        .send()
        .context("failed to initialize Zhipu MCP web search")?;
    if !initialize.status().is_success() {
        bail!(
            "Zhipu MCP initialize failed with {}: {}",
            initialize.status(),
            initialize.text().unwrap_or_default()
        )
    }
    let session_id = initialize
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .context("Zhipu MCP initialize response did not include mcp-session-id")?
        .to_string();
    let _ = parse_mcp_sse_json(&initialize.text()?)
        .context("Failed to parse Zhipu MCP SSE response during initialization")?;
    let tool = tool.unwrap_or("web_search_prime");
    let response = retry(2, || {
        let resp = zhipu_request(client.post(mcp_url), api_key)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("Mcp-Session-Id", &session_id)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": tool,
                    "arguments": zhipu_tool_arguments(
                        tool,
                        query,
                        num_results,
                        content_size.unwrap_or("medium")
                    )
                }
            }))
            .send()
            .context("failed to call Zhipu MCP web search tool")?;
        if !resp.status().is_success() {
            bail!(
                "Zhipu MCP search failed with {}: {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )
        }
        Ok(resp)
    })?;
    let text = response.text()?;
    let value = parse_mcp_sse_json(&text).or_else(|_| {
        serde_json::from_str::<serde_json::Value>(&text).with_context(|| "failed to parse Zhipu MCP response")
    })?;
    if let Some(error) = value.get("error") {
        bail!("Zhipu MCP search returned error: {error}");
    }
    let mut hits = Vec::new();
    collect_zhipu_hits(&value, query, &mut hits);
    if hits.is_empty() {
        bail!("Zhipu MCP search returned no structured search hits: {text}");
    }
    Ok(dedupe_hits(hits).into_iter().take(num_results).collect())
}

fn zhipu_request(
    builder: reqwest::blocking::RequestBuilder,
    api_key: Option<&str>,
) -> reqwest::blocking::RequestBuilder {
    let builder = builder
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json");
    if let Some(api_key) = api_key {
        builder.header("Authorization", format!("Bearer {api_key}"))
    } else {
        builder
    }
}
