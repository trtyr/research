use std::thread;

use anyhow::{Result, bail};

use crate::{SearchHit, dedupe_hits};

pub(crate) struct HybridInputs<'a> {
    pub query: &'a str,
    pub max_sources: u32,
    pub run_code_surface: bool,
    pub exa_api_key: Option<&'a str>,
    pub code_tokens_num: u32,
    pub code_api_key: Option<&'a str>,
    pub zhipu_mcp_url: Option<&'a str>,
    pub zhipu_api_key: Option<&'a str>,
    pub zhipu_tool: Option<&'a str>,
    pub zhipu_content_size: Option<&'a str>,
    pub minimax_api_key: Option<&'a str>,
    pub minimax_api_host: Option<&'a str>,
    pub minimax_mcp_command: Option<&'a str>,
    pub minimax_mcp_args: Option<&'a [String]>,
    pub kimi_api_key: Option<&'a str>,
    pub kimi_model: Option<&'a str>,
    pub mcp_timeout_secs: u64,
}

/// Analyze query to determine which providers are likely useful.
fn analyze_query(query: &str) -> QueryProfile {
    let q = query.to_lowercase();
    let is_code = q.contains("code")
        || q.contains("api")
        || q.contains("sdk")
        || q.contains("library")
        || q.contains("framework")
        || q.contains("rust")
        || q.contains("python")
        || q.contains("javascript")
        || q.contains("typescript")
        || q.contains("golang")
        || q.contains("github")
        || q.contains("crate")
        || q.contains("npm")
        || q.contains("implementation")
        || q.contains("benchmark")
        || q.contains("performance");
    let is_chinese = q.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    QueryProfile { is_code, is_chinese }
}

struct QueryProfile {
    is_code: bool,
    is_chinese: bool,
}

pub(crate) fn search<R>(inputs: HybridInputs<'_>, mut record: R) -> Result<Vec<SearchHit>>
where
    R: FnMut(&str, &Result<Vec<SearchHit>>) -> Result<()>,
{
    let profile = analyze_query(inputs.query);

    // Run providers concurrently using scoped threads
    let results: Vec<(&str, Result<Vec<SearchHit>>)> = thread::scope(|s| {
        let exa_handle = s.spawn(|| {
            crate::providers::exa::search(inputs.query, inputs.max_sources, inputs.exa_api_key)
        });

        let run_code = inputs.run_code_surface || profile.is_code;
        let code_handle = if run_code {
            Some(s.spawn(|| {
                crate::providers::code::search(
                    inputs.query,
                    inputs.code_tokens_num,
                    inputs.code_api_key,
                    inputs.mcp_timeout_secs,
                )
            }))
        } else {
            None
        };

        let zhipu_handle = s.spawn(|| {
            crate::providers::zhipu::search(
                inputs.query,
                inputs.max_sources,
                inputs.zhipu_mcp_url,
                inputs.zhipu_api_key,
                inputs.zhipu_tool,
                inputs.zhipu_content_size,
                inputs.mcp_timeout_secs,
            )
        });

        let minimax_handle = if !profile.is_code || profile.is_chinese {
            Some(s.spawn(|| {
                crate::providers::minimax::search(
                    inputs.query,
                    inputs.max_sources,
                    inputs.minimax_api_key,
                    inputs.minimax_api_host,
                    inputs.minimax_mcp_command,
                    inputs.minimax_mcp_args,
                )
            }))
        } else {
            None
        };

        let kimi_handle = if inputs.kimi_api_key.is_some() && !inputs.kimi_api_key.unwrap_or("").is_empty() {
            Some(s.spawn(|| {
                crate::providers::kimi::search(
                    inputs.query,
                    inputs.kimi_api_key.unwrap(),
                    inputs.kimi_model,
                    inputs.mcp_timeout_secs,
                )
            }))
        } else {
            None
        };

        let mut results: Vec<(&str, Result<Vec<SearchHit>>)> = Vec::new();

        match exa_handle.join() {
            Ok(r) => results.push(("exa", r)),
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<String>() { s.clone() } else if let Some(s) = e.downcast_ref::<&str>() { s.to_string() } else { "unknown panic".to_string() };
                results.push(("exa", Err(anyhow::anyhow!("exa thread panicked: {}", msg))))
            }
        }

        if let Some(handle) = code_handle {
            match handle.join() {
                Ok(r) => results.push(("code", r)),
                Err(e) => {
                    let msg = if let Some(s) = e.downcast_ref::<String>() { s.clone() } else if let Some(s) = e.downcast_ref::<&str>() { s.to_string() } else { "unknown panic".to_string() };
                    results.push(("code", Err(anyhow::anyhow!("code thread panicked: {}", msg))))
                }
            }
        }

        match zhipu_handle.join() {
            Ok(r) => results.push(("zhipu", r)),
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<String>() { s.clone() } else if let Some(s) = e.downcast_ref::<&str>() { s.to_string() } else { "unknown panic".to_string() };
                results.push(("zhipu", Err(anyhow::anyhow!("zhipu thread panicked: {}", msg))))
            }
        }

        if let Some(handle) = minimax_handle {
            match handle.join() {
                Ok(r) => results.push(("minimax", r)),
                Err(e) => {
                    let msg = if let Some(s) = e.downcast_ref::<String>() { s.clone() } else if let Some(s) = e.downcast_ref::<&str>() { s.to_string() } else { "unknown panic".to_string() };
                    results.push(("minimax", Err(anyhow::anyhow!("minimax thread panicked: {}", msg))))
                }
            }
        }

        if let Some(handle) = kimi_handle {
            match handle.join() {
                Ok(r) => results.push(("kimi", r)),
                Err(e) => {
                    let msg = if let Some(s) = e.downcast_ref::<String>() { s.clone() } else if let Some(s) = e.downcast_ref::<&str>() { s.to_string() } else { "unknown panic".to_string() };
                    results.push(("kimi", Err(anyhow::anyhow!("kimi thread panicked: {}", msg))))
                }
            }
        }

        results
    });

    // Record results and collect hits
    let mut errors = Vec::new();
    let mut hits = Vec::new();

    for (provider_name, result) in results {
        record(provider_name, &result)?;
        match result {
            Ok(provider_hits) => hits.extend(provider_hits),
            Err(error) => errors.push(format!("{provider_name}: {error}")),
        }
    }

    let hits = dedupe_hits(hits)
        .into_iter()
        .take(inputs.max_sources as usize)
        .collect::<Vec<_>>();
    if hits.is_empty() && !errors.is_empty() {
        bail!("hybrid search failed: {}", errors.join("; "));
    }
    Ok(hits)
}
