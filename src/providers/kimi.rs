use anyhow::{Context, Result, bail};
use serde_json::json;
use std::time::Duration;

use crate::SearchHit;

/// Kimi (Moonshot AI) web search provider.
///
/// Uses Kimi's built-in `$web_search` function via the OpenAI-compatible
/// chat completions API. Kimi performs the actual web search on its backend
/// and returns an AI-synthesized answer with sources.
///
/// API: POST https://api.moonshot.cn/v1/chat/completions
/// Docs: https://platform.kimi.com/docs/guide/use-web-search
pub(crate) fn search(
    query: &str,
    api_key: &str,
    model: Option<&str>,
    timeout_secs: u64,
) -> Result<Vec<SearchHit>> {
    let model = model.unwrap_or("kimi-k2.6");
    // Kimi web search can take up to 90 seconds due to search + synthesis
    let effective_timeout = timeout_secs.max(120);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(effective_timeout))
        .build()?;

    let mut messages: Vec<serde_json::Value> = vec![
        json!({"role": "user", "content": query}),
    ];

    let tools = vec![json!({
        "type": "builtin_function",
        "function": {
            "name": "$web_search",
        },
    })];

    // Loop: Kimi may return tool_calls multiple times before final answer
    let mut iterations = 0;
    let max_iterations = 5; // safety limit

    loop {
        iterations += 1;
        if iterations > max_iterations {
            bail!("Kimi search exceeded {} tool-call iterations", max_iterations);
        }

        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.6,
            "max_tokens": 8000,
            "tools": tools,
            "thinking": {"type": "disabled"},
        });

        let resp = client
            .post("https://api.moonshot.cn/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("failed to call Kimi API")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            bail!("Kimi API error {}: {}", status, &text[..text.len().min(200)]);
        }

        let parsed: serde_json::Value = resp
            .json()
            .context("failed to parse Kimi API response")?;

        let choice = parsed["choices"]
            .get(0)
            .context("Kimi response has no choices")?;

        let finish_reason = choice["finish_reason"]
            .as_str()
            .unwrap_or("stop");

        if finish_reason == "tool_calls" {
            // Kimi wants to execute $web_search — just pass arguments back
            let assistant_msg = choice["message"].clone();
            messages.push(assistant_msg.clone());

            if let Some(tool_calls) = assistant_msg["tool_calls"].as_array() {
                for tool_call in tool_calls {
                    let call_id = tool_call["id"].as_str().unwrap_or("");
                    let call_name = tool_call["function"]["name"].as_str().unwrap_or("");
                    let call_args = tool_call["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}");

                    // For $web_search, pass arguments back as-is
                    // Kimi executes the search internally
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "name": call_name,
                        "content": call_args,
                    }));
                }
            }
            continue;
        }

        // finish_reason == "stop" — final answer
        let content = choice["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            bail!(
                "Kimi returned empty response. This may indicate the query triggered content filtering or the model hit a token limit. Query: {:.80}",
                query
            );
        }

        // Extract sources from the response if available
        let mut hits = Vec::new();

        // Parse the Kimi response for URLs and content
        let urls = extract_urls_from_text(&content);

        if urls.is_empty() {
            // No specific URLs found — create a single hit from the answer
            hits.push(SearchHit {
                title: format!("Kimi search: {}", query),
                url: format!("kimi://{}", query.replace(' ', "-")),
                summary: content.chars().take(500).collect(),
                text: content,
                provider: "kimi".to_string(),
                published_date: None,
                author: None,
            });
        } else {
            // Create hits from discovered URLs
            for (i, url) in urls.iter().take(5).enumerate() {
                hits.push(SearchHit {
                    title: format!("Kimi result {}: {}", i + 1, query),
                    url: url.clone(),
                    summary: content.chars().take(500).collect(),
                    text: content.clone(),
                    provider: "kimi".to_string(),
                    published_date: None,
                    author: None,
                });
            }
        }

        return Ok(hits);
    }
}

/// Extract URLs from Kimi's response text
fn extract_urls_from_text(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for word in text.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']' || c == '"' || c == '\'' || c == ',' || c == '.' || c == ':');
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            urls.push(trimmed.to_string());
        }
    }
    urls
}
