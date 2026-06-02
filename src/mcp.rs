use anyhow::Result;
use serde_json::{Value, json};

use crate::SearchHit;
use crate::utils::slug;

pub(crate) fn zhipu_tool_arguments(
    tool: &str,
    query: &str,
    num_results: usize,
    content_size: &str,
) -> Value {
    match tool {
        "web-search-pro" | "web-search" => json!({
            "search_query": query,
            "count": num_results,
            "contentSize": content_size,
        }),
        _ => json!({
            "search_query": query,
            "count": num_results,
        }),
    }
}

pub(crate) fn parse_mcp_sse_json(text: &str) -> Result<Value> {
    let payload = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(serde_json::from_str(&payload)?)
}

pub(crate) fn extract_mcp_text(value: &Value) -> String {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = value.pointer("/result/text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(items) = value
        .pointer("/result/content")
        .and_then(Value::as_array)
        .or_else(|| value.get("content").and_then(Value::as_array))
    {
        let parts = items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if !parts.is_empty() {
            return parts.join("\n\n");
        }
    }
    String::new()
}

pub(crate) fn collect_zhipu_hits(value: &Value, query: &str, hits: &mut Vec<SearchHit>) {
    // Zhipu MCP returns: result.content[0].text = JSON string containing JSON array
    // Need to parse twice: first get the string, then parse the string as array
    let parsed_array: Option<Vec<Value>> = value
        .pointer("/result/content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|first| first.get("text").and_then(Value::as_str))
        .and_then(|text| {
            // Try direct parse first
            serde_json::from_str(text).ok().or_else(|| {
                // Try parsing as JSON string containing JSON
                serde_json::from_str::<String>(text)
                    .ok()
                    .and_then(|inner| serde_json::from_str(&inner).ok())
            })
        });

    let array: Option<&Vec<Value>> = if let Some(ref arr) = parsed_array {
        Some(arr)
    } else {
        value
            .pointer("/result/results")
            .and_then(Value::as_array)
            .or_else(|| value.pointer("/results").and_then(Value::as_array))
    };

    if let Some(items) = array {
        for item in items {
            if let Some(url) = item
                .get("link")
                .and_then(Value::as_str)
                .or_else(|| item.get("url").and_then(Value::as_str))
            {
                let title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(query)
                    .to_string();
                let summary = item
                    .get("content")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("snippet").and_then(Value::as_str))
                    .unwrap_or_default()
                    .to_string();
                hits.push(SearchHit {
                    provider: "zhipu".to_string(),
                    url: url.to_string(),
                    title,
                    summary: summary.clone(),
                    text: summary,
                    published_date: None,
                    author: None,
                });
            }
        }
    }
    if hits.is_empty() {
        let text = extract_mcp_text(value);
        if !text.is_empty() {
            hits.push(SearchHit {
                provider: "zhipu".to_string(),
                url: format!("zhipu://{}", slug(query)),
                title: format!("Zhipu web search: {query}"),
                summary: text.clone(),
                text,
                published_date: None,
                author: None,
            });
        }
    }
}

pub(crate) fn collect_minimax_hits(value: &Value, query: &str, hits: &mut Vec<SearchHit>) {
    // Try structuredContent.text first (new minimax MCP format)
    let organic = value
        .get("result")
        .and_then(|r| r.get("structuredContent"))
        .and_then(|sc| sc.get("text"))
        .and_then(|t| t.as_str())
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|v| v.get("organic").cloned())
        .or_else(|| {
            // Fallback: result.content[].text
            value
                .get("result")
                .and_then(|r| r.get("content"))
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("text"))
                .and_then(|t| t.as_str())
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .and_then(|v| v.get("organic").cloned())
        });
    let array = match organic.and_then(|v| v.as_array().cloned()) {
        Some(a) => a,
        None => {
            // Legacy: value itself is an array
            if let Some(arr) = value.as_array() {
                collect_from_array(arr, query, hits);
            }
            return;
        }
    };
    collect_from_array(&array, query, hits);
}

fn collect_from_array(array: &[Value], query: &str, hits: &mut Vec<SearchHit>) {
    for item in array {
        let url = item
            .get("url")
            .or_else(|| item.get("link"))
            .and_then(Value::as_str);
        if let Some(url) = url {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(query)
                .to_string();
            let summary = item
                .get("snippet")
                .or_else(|| item.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            hits.push(SearchHit {
                provider: "minimax".to_string(),
                url: url.to_string(),
                title,
                summary: summary.clone(),
                text: summary,
                published_date: item
                    .get("date")
                    .and_then(Value::as_str)
                    .map(String::from),
                author: None,
            });
        }
    }
}
