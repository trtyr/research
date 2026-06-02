use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;

use crate::models::SearchHit;
use crate::mcp::{extract_mcp_text, parse_mcp_sse_json};
use crate::{EmptyText, SearchProvider, short_excerpt, slug};
use crate::utils::retry;

#[derive(Debug, Deserialize)]
struct ExaSearchResponse {
    results: Vec<ExaResult>,
}

#[derive(Debug, Deserialize)]
struct ExaResult {
    url: String,
    title: Option<String>,
    text: Option<String>,
    summary: Option<String>,
    published_date: Option<String>,
    author: Option<String>,
}

pub(crate) fn search(query: &str, num_results: u32, api_key: Option<&str>) -> Result<Vec<SearchHit>> {
    let api_key = api_key
        .context("exa_api_key is required in research config when --search-provider exa is used")?;
    let response = retry(2, || {
        let resp = reqwest::blocking::Client::new()
            .post("https://api.exa.ai/search")
            .header("x-api-key", api_key)
            .json(&json!({
                "query": query,
                "numResults": num_results.max(1),
                "type": "auto",
                "contents": {
                    "text": { "maxCharacters": 12000 },
                    "summary": { "query": format!("Summarize this source for the research query: {query}") }
                }
            }))
            .send()
            .with_context(|| format!("failed to call Exa search API for query: {:.80}", query))?;
        if !resp.status().is_success() {
            bail!(
                "Exa search failed with {}: {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )
        }
        Ok(resp)
    })?;
    Ok(response
        .json::<ExaSearchResponse>()
        .context("failed to parse Exa search response")?
        .results
        .into_iter()
        .map(|result| SearchHit {
            provider: SearchProvider::Exa.to_string(),
            title: result.title.unwrap_or_else(|| result.url.clone()),
            summary: result
                .summary
                .clone()
                .or_else(|| result.text.as_ref().map(|text| short_excerpt(text)))
                .unwrap_or_else(|| "No summary returned by Exa.".to_string()),
            text: result.text.unwrap_or_default(),
            url: result.url,
            published_date: result.published_date,
            author: result.author,
        })
        .collect())
}

pub(crate) fn search_code_context(
    query: &str,
    tokens_num: u32,
    api_key: Option<&str>,
    timeout_secs: u64,
) -> Result<Vec<SearchHit>> {
    let api_key = api_key.context(
        "search.providers.code.api_key or search.providers.exa.api_key is required when code search is used",
    )?;
    let response = retry(2, || {
        let resp = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()?
            .post("https://mcp.exa.ai/mcp")
            .query(&[("tools", "web_fetch_exa,get_code_context_exa")])
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("x-api-key", api_key)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "get_code_context_exa",
                    "arguments": {
                        "query": query,
                        "tokensNum": tokens_num.clamp(1_000, 50_000)
                    }
                }
            }))
            .send()
            .context("failed to call Exa code context MCP")?;
        if !resp.status().is_success() {
            bail!(
                "Exa code context failed with {}: {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )
        }
        Ok(resp)
    })?;
    let text = response.text()?;
    let value = parse_mcp_sse_json(&text).or_else(|_| {
        serde_json::from_str::<serde_json::Value>(&text)
            .with_context(|| "failed to parse Exa code context response")
    })?;
    if let Some(error) = value.get("error") {
        bail!("Exa code context returned error: {error}");
    }
    let context = extract_mcp_text(&value).if_empty("No code context returned by Exa.");
    Ok(vec![SearchHit {
        provider: SearchProvider::Code.to_string(),
        url: format!("exa-code://{}", slug(query)),
        title: format!("Exa code context: {query}"),
        summary: short_excerpt(&context),
        text: context,
        published_date: None,
        author: Some("Exa get_code_context_exa".to_string()),
    }])
}
