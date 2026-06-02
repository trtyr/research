mod config;
mod graph_utils;
mod lab;
mod local_search;
mod mcp;
mod models;
mod providers;
mod rendering;
mod storage;
mod types;
mod utils;
mod viewer;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(not(test))]
use std::net::TcpListener;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::Duration,
};
use crate::config::{ResolvedConfig, default_project_root, expand_tilde, unique_paths};
use crate::lab::Lab;
use crate::local_search::normalize_url_key;
use crate::storage::{Storage, artifact_entry, is_optional_jsonl_target, read_dir_names, read_target_name, read_to_string, write_file};
use crate::types::{SearchProfile, StartInput};
use crate::utils::{Bm25, Bm25Corpus, clean_text, cosine_similarity, embed_single, embed_texts, fetch_pdf_text, id, is_pdf_url, normalize, now, rerank, slug, tokenize, token_overlap};
use crate::models::{
    AgentRole, AgentRunRecord, CandidateSourceRecord, ClaimRecord, GateDecision, GraphData,
    GraphEdge, GraphNode, LeadRecord, RankedSearchHit, SearchAttemptRecord, SearchHit,
    SourceChunkRecord, SourceRecord, State,
};
pub use crate::config::{
    AiConfig, CodeConfig, EmbeddingConfig, ExaConfig, LocalProjectConfig, MinimaxConfig,
    ProfileBudgetConfig, ProfileConfig, ResearchConfig, RerankerConfig, SearchConfig, SearchProvidersConfig,
    ServerConfig, StorageConfig, TimeoutConfig, ZhipuConfig,
};
pub use crate::models::LinkRecord;

const DEFAULT_CONFIG_RELATIVE_PATH: &str = "research/config.json";

#[derive(Parser, Debug)]
#[command(name = "research", about = "Persistent research workspace CLI")]
pub struct Cli {
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    #[arg(long, global = true)]
    pub search_provider: Option<SearchProvider>,
    #[arg(long, global = true)]
    pub exa_api_key: Option<String>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Start {
        #[arg(long)]
        topic: String,
        #[arg(long)]
        topic_id: Option<String>,
        #[arg(long)]
        quality_hint: Option<QualityHint>,
        #[arg(long)]
        max_rounds: Option<u32>,
        #[arg(long)]
        max_sources: Option<u32>,
        #[arg(long)]
        max_runtime_minutes: Option<u32>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        level: Option<SearchLevel>,
        #[arg(long)]
        initial_direction: Vec<String>,
        #[arg(long, value_name = "PATH")]
        local_project: Vec<PathBuf>,
        #[arg(long)]
        resume_existing: bool,
    },
    Status {
        #[arg(long)]
        topic_id: Option<String>,
    },
    Read {
        #[arg(long)]
        topic_id: String,
        #[arg(long)]
        target: ReadTarget,
        #[arg(long)]
        note_id: Option<String>,
        #[arg(long, default_value_t = 1)]
        offset: usize,
        #[arg(long, default_value_t = 20_000)]
        limit: usize,
    },
    AddDirection {
        #[arg(long)]
        topic_id: String,
        #[arg(long)]
        direction: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long, default_value_t = Priority::Normal)]
        priority: Priority,
    },
    Search {
        #[arg(long)]
        topic_id: String,
        #[arg(long)]
        query: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Pause {
        #[arg(long)]
        topic_id: String,
    },
    Resume {
        #[arg(long)]
        topic_id: String,
        #[arg(long)]
        focus: Option<String>,
        #[arg(long, value_name = "PATH")]
        local_project: Vec<PathBuf>,
        #[arg(long)]
        model: Option<String>,
    },
    Stop {
        #[arg(long)]
        topic_id: String,
    },
    FindLinks {
        #[arg(long)]
        topic_id: String,
    },
    Ask {
        #[arg(long)]
        topic_id: String,
        #[arg(long)]
        question: String,
    },
    Refine {
        #[arg(long)]
        topic_id: String,
    },
    Synthesize {
        #[arg(long)]
        topic_id: String,
    },
    Export {
        #[arg(long)]
        topic_id: String,
        #[arg(long)]
        format: ExportFormat,
    },
    Serve {
        #[arg(long)]
        topic_id: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    Path,
    Show,
    Set { key: String, value: String },
    Unset { key: String },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QualityHint {
    Academic,
    #[default]
    General,
    Broad,
}

impl std::fmt::Display for QualityHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            QualityHint::Academic => "academic",
            QualityHint::General => "general",
            QualityHint::Broad => "broad",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchProvider {
    #[default]
    Deterministic,
    Exa,
    Code,
    Zhipu,
    Minimax,
    Kimi,
    Hybrid,
}

impl std::fmt::Display for SearchProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SearchProvider::Deterministic => "deterministic",
            SearchProvider::Exa => "exa",
            SearchProvider::Code => "code",
            SearchProvider::Zhipu => "zhipu",
            SearchProvider::Minimax => "minimax",
            SearchProvider::Kimi => "kimi",
            SearchProvider::Hybrid => "hybrid",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchLevel {
    Quick,
    #[default]
    Deep,
    Research,
}

impl std::fmt::Display for SearchLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SearchLevel::Quick => "quick",
            SearchLevel::Deep => "deep",
            SearchLevel::Research => "research",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
            Priority::High => "high",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ReadTarget {
    Agenda,
    Questions,
    Sources,
    Chunks,
    CandidateSources,
    AcceptedSources,
    RejectedSources,
    SearchAttempts,
    AgentRuns,
    Notes,
    Claims,
    Evidence,
    Entities,
    Links,
    Insights,
    Leads,
    Timeline,
    Gaps,
    Evaluations,
    Decisions,
    Threads,
    Report,
    ReportReview,
    Refine,
    ResumeSummary,
    ClaimEvents,
    Answers,
    Events,
    State,
    Plan,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Report,
    Outline,
    KnowledgeBase,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Paused,
    Stopped,
    Completed,
    Error,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Status::Running => "running",
            Status::Paused => "paused",
            Status::Stopped => "stopped",
            Status::Completed => "completed",
            Status::Error => "error",
        })
    }
}

pub fn run(cli: Cli) -> Result<Value> {
    let config = ResolvedConfig::load(cli.config.clone())?;
    if let Command::Config { command } = cli.command {
        return config.run(command);
    }
    let root = cli
        .root
        .or_else(|| config.values.root.clone())
        .unwrap_or_else(default_project_root);
    let search_provider = cli
        .search_provider
        .or(config.values.search_provider)
        .unwrap_or_default();
    let lab = Lab::new(
        root,
        search_provider,
        cli.exa_api_key
            .or_else(|| config.values.exa_api_key.clone()),
        config.values,
    );
    match cli.command {
        Command::Start {
            topic,
            topic_id,
            quality_hint,
            max_rounds,
            max_sources,
            max_runtime_minutes,
            model,
            level,
            initial_direction,
            local_project,
            resume_existing,
        } => lab.start(StartInput {
            topic,
            topic_id,
            quality_hint,
            max_rounds,
            max_sources,
            max_runtime_minutes,
            model,
            level,
            initial_directions: initial_direction,
            local_project_paths: local_project,
            resume_existing,
        }),
        Command::Status { topic_id } => lab.status(topic_id),
        Command::Read {
            topic_id,
            target,
            note_id,
            offset,
            limit,
        } => lab.read(&topic_id, target, note_id, offset, limit),
        Command::AddDirection {
            topic_id,
            direction,
            reason,
            priority,
        } => lab.add_direction(&topic_id, direction, reason, priority),
        Command::Search {
            topic_id,
            query,
            reason,
        } => lab.search_query(&topic_id, query, reason),
        Command::Pause { topic_id } => lab.set_status(&topic_id, Status::Paused),
        Command::Stop { topic_id } => lab.set_status(&topic_id, Status::Stopped),
        Command::Resume {
            topic_id,
            focus,
            local_project,
            model,
        } => lab.resume(&topic_id, focus, local_project, model),
        Command::FindLinks { topic_id } => lab.find_links(&topic_id),
        Command::Ask { topic_id, question } => lab.ask(&topic_id, question),
        Command::Refine { topic_id } => lab.refine(&topic_id),
        Command::Synthesize { topic_id } => lab.synthesize(&topic_id),
        Command::Export { topic_id, format } => lab.export(&topic_id, format),
        Command::Serve {
            topic_id,
            host,
            port,
        } => lab.serve(&topic_id, host, port),
        Command::Config { .. } => unreachable!("config command is handled before lab setup"),
    }
}

pub(crate) trait EmptyText {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyText for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.trim().is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

fn follow_up_leads(
    state: &State,
    query: &str,
    source: &SourceRecord,
    chunks: &[SourceChunkRecord],
) -> Vec<LeadRecord> {
    let strongest_excerpt = chunks
        .iter()
        .max_by(|left, right| left.relevance_score.total_cmp(&right.relevance_score))
        .map(|chunk| short_excerpt(&chunk.text))
        .unwrap_or_else(|| source.summary.clone());
    vec![
        LeadRecord::new(
            format!(
                "Find independent confirmation for {}",
                short(&source.title, 96)
            ),
            Some(format!("Avoid relying on one source for: {query}")),
            Priority::Normal,
        ),
        LeadRecord::new(
            format!(
                "Look for counter-evidence or limitations for {}",
                short(&state.topic, 96)
            ),
            Some(format!(
                "Current strongest excerpt to challenge: {}",
                short(&strongest_excerpt, 140)
            )),
            Priority::Normal,
        ),
        LeadRecord::new(
            format!(
                "Trace the original source behind {}",
                short(&source.title, 96)
            ),
            Some("Prefer primary sources over summaries or aggregators.".to_string()),
            Priority::Low,
        ),
    ]
}

#[cfg(not(test))]
fn reserve_viewer_port(host: &str) -> Result<u16> {
    Ok(TcpListener::bind(format!("{host}:0"))?.local_addr()?.port())
}

#[cfg(not(test))]
fn viewer_responds(url: &str, timeout_ms: u64) -> bool {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .and_then(|client| {
            client
                .get(format!("{}/api/state", url.trim_end_matches('/')))
                .send()
        })
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct ModelMessageResponse {
    #[serde(default)]
    content: Option<Vec<ModelContentBlock>>,
    #[serde(default)]
    choices: Option<Vec<ModelChoice>>,
}

#[derive(Debug, Deserialize)]
struct ModelChoice {
    message: ModelChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ModelChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ModelContentBlock {
    Text { text: String },
    Other,
}

pub(crate) fn extract_model_text(response: ModelMessageResponse) -> String {
    // OpenAI format: choices[0].message.content
    if let Some(choices) = response.choices {
        if let Some(choice) = choices.first() {
            if let Some(content) = &choice.message.content {
                if !content.is_empty() {
                    return content.clone();
                }
            }
        }
    }
    // Anthropic format: content[].text
    if let Some(content) = response.content {
        return content
            .into_iter()
            .filter_map(|block| match block {
                ModelContentBlock::Text { text } => Some(text),
                ModelContentBlock::Other => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    "The configured research model returned no text content.".to_string()
}

pub(crate) fn dedupe_hits(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut seen = BTreeSet::new();
    hits.into_iter()
        .filter(|hit| seen.insert(normalize_url_key(&hit.url)))
        .collect()
}

pub(crate) fn interleave_hits(left: Vec<SearchHit>, right: Vec<SearchHit>) -> Vec<SearchHit> {
    let max_len = left.len().max(right.len());
    dedupe_hits(
        (0..max_len)
            .flat_map(|index| {
                [left.get(index).cloned(), right.get(index).cloned()]
                    .into_iter()
                    .flatten()
            })
            .collect(),
    )
}

/// Normalize URL for deduplication: strip tracking params, normalize trailing slash.
fn normalize_url_for_dedup(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    // Remove common tracking parameters
    if let Some(query_start) = url.find('?') {
        let base = &url[..query_start];
        let query = &url[query_start + 1..];
        let params: Vec<&str> = query
            .split('&')
            .filter(|p| {
                let key = p.split('=').next().unwrap_or("");
                !matches!(
                    key,
                    "utm_source" | "utm_medium" | "utm_campaign" | "utm_content" | "utm_term"
                        | "fbclid" | "gclid" | "ref" | "source" | "from"
                )
            })
            .collect();
        if params.is_empty() {
            base.to_string()
        } else {
            format!("{}?{}", base, params.join("&"))
        }
    } else {
        url.to_string()
    }
}

fn rerank_hits(state: &State, query: &str, hits: Vec<SearchHit>, embedding_config: &EmbeddingConfig, reranker_config: &RerankerConfig) -> Vec<RankedSearchHit> {
    if hits.is_empty() {
        return Vec::new();
    }
    // Cross-provider dedup: keep the hit with most text when URLs match
    let hits = {
        let mut by_url: std::collections::HashMap<String, SearchHit> = std::collections::HashMap::new();
        for hit in hits {
            let key = normalize_url_for_dedup(&hit.url);
            by_url
                .entry(key)
                .and_modify(|existing| {
                    // Keep the one with more text content
                    if hit.text.len() > existing.text.len() {
                        *existing = hit.clone();
                    }
                })
                .or_insert(hit);
        }
        by_url.into_values().collect::<Vec<_>>()
    };
    let scorer = Bm25::default();
    let query_tokens = tokenize(query);
    let topic_tokens = tokenize(&state.topic);
    // Build combined query: topic + query terms (deduplicated)
    let mut combined_query = topic_tokens.clone();
    for t in &query_tokens {
        if !combined_query.contains(t) {
            combined_query.push(t.clone());
        }
    }
    // Tokenize each hit's text fields
    let doc_tokens: Vec<Vec<String>> = hits
        .iter()
        .map(|hit| tokenize(&format!("{} {} {}", hit.title, hit.summary, hit.text)))
        .collect();
    let corpus = Bm25Corpus::from_documents(&doc_tokens);

    // Compute BM25 scores for all hits
    let bm25_scores: Vec<f32> = doc_tokens
        .iter()
        .map(|dt| corpus.score(&scorer, &combined_query, dt))
        .collect();

    // Attempt cross-encoder reranking (highest quality)
    let query_text = format!("{} {}", state.topic, query);
    let hit_texts: Vec<String> = hits
        .iter()
        .map(|h| format!("{} {} {}", h.title, h.summary, h.text))
        .collect();
    let reranker_scores: Option<Vec<f32>> =
        if reranker_config.api_url.is_some() && reranker_config.api_key.is_some() {
            match rerank(reranker_config, &query_text, &hit_texts) {
                Ok(scores) => Some(scores),
                Err(e) => {
                    eprintln!("reranker failed, falling back: {}", e);
                    None
                }
            }
        } else {
            None
        };

    // Attempt semantic embedding reranking (bi-encoder, fallback if no cross-encoder)
    let embedding_scores: Option<Vec<f32>> =
        if reranker_scores.is_none() && embedding_config.api_url.is_some() && embedding_config.api_key.is_some() {
            match embed_texts(embedding_config, &hit_texts) {
                Ok(hit_vecs) => match embed_single(embedding_config, &query_text) {
                    Ok(query_vec) => {
                        Some(hit_vecs.iter().map(|hv| cosine_similarity(&query_vec, hv)).collect())
                    }
                    Err(e) => {
                        eprintln!("Warning: embedding query encoding failed, falling back to BM25 only: {e}");
                        None
                    }
                },
                Err(e) => {
                    eprintln!("Warning: embedding API failed, falling back to BM25 only: {e}");
                    None
                }
            }
        } else {
            None
        };

    let mut ranked: Vec<RankedSearchHit> = hits
        .into_iter()
        .enumerate()
        .map(|(index, hit)| {
            let bm25_raw = bm25_scores[index];
            // Convert BM25 to u32 (multiply by 1000 for precision)
            let mut score = (bm25_raw * 1000.0) as u32;
            let mut reasons = vec![format!("bm25 {:.3}", bm25_raw)];

            if let Some(ref rr_scores) = reranker_scores {
                // Cross-encoder reranker: 30% BM25 + 70% reranker
                let rr = rr_scores[index];
                let rr_u32 = (rr * 1000.0).max(0.0) as u32;
                score = (score as f64 * 0.3 + rr_u32 as f64 * 0.7) as u32;
                reasons.push(format!("reranker {:.3}", rr));
            } else if let Some(ref emb_scores) = embedding_scores {
                // Bi-encoder embedding: 60% BM25 + 40% semantic
                let cos_sim = emb_scores[index];
                let emb_score_u32 = (cos_sim * 1000.0).max(0.0) as u32;
                score = (score as f64 * 0.6 + emb_score_u32 as f64 * 0.4) as u32;
                reasons.push(format!("semantic {:.3}", cos_sim));
            }

            // Bonus for having full text (more content = more useful)
            if !hit.text.trim().is_empty() {
                score = score.saturating_add(50);
                reasons.push("has full text".to_string());
            }
            if !hit.summary.trim().is_empty() {
                score = score.saturating_add(20);
                reasons.push("has summary".to_string());
            }
            // Noise penalty
            let haystack = format!("{} {} {}", hit.title, hit.summary, hit.url);
            if likely_noise(&haystack) {
                score = score.saturating_sub(200);
                reasons.push("noise penalty".to_string());
            }
            // Slight provider rank bonus (lower index = higher in provider's ranking)
            score = score.saturating_add(10_u32.saturating_sub(index.min(10) as u32));
            RankedSearchHit {
                hit,
                rank: index + 1,
                rerank_score: score,
                rerank_reasons: reasons,
            }
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .rerank_score
            .cmp(&left.rerank_score)
            .then_with(|| left.rank.cmp(&right.rank))
    });
    ranked
}

/// Extract the root domain from a URL (e.g., "www.example.com" → "example.com").
pub(crate) fn extract_root_domain(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .to_lowercase();
    // Strip port
    let host = host.split(':').next().unwrap_or("");
    // Strip www. prefix
    let host = host.strip_prefix("www.").unwrap_or(host);
    // Keep last two parts (or three if TLD has dot, like co.uk — but keep simple)
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2..].join(".")
    } else {
        host.to_string()
    }
}

fn evidence_gate(state: &State, query: &str, hit: &SearchHit, accepted_domains: &[String]) -> GateDecision {
    if hit.provider == SearchProvider::Deterministic.to_string() {
        return GateDecision {
            accepted: true,
            score: 100,
            reasons: vec!["deterministic test provider".to_string()],
        };
    }
    let haystack_raw = format!("{} {} {} {}", hit.title, hit.summary, hit.text, hit.url);
    let haystack = normalize(&haystack_raw);
    let haystack_identifiers = strict_identifiers(&haystack_raw);
    let topic = normalize(&state.topic);
    let query = normalize(query);
    let mut score = 0_u32;
    let mut reasons = Vec::new();
    let identifiers = strict_identifiers(&format!("{} {}", state.topic, query));
    if !identifiers.is_empty() {
        if let Some(identifier) = identifiers
            .iter()
            .find(|identifier| haystack_identifiers.contains(*identifier))
        {
            score += 70;
            reasons.push(format!("exact identifier match: {identifier}"));
        } else if mentions_related_identifier(&haystack_identifiers, &identifiers) {
            return GateDecision {
                accepted: false,
                score,
                reasons: vec![
                    "mentions a related identifier but not the requested one".to_string(),
                ],
            };
        } else {
            return GateDecision {
                accepted: false,
                score,
                reasons: vec!["missing requested exact identifier".to_string()],
            };
        }
    }
    let topic_overlap = token_overlap(&topic, &haystack);
    let query_overlap = token_overlap(&query, &haystack);
    if hit.provider == "local_project" && (topic_overlap >= 0.08 || query_overlap >= 0.08) {
        score += 25;
        reasons.push("local project file matched the research query".to_string());
    }
    if topic_overlap >= 0.25 {
        score += (topic_overlap * 40.0) as u32;
        reasons.push(format!("topic overlap {:.2}", topic_overlap));
    }
    if query_overlap >= 0.25 {
        score += (query_overlap * 40.0) as u32;
        reasons.push(format!("query overlap {:.2}", query_overlap));
    }
    let authority = source_authority(&hit.url);
    match authority.as_str() {
        "academic" => {
            score += 20;
            reasons.push("academic source (arxiv/doi/pubmed)".to_string());
        }
        "official" => {
            score += 18;
            reasons.push("official source (gov/edu)".to_string());
        }
        "media" => {
            score += 8;
            reasons.push("media source".to_string());
        }
        "community" => {
            score += 3;
            reasons.push("community source".to_string());
        }
        _ => {}
    }
    // Penalize archived/cached content
    let url_lower = hit.url.to_lowercase();
    if url_lower.contains("web.archive.org")
        || url_lower.contains("cache.google")
        || url_lower.contains("webcache")
    {
        score = score.saturating_sub(10);
        reasons.push("archived/cached content".to_string());
    }
    // Freshness signal
    if hit.published_date.is_some() {
        score += 5;
        reasons.push("has publication date".to_string());
    }
    // Domain diversity bonus: reward sources from new domains
    let hit_domain = extract_root_domain(&hit.url);
    if !hit_domain.is_empty() && !accepted_domains.contains(&hit_domain) {
        score += 8;
        reasons.push("novel domain (diversity bonus)".to_string());
    }
    if likely_noise(&haystack) {
        score = score.saturating_sub(30);
        reasons.push("likely navigation, media, or unrelated aggregator noise".to_string());
    }
    let accepted = score >= if identifiers.is_empty() { 18 } else { 55 };
    if !accepted && reasons.is_empty() {
        reasons.push("insufficient topical relevance".to_string());
    }
    GateDecision {
        accepted,
        score,
        reasons,
    }
}

fn effective_target_sources(state: &State) -> u32 {
    if state.target_accepted_sources == 0 {
        return state.max_sources;
    }
    state.target_accepted_sources
}

fn should_complete_after_round(state: &State, collected_this_round: usize) -> bool {
    let target_sources = effective_target_sources(state) as usize;
    if target_sources > 0 && state.source_count >= target_sources {
        return true;
    }
    if state.stop_when_confident
        && state.min_accepted_sources > 0
        && state.source_count >= state.min_accepted_sources as usize
    {
        return true;
    }
    if state.stop_on_no_new_sources && collected_this_round == 0 {
        return true;
    }
    state.current_round >= state.max_rounds
}

fn strict_identifiers(value: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    value
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.')
        .map(normalize_identifier)
        .filter(|part| {
            part.len() >= 6
                && part.chars().any(|c| c.is_ascii_digit())
                && part.chars().any(|c| c.is_ascii_alphabetic())
                && part.chars().any(|c| matches!(c, '-' | '_' | '.'))
                && seen.insert(part.clone())
        })
        .collect()
}

fn mentions_related_identifier(candidates: &[String], identifiers: &[String]) -> bool {
    let namespaces = identifiers
        .iter()
        .filter_map(|identifier| identifier_namespace(identifier))
        .collect::<BTreeSet<_>>();
    candidates.iter().any(|candidate| {
        !identifiers.contains(&candidate)
            && identifier_namespace(&candidate)
                .is_some_and(|namespace| namespaces.contains(&namespace))
    })
}

fn normalize_identifier(value: &str) -> String {
    value
        .to_lowercase()
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string()
}

fn identifier_namespace(identifier: &str) -> Option<String> {
    let namespace = identifier
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>();
    if namespace.len() >= 2 {
        return Some(namespace);
    }
    None
}

fn likely_noise(value: &str) -> bool {
    [
        "bilibili",
        "newsletter",
        "signing up",
        "unsubscribe",
        "contacting us",
        "copy link",
        "new issue",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn is_technical_research_text(value: &str) -> bool {
    let value = normalize(value);
    [
        "api",
        "sdk",
        "library",
        "framework",
        "package",
        "module",
        "crate",
        "npm",
        "pip",
        "cargo",
        "rust",
        "python",
        "javascript",
        "typescript",
        "react",
        "vue",
        "nextjs",
        "node",
        "golang",
        "java",
        "linux",
        "kernel",
        "cve",
        "vulnerability",
        "exploit",
        "patch",
        "commit",
        "protocol",
        "rfc",
        "database",
        "sql",
        "kubernetes",
        "docker",
        "terraform",
        "oauth",
        "graphql",
        "grpc",
        "cli",
        "代码",
        "接口",
        "漏洞",
        "补丁",
        "框架",
        "库",
        "协议",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

pub(crate) fn short_excerpt(value: &str) -> String {
    short(value, 360)
}

pub(crate) fn short(value: &str, max_chars: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn credibility_score(url: &str) -> f32 {
    match source_authority(url).as_str() {
        "academic" => 0.76,
        "official" => 0.74,
        "local_project" => 0.66,
        "media" => 0.58,
        "community" => 0.38,
        _ => 0.45,
    }
}

fn source_authority(url: &str) -> String {
    let value = url.to_lowercase();
    if value.starts_with("file://") {
        return "local_project".to_string();
    }
    if value.contains(".gov")
        || value.contains(".edu")
        || value.contains("un.org")
        || value.contains("who.int")
        || value.contains("europa.eu")
    {
        return "official".to_string();
    }
    if value.contains("arxiv.org")
        || value.contains("doi.org")
        || value.contains("pubmed")
        || value.contains("nature.com")
        || value.contains("ieee.org")
        || value.contains("acm.org")
    {
        return "academic".to_string();
    }
    if value.contains("reuters")
        || value.contains("bbc")
        || value.contains("bloomberg")
        || value.contains("nytimes")
        || value.contains("ft.com")
    {
        return "media".to_string();
    }
    if value.contains("blog")
        || value.contains("medium.com")
        || value.contains("github.io")
        || value.contains("reddit")
    {
        return "community".to_string();
    }
    "unknown".to_string()
}

fn chunk_source(
    state: &State,
    query: &str,
    source: &SourceRecord,
    hit: &SearchHit,
) -> Vec<SourceChunkRecord> {
    let text = if hit.text.trim().is_empty() {
        hit.summary.clone()
    } else {
        hit.text.clone()
    };
    // If text is empty and URL is a PDF, try to fetch and extract text
    let text = if text.trim().is_empty() && is_pdf_url(&hit.url) {
        match fetch_pdf_text(&hit.url, 60) {
            Ok(pdf_text) => pdf_text,
            Err(_) => text, // fallback to empty/summary
        }
    } else {
        text
    };
    let chunks = text
        .split("\n\n")
        .flat_map(|paragraph| split_long_text(paragraph, 900))
        .map(|item| clean_text(&item))
        .filter(|item| item.chars().count() >= 80)
        .take(24)
        .collect::<Vec<_>>();
    let chunks = if chunks.is_empty() {
        vec![clean_text(&text)]
    } else {
        chunks
    };
    // Sort by relevance to topic+query, take top-K most relevant paragraphs
    let topic_query = normalize(&format!("{} {}", state.topic, query));
    let mut scored_chunks: Vec<(f32, String)> = chunks
        .into_iter()
        .filter(|item| !item.trim().is_empty())
        .map(|text| {
            let relevance = token_overlap(&topic_query, &normalize(&text));
            (relevance, text)
        })
        .collect();
    scored_chunks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored_chunks
        .into_iter()
        .take(12)
        .map(|(_, text)| text)
        .enumerate()
        .map(|(index, text)| SourceChunkRecord {
            id: id("chunk"),
            source_id: source.id.clone(),
            chunk_index: index,
            query: query.to_string(),
            url: source.url.clone(),
            title: source.title.clone(),
            token_count: normalize(&text).split_whitespace().count(),
            relevance_score: token_overlap(&topic_query, &normalize(&text)),
            text,
            created_at: now(),
        })
        .collect()
}

fn split_long_text(value: &str, max_chars: usize) -> Vec<String> {
    let value = value.trim();
    if value.chars().count() <= max_chars {
        return vec![value.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for sentence in value.split(['.', '。', '!', '?', '\n']) {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        if current.chars().count() + sentence.chars().count() + 2 > max_chars && !current.is_empty()
        {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(sentence);
        current.push_str(". ");
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn extract_claims(
    topic: &str,
    query: &str,
    source: &SourceRecord,
    hit: &SearchHit,
    chunks: &[SourceChunkRecord],
) -> Vec<String> {
    let chunk_text = chunks
        .iter()
        .filter(|chunk| chunk.relevance_score > 0.0)
        .take(6)
        .map(|chunk| chunk.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let fallback_text = if hit.text.trim().is_empty() {
        hit.summary.as_str()
    } else {
        hit.text.as_str()
    };
    let text = if chunk_text.trim().is_empty() {
        fallback_text
    } else {
        chunk_text.as_str()
    };
    let cleaned = strip_links_for_claims(text);
    let claims = sentence_candidates(&cleaned)
        .into_iter()
        .map(|item| clean_text(&item))
        .filter(|item| item.chars().count() >= 40)
        .filter(|item| !likely_claim_fragment(item))
        .take(4)
        .collect::<Vec<_>>();
    if claims.is_empty() {
        return vec![format!(
            "{} requires investigation of {} based on source {}.",
            topic, query, source.id
        )];
    }
    claims
}

fn strip_links_for_claims(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '[' {
            let mut label = String::new();
            for inner in chars.by_ref() {
                if inner == ']' {
                    break;
                }
                label.push(inner);
            }
            if chars.peek() == Some(&'(') {
                chars.next();
                for inner in chars.by_ref() {
                    if inner == ')' {
                        break;
                    }
                }
            }
            output.push_str(&label);
            continue;
        }
        output.push(ch);
    }
    output
        .split_whitespace()
        .filter(|part| {
            !part.starts_with("http://")
                && !part.starts_with("https://")
                && !part.starts_with("www.")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sentence_candidates(value: &str) -> Vec<String> {
    value
        .split(['。', '!', '?', '\n'])
        .flat_map(|part| part.split(". "))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn likely_claim_fragment(value: &str) -> bool {
    let normalized = normalize(value);
    normalized == "html"
        || normalized.starts_with("html ")
        || normalized.ends_with(" html")
        || normalized.contains(" c3ref ")
        || value.contains("](")
        || value.contains("https://")
        || value.contains("http://")
        || value.matches('/').count() >= 3
}

fn viewer_html(topic_id: &str, embedded_state: Option<&Value>) -> Result<String> {
    Ok(r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>research · __TOPIC_ID__</title>
  <style>
    :root {
      color-scheme: light;
      --ink: oklch(21% 0.028 248);
      --muted: oklch(49% 0.035 248);
      --quiet: oklch(66% 0.025 248);
      --paper: oklch(97.5% 0.008 110);
      --panel: oklch(99% 0.003 110);
      --line: oklch(87% 0.018 105);
      --line-strong: oklch(72% 0.035 122);
      --green: oklch(49% 0.12 158);
      --blue: oklch(50% 0.13 246);
      --red: oklch(55% 0.16 24);
      --gold: oklch(67% 0.14 82);
      --shadow: 0 14px 40px color-mix(in oklch, var(--ink), transparent 92%);
    }
    * { box-sizing: border-box; }
    html, body { height: 100%; }
    body {
      margin: 0;
      overflow: hidden;
      background: linear-gradient(180deg, oklch(99% 0.004 110), var(--paper));
      color: var(--ink);
      font: 14px/1.45 ui-sans-serif, "SF Pro Text", "PingFang SC", "Noto Sans CJK SC", sans-serif;
    }
    button, input { font: inherit; color: inherit; }
    button { cursor: pointer; }
    .app { position: relative; height: 100vh; display: grid; grid-template-rows: auto 1fr; }
    .top {
      min-height: 66px;
      border-bottom: 1px solid var(--line);
      display: grid;
      grid-template-columns: minmax(360px, 1fr) minmax(270px, auto) auto auto;
      align-items: center;
      gap: 14px;
      padding: 9px 16px 9px 20px;
      background:
        linear-gradient(180deg, color-mix(in oklch, white, var(--paper) 12%), color-mix(in oklch, var(--panel), var(--paper) 24%));
    }
    .identity { min-width: 0; }
    .eyebrow { color: var(--green); font-weight: 780; font-size: 11px; letter-spacing: .1em; text-transform: uppercase; }
    h1 { margin: 4px 0 0; font-size: 17px; line-height: 1.22; letter-spacing: 0; max-width: 980px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .strategy {
      min-width: 0;
      display: grid;
      grid-template-columns: repeat(3, minmax(104px, 1fr));
      gap: 6px;
      padding: 7px;
      border: 1px solid var(--line);
      border-radius: 10px;
      background: color-mix(in oklch, white, var(--paper) 14%);
    }
    .strategy-pill {
      min-width: 0;
      display: grid;
      grid-template-columns: auto 1fr;
      align-items: baseline;
      gap: 6px;
      border: 0;
      border-radius: 6px;
      background: transparent;
      padding: 2px 4px;
      color: var(--muted);
      font-size: 11px;
      font-weight: 760;
      line-height: 1;
    }
    .strategy-pill strong { color: var(--ink); font-weight: 820; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .modebar {
      display: flex;
      gap: 4px;
      align-items: center;
      padding: 4px;
      border: 1px solid var(--line);
      border-radius: 999px;
      background: color-mix(in oklch, white, var(--paper) 10%);
    }
    .mode {
      border: 1px solid transparent;
      border-radius: 999px;
      background: transparent;
      color: var(--muted);
      padding: 7px 10px;
      font-size: 12px;
      font-weight: 760;
    }
    .mode.active { border-color: var(--line); color: var(--ink); background: white; box-shadow: 0 1px 0 color-mix(in oklch, var(--line), transparent 42%); }
    .metrics {
      display: grid;
      grid-template-columns: repeat(3, auto);
      align-items: stretch;
      gap: 0;
      padding-left: 8px;
      border-left: 1px solid var(--line);
    }
    .metric {
      min-width: 0;
      border: 0;
      background: transparent;
      padding: 0 9px;
    }
    .metric b { display: block; color: var(--quiet); font-size: 10px; text-transform: uppercase; letter-spacing: .06em; }
    .metric span { display: block; margin-top: 2px; font-size: 13px; font-weight: 780; line-height: 1; }
    .metric.bump { animation: bump .45s cubic-bezier(.2,.8,.2,1); border-color: color-mix(in oklch, var(--green), white 20%); }
    .workflow {
      display: none;
      border-bottom: 1px solid var(--line);
      background: color-mix(in oklch, var(--panel), var(--paper) 32%);
      grid-template-columns: minmax(470px, 1fr) auto;
      gap: 18px;
      padding: 9px 18px 10px 22px;
      align-items: center;
    }
    .steps { display: grid; grid-template-columns: repeat(5, minmax(86px, 1fr)); gap: 0; min-width: 0; overflow: hidden; border: 1px solid var(--line); border-radius: 8px; background: white; }
    .step {
      min-width: 0;
      border: 0;
      border-right: 1px solid var(--line);
      border-radius: 0;
      padding: 7px 10px;
      background: transparent;
      color: var(--muted);
      text-align: left;
    }
    .step:last-child { border-right: 0; }
    .step b { display: block; font-size: 12px; line-height: 1.1; color: var(--ink); }
    .step span { display: block; margin-top: 3px; font-size: 11px; color: var(--quiet); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .step.done { background: color-mix(in oklch, var(--green), white 94%); }
    .step.active { background: color-mix(in oklch, var(--blue), white 92%); box-shadow: inset 0 -2px 0 var(--blue); }
    .artifacts { display: flex; justify-content: flex-end; gap: 6px; min-width: 0; }
    .artifact {
      min-width: 84px;
      border: 1px solid var(--line);
      border-radius: 999px;
      background: white;
      padding: 7px 11px;
      text-align: left;
    }
    .artifact:hover { border-color: var(--blue); background: color-mix(in oklch, var(--blue), white 94%); }
    .artifact b { display: inline; font-size: 12px; line-height: 1.1; }
    .artifact span { display: none; }
    .workspace {
      position: relative;
      min-height: 0;
      height: 100%;
      display: block;
      overflow: hidden;
      background:
        radial-gradient(circle at 24px 24px, color-mix(in oklch, var(--line), transparent 12%) 1px, transparent 1.4px),
        linear-gradient(180deg, color-mix(in oklch, white, var(--paper) 26%), var(--paper));
      background-size: 42px 42px, auto;
    }
    .map { position: absolute; inset: 0; min-width: 0; min-height: 0; overflow: hidden; }
    .stage { position: absolute; inset: 0; overflow: hidden; cursor: grab; touch-action: none; }
    .stage.dragging { cursor: grabbing; }
    .board { position: relative; width: max(1180px, 100%); min-height: max(760px, 100%); transform-origin: 0 0; }
    svg { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; overflow: visible; }
    .node {
      position: absolute;
      width: 240px;
      height: 104px;
      padding: 10px 11px;
      border: 1px solid var(--line-strong);
      border-radius: 8px;
      background: color-mix(in oklch, white, var(--paper) 10%);
      box-shadow: 0 1px 0 color-mix(in oklch, var(--line), transparent 18%), var(--shadow);
      text-align: left;
      overflow: hidden;
      cursor: grab;
      user-select: none;
      touch-action: none;
      transition: left .24s ease, top .24s ease, transform .18s ease, border-color .18s ease, background .18s ease;
    }
    .node.dim { opacity: .38; filter: saturate(.72); }
    .node.lit { opacity: 1; filter: none; background: white; border-color: var(--blue); box-shadow: 0 0 0 2px color-mix(in oklch, var(--blue), transparent 28%), 0 12px 32px color-mix(in oklch, var(--blue), transparent 86%); }
    .node:hover, .node.active { transform: translateY(-2px); border-color: var(--blue); background: white; }
    .node.grabbing { cursor: grabbing; transform: translateY(-2px) scale(1.01); z-index: 5; }
    .node.new { animation: appear .55s cubic-bezier(.18,.8,.22,1); }
    .node.dim.new { animation: none; }
    .node.root { width: 260px; border-color: color-mix(in oklch, var(--blue), white 25%); }
    .node.source { border-color: color-mix(in oklch, var(--gold), white 12%); }
    .node.claim { border-color: color-mix(in oklch, var(--green), white 12%); background: color-mix(in oklch, var(--green), white 91%); }
    .node.lead { border-color: color-mix(in oklch, var(--blue), white 12%); background: color-mix(in oklch, var(--blue), white 93%); }
    .node.report { border-color: var(--ink); }
    .kind { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: 11px; font-weight: 760; text-transform: uppercase; letter-spacing: .04em; }
    .kind::before { content: ""; width: 8px; height: 8px; background: currentColor; }
    .node-title { margin: 7px 0 0; font-size: 12px; line-height: 1.32; height: 47px; overflow: hidden; }
    .node-meta { margin-top: 7px; color: var(--quiet); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .controls {
      position: absolute;
      left: 14px;
      bottom: 14px;
      z-index: 6;
      display: flex;
      gap: 6px;
      background: color-mix(in oklch, white, var(--paper) 18%);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 5px;
      box-shadow: 0 1px 0 color-mix(in oklch, var(--line), transparent 20%);
    }
    .controls button {
      width: 30px;
      height: 28px;
      border: 1px solid transparent;
      background: white;
      border-radius: 6px;
      font-weight: 780;
    }
    .controls span { min-width: 46px; display: grid; place-items: center; color: var(--muted); font-size: 12px; }
    .inspector {
      position: absolute;
      top: 16px;
      right: 16px;
      bottom: 16px;
      z-index: 7;
      width: min(390px, calc(100vw - 32px));
      min-width: 0;
      min-height: 0;
      border: 1px solid var(--line);
      border-radius: 12px;
      background: color-mix(in oklch, white, var(--paper) 6%);
      box-shadow: 0 18px 60px color-mix(in oklch, var(--ink), transparent 88%);
      display: none;
      grid-template-rows: auto auto auto 1fr;
      overflow: hidden;
    }
    .workspace.panel-open .inspector { display: grid; }
    .inspector-title { padding: 12px 14px 8px; border-bottom: 1px solid var(--line); display: grid; grid-template-columns: 1fr auto; gap: 10px; align-items: start; }
    .inspector-title b { display: block; font-size: 13px; }
    .inspector-title span { display: block; margin-top: 2px; color: var(--quiet); font-size: 11px; }
    .panel-close {
      width: 26px;
      height: 26px;
      border: 1px solid var(--line);
      border-radius: 999px;
      background: white;
      color: var(--muted);
      font-weight: 760;
      line-height: 1;
    }
    .panel-close:hover { border-color: var(--blue); color: var(--ink); }
    .tabs { display: none; }
    .tab {
      border: 1px solid transparent;
      background: transparent;
      border-radius: 6px;
      padding: 6px 7px;
      color: var(--muted);
      font-weight: 720;
      font-size: 12px;
    }
    .tab.active { border-color: var(--line); background: white; color: var(--ink); box-shadow: 0 1px 0 color-mix(in oklch, var(--line), transparent 45%); }
    .filter { padding: 7px 10px 9px; border-bottom: 1px solid var(--line); }
    .filter input {
      width: 100%;
      border: 1px solid var(--line);
      background: white;
      border-radius: 7px;
      padding: 8px 10px;
      outline: none;
    }
    .filter input:focus { border-color: var(--blue); }
    .facets {
      border-bottom: 1px solid var(--line);
      padding: 8px 10px 9px;
      display: flex;
      gap: 6px;
      overflow-x: auto;
    }
    .facet {
      flex: 0 0 auto;
      border: 1px solid var(--line);
      border-radius: 999px;
      background: color-mix(in oklch, white, var(--paper) 20%);
      padding: 5px 9px;
      color: var(--muted);
      font-size: 12px;
      font-weight: 720;
    }
    .facet.active { border-color: var(--blue); color: var(--ink); background: color-mix(in oklch, var(--blue), white 92%); }
    .list { min-height: 0; overflow: auto; padding: 10px 10px 14px; display: grid; align-content: start; gap: 7px; }
    .row {
      border: 1px solid var(--line);
      border-radius: 8px;
      background: white;
      padding: 9px 10px;
      text-align: left;
      box-shadow: 0 1px 0 color-mix(in oklch, var(--line), transparent 42%);
    }
    .row:hover, .row.active { border-color: var(--blue); background: color-mix(in oklch, var(--blue), white 96%); }
    .row.new { animation: appear .55s cubic-bezier(.18,.8,.22,1); }
    .row-title { font-weight: 760; line-height: 1.28; overflow-wrap: anywhere; font-size: 12px; }
    .row-meta { margin-top: 4px; color: var(--muted); font-size: 11px; overflow-wrap: anywhere; }
    .detail {
      position: absolute;
      left: 18px;
      bottom: 18px;
      z-index: 8;
      width: min(560px, calc(100vw - 36px));
      border: 1px solid var(--line);
      border-radius: 12px;
      background: color-mix(in oklch, white, var(--paper) 6%);
      max-height: 48vh;
      overflow: auto;
      box-shadow: 0 18px 60px color-mix(in oklch, var(--ink), transparent 88%);
      display: none;
    }
    .detail.open { display: block; }
    .detail-head {
      position: sticky;
      top: 0;
      z-index: 1;
      display: grid;
      grid-template-columns: 1fr auto;
      gap: 10px;
      align-items: start;
      padding: 12px 14px 10px;
      border-bottom: 1px solid var(--line);
      background: color-mix(in oklch, white, var(--paper) 6%);
    }
    .detail h2 { margin: 0; font-size: 15px; line-height: 1.32; }
    .detail-kind { margin-top: 3px; color: var(--quiet); font-size: 11px; font-weight: 760; }
    .detail-body { padding: 12px 14px 14px; color: var(--muted); }
    .detail-body h1, .detail-body h2, .detail-body h3 {
      margin: 14px 0 8px;
      color: var(--ink);
      line-height: 1.28;
    }
    .detail-body h1 { font-size: 18px; }
    .detail-body h2 { font-size: 15px; }
    .detail-body h3 { font-size: 13px; }
    .detail-body p { margin: 8px 0; overflow-wrap: anywhere; }
    .detail-body blockquote {
      margin: 10px 0;
      padding: 7px 10px;
      border-left: 3px solid var(--green);
      background: color-mix(in oklch, var(--green), white 94%);
      color: var(--ink);
    }
    .detail-body ul, .detail-body ol { margin: 8px 0 8px 20px; padding: 0; }
    .detail-body li { margin: 4px 0; }
    .table-wrap {
      margin: 10px 0;
      overflow: auto;
      border: 1px solid var(--line);
      border-radius: 8px;
      background: white;
    }
    .detail-body table {
      width: 100%;
      border-collapse: collapse;
      min-width: 520px;
      font-size: 12px;
      line-height: 1.42;
    }
    .detail-body th, .detail-body td {
      border-bottom: 1px solid var(--line);
      border-right: 1px solid var(--line);
      padding: 7px 8px;
      text-align: left;
      vertical-align: top;
    }
    .detail-body th {
      color: var(--ink);
      background: color-mix(in oklch, var(--paper), white 52%);
      font-weight: 780;
    }
    .detail-body td:last-child, .detail-body th:last-child { border-right: 0; }
    .detail-body tr:last-child td { border-bottom: 0; }
    .detail-body code {
      padding: 1px 4px;
      border: 1px solid var(--line);
      border-radius: 4px;
      background: color-mix(in oklch, white, var(--paper) 18%);
      font: 11px/1.35 ui-monospace, "SF Mono", Menlo, monospace;
    }
    .detail-body pre {
      margin: 10px 0;
      padding: 10px;
      border: 1px solid var(--line);
      border-radius: 8px;
      background: color-mix(in oklch, var(--ink), white 96%);
      overflow: auto;
      white-space: pre-wrap;
    }
    .detail-body pre code { border: 0; padding: 0; background: transparent; }
    .json-view {
      margin: 0;
      font: 11px/1.55 ui-monospace, "SF Mono", Menlo, monospace;
      color: var(--muted);
    }
    .json-key { color: var(--ink); font-weight: 720; }
    .json-string { color: color-mix(in oklch, var(--green), black 10%); }
    .json-number, .json-boolean { color: var(--blue); }
    .json-null { color: var(--quiet); }
    .detail a { color: var(--blue); text-decoration: none; overflow-wrap: anywhere; }
    .detail-close {
      width: 26px;
      height: 26px;
      border: 1px solid var(--line);
      border-radius: 999px;
      background: white;
      color: var(--muted);
      font-weight: 760;
      line-height: 1;
    }
    .detail-close:hover { border-color: var(--blue); color: var(--ink); }
    .stream {
      display: none;
      height: 78px;
      border-top: 1px solid var(--line);
      background: color-mix(in oklch, var(--panel), var(--paper) 18%);
      grid-template-columns: minmax(300px, 1fr) minmax(290px, 24vw);
    }
    .events { overflow: auto; padding: 8px 12px; display: flex; gap: 6px; align-items: flex-start; }
    .event {
      flex: 0 0 170px;
      border-left: 2px solid color-mix(in oklch, var(--green), white 25%);
      background: transparent;
      padding: 4px 8px 5px;
      text-align: left;
    }
    .event.new { animation: appear .55s cubic-bezier(.18,.8,.22,1); }
    .event b { display: block; font-size: 12px; }
    .event span { display: block; margin-top: 5px; color: var(--quiet); font-size: 11px; }
    .growth {
      border-left: 1px solid var(--line);
      padding: 9px 12px;
      display: grid;
      gap: 8px;
      align-content: center;
    }
    .bar { height: 5px; background: color-mix(in oklch, var(--line), white 30%); border-radius: 99px; overflow: hidden; }
    .bar span { display: block; height: 100%; width: 0%; background: var(--green); transition: width .35s ease; }
    .empty { color: var(--quiet); padding: 26px; }
    .edge { transition: stroke-width .18s ease, opacity .18s ease; }
    .edge.dim { opacity: .12; }
    .edge.lit { opacity: 1; }
    .edge.new { animation: draw .7s cubic-bezier(.18,.8,.22,1); }
    .edge.dim.new { animation: none; }
    @keyframes appear { from { opacity: 0; transform: translateY(10px) scale(.985); } to { opacity: 1; transform: translateY(0) scale(1); } }
    @keyframes bump { 0% { transform: scale(1); } 45% { transform: scale(1.045); } 100% { transform: scale(1); } }
    @keyframes draw { from { opacity: 0; stroke-dashoffset: 42; } to { opacity: 1; stroke-dashoffset: 0; } }
    @media (max-width: 980px) {
      body { overflow: auto; }
      .app { min-height: 100vh; height: auto; }
      .top { grid-template-columns: 1fr; align-items: stretch; }
      .strategy { grid-template-columns: repeat(2, minmax(0, 1fr)); }
      .modebar { justify-content: flex-start; overflow-x: auto; border-radius: 10px; }
      .metrics { overflow-x: auto; }
      .steps, .artifacts { overflow-x: auto; justify-content: flex-start; }
      .workspace { min-height: 900px; }
      .inspector { position: fixed; top: 74px; left: 12px; right: 12px; bottom: 12px; width: auto; }
      .stream { height: auto; }
      .growth { border-left: 0; border-top: 1px solid var(--line); }
    }
  </style>
</head>
<body>
  <div class="app">
    <header class="top">
      <div class="identity">
        <div class="eyebrow">research graph · __TOPIC_ID__</div>
        <h1 id="topic">Loading research workspace</h1>
      </div>
      <div class="strategy" id="strategy"></div>
      <div class="modebar" id="modebar"></div>
      <div class="metrics" id="metrics"></div>
    </header>
    <section class="workflow">
      <div class="steps" id="steps"></div>
      <div class="artifacts" id="artifacts"></div>
    </section>
    <main class="workspace" id="workspace">
      <section class="map">
        <div class="stage" id="stage">
          <div class="board" id="board">
            <svg id="edges"></svg>
            <div id="nodes"></div>
          </div>
        </div>
        <div class="controls">
          <button id="zoomOut" title="缩小">−</button>
          <span id="zoomLabel">100%</span>
          <button id="zoomIn" title="放大">+</button>
          <button id="zoomFit" title="适配">⤢</button>
          <button id="resetLayout" title="重置卡片位置">↺</button>
        </div>
      </section>
      <section class="inspector">
        <div class="inspector-title">
          <div>
            <b id="panelTitle">证据检查器</b>
            <span id="panelSubtitle">筛选来源、结论和图谱对象</span>
          </div>
          <button class="panel-close" id="panelClose" title="关闭">×</button>
        </div>
        <div class="tabs" id="tabs"></div>
        <div class="filter"><input id="filter" placeholder="筛选证据、结论、节点、事件" /></div>
        <div class="facets" id="facets"></div>
        <div class="list" id="list"></div>
      </section>
    </main>
    <footer class="stream">
      <div class="events" id="events"></div>
      <div class="growth">
        <div><b id="growthTitle">等待图谱增长</b></div>
        <div class="bar"><span id="sourceBar"></span></div>
        <div class="bar"><span id="claimBar"></span></div>
        <div class="bar"><span id="linkBar"></span></div>
      </div>
    </footer>
    <aside class="detail" id="detail"></aside>
  </div>
  <script>
    const topicId = "__TOPIC_ID__";
    const embeddedState = __EMBEDDED_STATE__;
      const storedManual = (() => {
      try { return new Map(JSON.parse(localStorage.getItem(`research:${topicId}:positions:v2`) || "[]")); }
      catch { return new Map(); }
    })();
    const state = { mode: "graph", facet: "all", selected: null, related: new Set(), data: null, seen: new Set(), counts: { sources: 0, claims: 0, links: 0, events: 0 }, view: { x: 24, y: 24, scale: .86 }, drag: null, nodeDrag: null, suppressClick: false, fitted: false, manual: storedManual };
    const esc = (s) => String(s ?? "").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;","\"":"&quot;","'":"&#39;"}[c]));
    const short = (s, n = 120) => {
      const value = String(s ?? "").replace(/\s+/g, " ").trim();
      return value.length > n ? value.slice(0, n - 1) + "…" : value;
    };
    function inlineMarkdown(value) {
      return esc(value)
        .replace(/`([^`]+)`/g, "<code>$1</code>")
        .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
        .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2" target="_blank" rel="noreferrer">$1</a>');
    }
    function tableCells(line) {
      const trimmed = line.trim();
      if (!trimmed.includes("|")) return null;
      const normalized = trimmed.startsWith("|") ? trimmed.slice(1) : trimmed;
      const withoutEnd = normalized.endsWith("|") ? normalized.slice(0, -1) : normalized;
      return withoutEnd.split("|").map(cell => cell.trim());
    }
    function isTableDivider(line) {
      const cells = tableCells(line);
      return Boolean(cells?.length) && cells.every(cell => /^:?-{3,}:?$/.test(cell));
    }
    function renderTable(rows) {
      const header = rows[0] || [];
      const body = rows.slice(2);
      return `<div class="table-wrap"><table><thead><tr>${header.map(cell => `<th>${inlineMarkdown(cell)}</th>`).join("")}</tr></thead><tbody>${body.map(row => `<tr>${row.map(cell => `<td>${inlineMarkdown(cell)}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`;
    }
    function markdownToHtml(value) {
      const lines = String(value || "").replace(/\r\n/g, "\n").split("\n");
      const html = [];
      let paragraph = [];
      let list = null;
      let code = [];
      let table = null;
      const flushParagraph = () => {
        if (!paragraph.length) return;
        html.push(`<p>${inlineMarkdown(paragraph.join(" "))}</p>`);
        paragraph = [];
      };
      const flushList = () => {
        if (!list) return;
        html.push(`<${list.type}>${list.items.map(item => `<li>${inlineMarkdown(item)}</li>`).join("")}</${list.type}>`);
        list = null;
      };
      const flushTable = () => {
        if (!table) return;
        html.push(renderTable(table));
        table = null;
      };
      for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
        const line = lines[lineIndex];
        if (line.trim().startsWith("```")) {
          if (code.length) {
            html.push(`<pre><code>${esc(code.join("\n"))}</code></pre>`);
            code = [];
            continue;
          }
          flushParagraph();
          flushList();
          flushTable();
          code.push("");
          continue;
        }
        if (code.length) {
          code.push(line);
          continue;
        }
        const trimmed = line.trim();
        if (!trimmed) {
          flushParagraph();
          flushList();
          flushTable();
          continue;
        }
        const cells = tableCells(trimmed);
        if (cells && (table || isTableDivider(lines[lineIndex + 1] || "") || isTableDivider(trimmed))) {
          flushParagraph();
          flushList();
          if (!table) table = [];
          table.push(cells);
          continue;
        }
        if (table) flushTable();
        const heading = trimmed.match(/^(#{1,3})\s+(.+)$/);
        if (heading) {
          flushParagraph();
          flushList();
          flushTable();
          const level = heading[1].length;
          html.push(`<h${level}>${inlineMarkdown(heading[2])}</h${level}>`);
          continue;
        }
        const quote = trimmed.match(/^>\s?(.+)$/);
        if (quote) {
          flushParagraph();
          flushList();
          flushTable();
          html.push(`<blockquote>${inlineMarkdown(quote[1])}</blockquote>`);
          continue;
        }
        const ordered = trimmed.match(/^\d+\.\s+(.+)$/);
        const unordered = trimmed.match(/^[-*]\s+(.+)$/);
        if (ordered || unordered) {
          flushParagraph();
          const type = ordered ? "ol" : "ul";
          if (!list || list.type !== type) flushList();
          if (!list) list = { type, items: [] };
          list.items.push((ordered || unordered)[1]);
          continue;
        }
        paragraph.push(trimmed);
      }
      if (code.length) html.push(`<pre><code>${esc(code.join("\n"))}</code></pre>`);
      flushParagraph();
      flushList();
      flushTable();
      return html.join("");
    }
    function tryJson(value) {
      if (Array.isArray(value) || (value && typeof value === "object")) return value;
      if (typeof value !== "string") return null;
      const trimmed = value.trim();
      if (!trimmed || !["{", "["].includes(trimmed[0])) return null;
      try { return JSON.parse(trimmed); } catch { return null; }
    }
    function jsonToHtml(value) {
      return esc(JSON.stringify(value, null, 2))
        .replace(/(&quot;[^&]+?&quot;)(?=:)/g, '<span class="json-key">$1</span>')
        .replace(/: (&quot;.*?&quot;)/g, ': <span class="json-string">$1</span>')
        .replace(/: (-?\d+(?:\.\d+)?)/g, ': <span class="json-number">$1</span>')
        .replace(/: (true|false)/g, ': <span class="json-boolean">$1</span>')
        .replace(/: null/g, ': <span class="json-null">null</span>');
    }
    function detailBodyHtml(value) {
      const json = tryJson(value);
      if (json !== null) return `<pre class="json-view"><code>${jsonToHtml(json)}</code></pre>`;
      return markdownToHtml(value);
    }
    function itemKey(kind, item, index) {
      return `${kind}:${item.id || item.url || item.time || index}`;
    }
    function markNew(kind, item, index) {
      const key = itemKey(kind, item, index);
      const fresh = !state.seen.has(key);
      state.seen.add(key);
      return fresh ? " new" : "";
    }
    function label(value) {
      return ({
        level: "级别",
        sources: "来源",
        claims: "结论",
        links: "关系",
        nodes: "节点",
        events: "事件",
        sources: "来源",
        claims: "结论",
        status: "状态",
        root: "研究主题",
        source: "来源",
        claim: "结论",
        lead: "线索",
        report: "报告",
        graph: "图谱",
        agents: "智能体",
        agent_run: "智能体运行",
        planner: "规划",
        searcher: "检索",
        verifier: "验证",
        reader: "阅读",
        linker: "关联",
        writer: "写作",
        reviewer: "复核",
        single_pass_agent_pipeline: "单轮流水线",
        multi_role_reviewed_pipeline: "多角色复核流水线",
        microkernel_multi_agent_research: "微内核多智能体研究",
        all: "全部",
        accepted: "已采纳",
        rejected: "已拒绝",
        fetched: "已抓取",
        exa: "Exa",
        code: "代码资料",
        zhipu: "智谱",
        minimax: "MiniMax",
        hybrid: "混合",
        deterministic: "确定性",
        quick: "快速",
        deep: "深度",
        research: "研究",
        completed: "已完成",
        running: "搜索中",
        paused: "已暂停",
        stopped: "已停止",
        error: "错误",
      })[value] || value;
    }
    function applyView() {
      const board = document.getElementById("board");
      board.style.transform = `translate(${state.view.x}px, ${state.view.y}px) scale(${state.view.scale})`;
      document.getElementById("zoomLabel").textContent = `${Math.round(state.view.scale * 100)}%`;
    }
    function setZoom(nextScale, anchor) {
      const stage = document.getElementById("stage");
      const scale = Math.max(.35, Math.min(1.8, nextScale));
      const rect = stage.getBoundingClientRect();
      const cx = anchor?.x ?? rect.width / 2;
      const cy = anchor?.y ?? rect.height / 2;
      const worldX = (cx - state.view.x) / state.view.scale;
      const worldY = (cy - state.view.y) / state.view.scale;
      state.view.scale = scale;
      state.view.x = cx - worldX * scale;
      state.view.y = cy - worldY * scale;
      applyView();
    }
    function renderModebar() {
      const modes = [["graph", "图谱"], ["search", "搜索"], ["agents", "智能体"], ["sources", "来源"], ["claims", "结论"], ["events", "过程"], ["report", "报告"]];
      document.getElementById("modebar").innerHTML = modes.map(mode =>
        `<button class="mode ${state.mode === mode[0] ? "active" : ""}" data-mode="${mode[0]}">${mode[1]}</button>`
      ).join("");
      document.getElementById("workspace").classList.toggle("panel-open", state.mode !== "graph");
    }
    function setMode(mode) {
      if (state.mode === mode && mode !== "graph") {
        closePanel();
        return;
      }
      state.mode = mode;
      state.facet = "all";
      renderModebar();
      renderPanelChrome();
      renderFacets(state.data);
      renderList(state.data);
      if (mode === "report") select("report", "report");
    }
    function closePanel() {
      state.mode = "graph";
      renderModebar();
      renderPanelChrome();
    }
    function fitView() {
      const stage = document.getElementById("stage");
      const board = document.getElementById("board");
      const scale = Math.max(.35, Math.min(1.05, Math.min(stage.clientWidth / board.offsetWidth, stage.clientHeight / board.offsetHeight) * .9));
      state.view.scale = scale;
      state.view.x = Math.max(18, (stage.clientWidth - board.offsetWidth * scale) / 2);
      state.view.y = Math.max(18, (stage.clientHeight - board.offsetHeight * scale) / 2);
      applyView();
    }
    function graphNodes(data) {
      const sources = new Map((data.sources || []).map(item => [item.id, item]));
      const claims = new Map((data.claims || []).map(item => [item.id, item]));
      const leads = new Map((data.leads || []).map(item => [item.id, item]));
      const nodes = (data.graph?.nodes || []).map(node => {
        const raw = sources.get(node.id) || claims.get(node.id) || leads.get(node.id) || node;
        return {
          kind: node.kind,
          id: node.id,
          label: label(node.kind),
          title: node.title,
          meta: node.detail || node.id,
          raw,
        };
      });
      nodes.push({
        kind: "report",
        id: "report",
        label: label("report"),
        title: data.state.status === "completed" ? "最终报告已生成" : "报告正在生成",
        meta: `${label(data.state.status)} · 第 ${data.state.current_round}/${data.state.max_rounds} 轮`,
        raw: { id: "report", title: "最终报告", report: data.report },
      });
      return nodes;
    }
    function layout(nodes, data) {
      const stage = document.getElementById("stage");
      const nodeWidth = 240;
      const nodeHeight = 104;
      const groups = {
        root: nodes.filter(node => node.kind === "root"),
        source: nodes.filter(node => node.kind === "source"),
        claim: nodes.filter(node => node.kind === "claim"),
        lead: nodes.filter(node => node.kind === "lead" || !["root", "source", "claim", "report"].includes(node.kind)),
        report: nodes.filter(node => node.kind === "report"),
      };
      const claimsBySource = new Map();
      for (const edge of data.graph?.edges || []) {
        if (edge.kind === "supported_by") {
          if (!claimsBySource.has(edge.to)) claimsBySource.set(edge.to, []);
          claimsBySource.get(edge.to).push(edge.from);
        }
      }
      const claimById = new Map(groups.claim.map(node => [node.id, node]));
      const assignedClaims = new Set();
      const blocks = groups.source.map(source => {
        const claims = (claimsBySource.get(source.id) || []).map(id => claimById.get(id)).filter(Boolean);
        claims.forEach(claim => assignedClaims.add(claim.id));
        return { source, claims };
      });
      const looseClaims = groups.claim.filter(claim => !assignedClaims.has(claim.id));
      if (looseClaims.length) blocks.push({ source: null, claims: looseClaims });
      const branchCount = Math.max(1, blocks.length);
      const maxRows = Math.max(1, ...blocks.map(block => Math.ceil(block.claims.length / Math.max(1, Math.min(3, Math.ceil(Math.sqrt(Math.max(1, block.claims.length))))))));
      const sourceRadius = Math.max(330, branchCount * 58);
      const claimStartRadius = sourceRadius + 280;
      const claimRowGap = 165;
      const outerRadius = claimStartRadius + Math.max(0, maxRows - 1) * claimRowGap + 360;
      const radius = Math.max(outerRadius, groups.lead.length ? sourceRadius + 420 : 0, groups.report.length ? sourceRadius + 420 : 0);
      const width = Math.max(stage.clientWidth * 1.2, radius * 2 + nodeWidth * 2);
      const height = Math.max(stage.clientHeight * 1.2, radius * 2 + nodeHeight * 2);
      document.getElementById("board").style.width = width + "px";
      document.getElementById("board").style.minHeight = height + "px";
      const points = new Map();
      const cx = width / 2;
      const cy = height / 2;
      const placeAt = (node, x, y) => points.set(node.id, [Math.round(x - nodeWidth / 2), Math.round(y - nodeHeight / 2)]);
      const branchAngle = (branchIndex) => {
        const start = -Math.PI / 2;
        return start + branchIndex * (Math.PI * 2 / branchCount);
      };
      groups.root.forEach(node => placeAt(node, cx, cy));
      blocks.forEach((block, branchIndex) => {
        const angle = branchAngle(branchIndex);
        const sx = cx + Math.cos(angle) * sourceRadius;
        const sy = cy + Math.sin(angle) * sourceRadius;
        if (block.source) placeAt(block.source, sx, sy);
        const perRow = Math.max(1, Math.min(3, Math.ceil(Math.sqrt(Math.max(1, block.claims.length)))));
        const sector = Math.PI * 2 / branchCount;
        const gap = Math.min(.22, sector / (perRow + 2));
        block.claims.forEach((claim, index) => {
          const row = Math.floor(index / perRow);
          const slot = index % perRow;
          const itemsInRow = Math.min(perRow, block.claims.length - row * perRow);
          const claimAngle = angle + (slot - (itemsInRow - 1) / 2) * gap;
          const distance = claimStartRadius + row * claimRowGap;
          placeAt(claim, cx + Math.cos(claimAngle) * distance, cy + Math.sin(claimAngle) * distance);
        });
      });
      groups.lead.forEach((lead, index) => {
        const angle = Math.PI / 2 + (index - (groups.lead.length - 1) / 2) * .28;
        const distance = sourceRadius + 280;
        const x = cx + Math.cos(angle) * distance;
        const y = cy + Math.sin(angle) * distance;
        placeAt(lead, x, y);
      });
      groups.report.forEach((report, index) => {
        const angle = Math.PI / 4 + index * .2;
        const distance = sourceRadius + 300;
        const x = cx + Math.cos(angle) * distance;
        const y = cy + Math.sin(angle) * distance;
        placeAt(report, x, y);
      });
      for (const [id, point] of state.manual) {
        if (points.has(id)) points.set(id, point);
      }
      return points;
    }
    function edgeColor(kind) {
      if (kind === "supported_by") return "color-mix(in oklch, var(--green), transparent 14%)";
      if (kind === "collects_source") return "color-mix(in oklch, var(--gold), transparent 12%)";
      if (kind === "tracks_claim") return "color-mix(in oklch, var(--green), transparent 20%)";
      if (kind === "tracks_lead" || kind === "suggests_lead") return "color-mix(in oklch, var(--blue), transparent 18%)";
      return "color-mix(in oklch, var(--muted), transparent 34%)";
    }
    function relatedFor(data, id) {
      const related = new Set(id ? [id] : []);
      for (const edge of data.graph?.edges || []) {
        if (edge.from === id) related.add(edge.to);
        if (edge.to === id) related.add(edge.from);
      }
      if (id === "report" && data.graph?.root_id) related.add(data.graph.root_id);
      if (id === data.graph?.root_id) related.add("report");
      return related;
    }
    function focusClass(id) {
      if (!state.selected) return " dim";
      return state.related.has(id) ? " lit" : " dim";
    }
    function edgeFocusClass(edge) {
      if (!state.selected) return " dim";
      return state.related.has(edge.from) && state.related.has(edge.to) && (edge.from === state.selected.id || edge.to === state.selected.id)
        ? " lit"
        : " dim";
    }
    function focusSelection(points) {
      if (!state.selected || !points.has(state.selected.id)) return;
      const ids = [...state.related].filter(id => points.has(id));
      if (ids.length < 2) return;
      const bounds = ids.reduce((box, id) => {
        const point = points.get(id);
        return {
          minX: Math.min(box.minX, point[0]),
          minY: Math.min(box.minY, point[1]),
          maxX: Math.max(box.maxX, point[0] + 240),
          maxY: Math.max(box.maxY, point[1] + 104),
        };
      }, { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity });
      const stage = document.getElementById("stage");
      const padding = 110;
      const width = Math.max(1, bounds.maxX - bounds.minX + padding * 2);
      const height = Math.max(1, bounds.maxY - bounds.minY + padding * 2);
      const scale = Math.max(.35, Math.min(1.08, Math.min(stage.clientWidth / width, stage.clientHeight / height)));
      state.view.scale = scale;
      state.view.x = (stage.clientWidth - (bounds.maxX + bounds.minX) * scale) / 2;
      state.view.y = (stage.clientHeight - (bounds.maxY + bounds.minY) * scale) / 2;
      applyView();
    }
    function drawEdges(data, points) {
      const svg = document.getElementById("edges");
      const edges = [...(data.graph?.edges || [])];
      const root = data.graph?.root_id;
      if (root && points.has(root) && points.has("report")) {
        edges.push({ id: "edge:report", kind: "synthesizes_report", from: root, to: "report", rationale: "Final report output" });
      }
      svg.innerHTML = edges.filter(edge => points.has(edge.from) && points.has(edge.to)).map((edge, index) => {
        const from = points.get(edge.from);
        const to = points.get(edge.to);
      const fromCenter = [from[0] + 120, from[1] + 52];
        const toCenter = [to[0] + 120, to[1] + 52];
        const dx = toCenter[0] - fromCenter[0];
        const dy = toCenter[1] - fromCenter[1];
        const length = Math.max(1, Math.hypot(dx, dy));
        const startX = fromCenter[0] + dx / length * 126;
        const startY = fromCenter[1] + dy / length * 58;
        const endX = toCenter[0] - dx / length * 126;
        const endY = toCenter[1] - dy / length * 58;
      const sameBand = Math.abs(dy) < 42;
      const local = length < 470;
      const vertical = Math.abs(dx) < 80;
      const shouldCurve = !sameBand && !local && !vertical;
        const bend = shouldCurve ? Math.min(150, Math.max(56, length * .14)) : 0;
        const normalX = -dy / length * bend;
        const normalY = dx / length * bend;
        const fresh = markNew("edge", edge, index);
        const d = shouldCurve
          ? `M${startX},${startY} C${startX + normalX},${startY + normalY} ${endX + normalX},${endY + normalY} ${endX},${endY}`
          : `M${startX},${startY} L${endX},${endY}`;
        return `<path class="edge${fresh}${edgeFocusClass(edge)}" d="${d}" fill="none" stroke="${edgeColor(edge.kind)}" stroke-width="${state.selected?.id === edge.from || state.selected?.id === edge.to ? 3 : 1.8}" stroke-dasharray="${shouldCurve ? "8 9" : "0"}"/>`;
      }).join("");
    }
    function renderGraph(data) {
      const nodes = graphNodes(data);
      const points = layout(nodes, data);
      drawEdges(data, points);
      document.getElementById("nodes").innerHTML = nodes.map((node, i) => {
        const point = points.get(node.id);
        return `
        <button class="node ${node.kind}${focusClass(node.id)}${state.selected?.id === node.id ? " active" : ""}${state.nodeDrag?.id === node.id ? " grabbing" : ""}${markNew("node", node, i)}" style="left:${point[0]}px;top:${point[1]}px" data-kind="${node.kind}" data-id="${esc(node.id)}">
          <div class="kind">${esc(node.label)}</div>
          <div class="node-title">${esc(short(node.title, 150))}</div>
          <div class="node-meta">${esc(short(node.meta, 90))}</div>
        </button>`;
      }).join("");
      applyView();
      return points;
    }
    function renderMetrics(data) {
      const attempts = data.search_attempts || [];
      const okAttempts = attempts.filter(item => item.status === "ok").length;
      const agents = data.agent_runs || [];
      const roster = data.state.agent_roster || [];
      document.getElementById("strategy").innerHTML = [
        ["搜索等级", label(data.state.search_level)],
        ["搜索模式", label(data.state.search_provider)],
        ["编排", label(data.state.orchestration || "single_pipeline")],
        ["智能体", `${roster.length || new Set(agents.map(item => item.role)).size || 0} 个角色`],
        ["搜索面", `${okAttempts}/${attempts.length || providerCounts(data).length || 0} 可用`],
        ["目标来源", `${data.sources.length}/${data.state.target_accepted_sources || data.state.target_sources || data.state.max_sources || 0}`],
      ].map(item => `<span class="strategy-pill">${esc(item[0])}<strong>${esc(item[1])}</strong></span>`).join("");
      const values = [
        ["status", label(data.state.status)],
        ["sources", data.sources.length],
        ["nodes", data.graph?.nodes?.length || 0],
      ];
      document.getElementById("metrics").innerHTML = values.map(([metric, value]) => {
        const changed = state.counts[metric] !== undefined && state.counts[metric] !== value;
        return `<div class="metric${changed ? " bump" : ""}"><b>${esc(label(metric))}</b><span>${esc(value)}</span></div>`;
      }).join("");
      state.counts = { sources: data.sources.length, claims: data.claims.length, links: data.links.length, nodes: data.graph?.nodes?.length || 0, events: data.events.length };
      document.getElementById("sourceBar").style.width = Math.min(100, data.sources.length / Math.max(1, data.state.target_accepted_sources || data.state.target_sources || 18) * 100) + "%";
      document.getElementById("claimBar").style.width = Math.min(100, data.claims.length / 40 * 100) + "%";
      document.getElementById("linkBar").style.width = Math.min(100, (data.graph?.edges?.length || 0) / 120 * 100) + "%";
      document.getElementById("growthTitle").textContent = `${data.graph?.nodes?.length || 0} 个节点 · ${data.graph?.edges?.length || 0} 条边`;
    }
    function providerCounts(data) {
      const counts = new Map();
      for (const source of data.sources || []) {
        const key = source.source_type || source.provider || "unknown";
        counts.set(key, (counts.get(key) || 0) + 1);
      }
      return [...counts.entries()].sort((a, b) => b[1] - a[1]);
    }
    function renderWorkflow(data) {
      const sourceReady = (data.sources || []).length >= Math.max(1, data.state.min_accepted_sources || 1);
      const claimReady = (data.claims || []).length > 0;
      const linkReady = (data.graph?.edges?.length || 0) > 0;
      const reportReady = Boolean(data.report);
      const steps = [
        ["检索", `${label(data.state.search_level)} · ${label(data.state.search_provider)} · ${(data.search_attempts || []).length} 次`, (data.search_attempts || []).length > 0],
        ["筛选", `${providerCounts(data).length || 0} 个搜索面`, sourceReady],
        ["提取", `${data.claims.length} 条结论`, claimReady],
        ["交叉验证", `${data.graph?.edges?.length || 0} 条关系`, linkReady],
        ["报告", reportReady ? "可阅读" : "生成中", reportReady],
      ];
      const activeIndex = Math.max(0, steps.findIndex(step => !step[2]));
      document.getElementById("steps").innerHTML = steps.map((step, index) =>
        `<button class="step ${step[2] ? "done" : ""} ${index === activeIndex ? "active" : ""}" data-artifact="${index === 4 ? "report" : ""}">
          <b>${esc(step[0])}</b><span>${esc(step[1])}</span>
        </button>`
      ).join("");
    }
    function renderArtifacts(data) {
      const providers = providerCounts(data).map(([name, count]) => `${label(name)} ${count}`).join(" · ") || "暂无来源";
      const items = [
        ["搜索方式", `${label(data.state.search_level)} · ${label(data.state.search_provider)}`, "search"],
        ["智能体", `${data.agent_runs?.length || 0} 次运行`, "agents"],
        ["报告", data.report ? "最终产物" : "等待生成", "report"],
        ["证据包", `${data.sources.length} 来源`, "sources"],
        ["来源面", providers, "sources"],
        ["图谱快照", `${data.graph?.nodes?.length || 0} 节点`, "nodes"],
      ];
      document.getElementById("artifacts").innerHTML = items.map(item =>
        `<button class="artifact" data-artifact="${esc(item[2])}"><b>${esc(item[0])}</b><span>${esc(short(item[1], 34))}</span></button>`
      ).join("");
    }
    function facetsFor(data) {
      if (state.mode === "sources") return [["all", `全部 ${data.sources.length}`], ...providerCounts(data).map(([name, count]) => [name, `${label(name)} ${count}`])];
      if (state.mode === "search") {
        const attempts = data.search_attempts || [];
        const providers = new Map();
        for (const attempt of attempts) providers.set(attempt.provider || "unknown", (providers.get(attempt.provider || "unknown") || 0) + 1);
        return [["all", `全部 ${attempts.length}`], ...[...providers.entries()].map(([name, count]) => [name, `${label(name)} ${count}`])];
      }
      if (state.mode === "claims") {
        const statuses = new Map((data.claims || []).map(item => [item.status || "active", 0]));
        for (const claim of data.claims || []) statuses.set(claim.status || "active", (statuses.get(claim.status || "active") || 0) + 1);
        return [["all", `全部 ${data.claims.length}`], ...[...statuses.entries()].map(([name, count]) => [name, `${label(name)} ${count}`])];
      }
      if (state.mode === "agents") {
        const roles = new Map();
        for (const run of data.agent_runs || []) roles.set(run.role || "unknown", (roles.get(run.role || "unknown") || 0) + 1);
        return [["all", `全部 ${(data.agent_runs || []).length}`], ...[...roles.entries()].map(([name, count]) => [name, `${label(name)} ${count}`])];
      }
      return [["all", "全部"]];
    }
    function renderFacets(data) {
      const facets = facetsFor(data);
      if (!facets.some(facet => facet[0] === state.facet)) state.facet = "all";
      document.getElementById("facets").innerHTML = facets.map(facet =>
        `<button class="facet ${state.facet === facet[0] ? "active" : ""}" data-facet="${esc(facet[0])}">${esc(facet[1])}</button>`
      ).join("");
    }
    function renderPanelChrome() {
      const config = {
        graph: ["", ""],
        search: ["搜索方式", "查看搜索等级、搜索面、provider 尝试和候选源排序"],
        agents: ["智能体", "查看规划、检索、验证、阅读、关联、写作和复核的运行记录"],
        sources: ["来源", "只查看搜索抓取到的来源"],
        claims: ["结论", "只查看从来源中提取出的结论"],
        events: ["过程", "只查看搜索过程和持久化事件"],
        report: ["报告", "只查看最终可读报告"],
      }[state.mode] || ["", ""];
      document.getElementById("panelTitle").textContent = config[0];
      document.getElementById("panelSubtitle").textContent = config[1];
      document.getElementById("filter").style.display = ["search", "agents", "sources", "claims", "events"].includes(state.mode) ? "" : "none";
      document.getElementById("facets").style.display = ["search", "agents", "sources", "claims"].includes(state.mode) ? "" : "none";
    }
    function tabItems(data) {
      if (state.mode === "search") return [
        ...(data.search_attempts || []).map(item => ({...item, _kind: "search_attempt", title: `${label(item.provider)} · ${label(item.status)} · ${item.result_count} 条`, summary: item.query || item.error || ""})),
        ...(data.candidate_sources || []).map(item => ({...item, _kind: "candidate", title: `${label(item.provider)} · ${label(item.decision)} · score ${item.rerank_score}`, summary: item.title || item.url || ""})),
      ];
      if (state.mode === "agents") return (data.agent_runs || []).map(item => ({
        ...item,
        _kind: "agent_run",
        title: `${label(item.role)} · 第 ${item.round || 0} 轮 · ${label(item.status)}`,
        summary: [item.model ? `模型 ${item.model}` : "无模型调用", item.output_summary || ""].filter(Boolean).join(" · "),
      }));
      if (state.mode === "sources") return data.sources || [];
      if (state.mode === "claims") return data.claims || [];
      if (state.mode === "events") return data.events || [];
      if (state.mode === "report") return [{ id: "report", title: "最终报告", summary: data.report }];
      return [];
    }
    function renderTabs() {
      document.getElementById("tabs").innerHTML = "";
    }
    function renderList(data) {
      const query = document.getElementById("filter").value.toLowerCase();
      const items = tabItems(data).filter(item => {
        if (state.facet === "all") return true;
        if (state.mode === "search") return (item.provider || "unknown") === state.facet;
        if (state.mode === "agents") return (item.role || "unknown") === state.facet;
        if (state.mode === "sources") return (item.source_type || item.provider || "unknown") === state.facet;
        if (state.mode === "claims") return (item.status || "active") === state.facet;
        return true;
      }).filter(item => JSON.stringify(item).toLowerCase().includes(query));
      document.getElementById("list").innerHTML = items.length ? items.map((item, index) => {
        const id = item.id || item.url || item.time || "report";
        const title = item.title || item.text || item.type || "Report";
        const meta = item.url || item.source_ids?.join(", ") || item.time || item.summary || item.error || "";
        return `<button class="row${state.selected?.id === id ? " active" : ""}${markNew(state.mode, item, index)}" data-kind="${state.mode}" data-id="${esc(id)}">
          <div class="row-title">${esc(short(title, 130))}</div>
          <div class="row-meta">${esc(short(meta, 170))}</div>
        </button>`;
      }).join("") : `<div class="empty">没有匹配的${esc(label(state.mode))}。</div>`;
    }
    function renderEvents(data) {
      document.getElementById("events").innerHTML = (data.events || []).slice(-18).reverse().map((event, index) => `
        <button class="event${markNew("event", event, index)}" data-kind="events" data-id="${esc(event.time)}">
          <b>${esc(event.type)}</b><span>${esc(event.time)}</span>
        </button>`).join("");
    }
    function renderDetail(item, kind) {
      if (!item) {
        state.selected = null;
        state.related = new Set();
        document.getElementById("detail").classList.remove("open");
        document.getElementById("detail").innerHTML = "";
        if (state.data) renderGraph(state.data);
        return;
      }
      const title = item.title || item.text || item.type || "报告";
      const url = item.url ? `<p><a href="${esc(item.url)}" target="_blank" rel="noreferrer">${esc(item.url)}</a></p>` : "";
      const body = item.summary || item.report || item.text || item.rationale || item;
      document.getElementById("detail").classList.add("open");
      document.getElementById("detail").innerHTML = `
        <div class="detail-head">
          <div>
            <h2>${esc(title)}</h2>
            <div class="detail-kind">${esc(label(kind))}</div>
          </div>
          <button class="detail-close" data-detail-close title="关闭">×</button>
        </div>
        <div class="detail-body">${url}${detailBodyHtml(body)}</div>`;
    }
    function findItem(data, kind, id) {
      if (kind === "nodes") {
        const node = graphNodes(data).find(x => x.id === id);
        return node?.raw || node;
      }
      if (kind === "source" || kind === "sources") return (data.sources || []).find(x => x.id === id || x.url === id);
      if (kind === "search") return [...(data.search_attempts || []), ...(data.candidate_sources || [])].find(x => x.id === id);
      if (kind === "agents") return (data.agent_runs || []).find(x => x.id === id);
      if (kind === "claim" || kind === "claims") return (data.claims || []).find(x => x.id === id);
      if (kind === "lead") return (data.leads || []).find(x => x.id === id);
      if (kind === "events") return (data.events || []).find(x => x.time === id);
      if (kind === "report") return { id: "report", title: "最终报告", report: data.report };
      if (kind === "root") return { id: "root", title: data.state.topic, summary: JSON.stringify(data.state, null, 2) };
      return null;
    }
    function select(kind, id) {
      if (state.selected?.kind === kind && state.selected?.id === id) {
        renderDetail(null);
        renderList(state.data);
        return;
      }
      const item = findItem(state.data, kind, id);
      state.selected = { kind, id };
      state.related = relatedFor(state.data, id);
      renderDetail(item, kind);
      focusSelection(renderGraph(state.data));
      renderList(state.data);
    }
    async function tick() {
      const data = embeddedState || await fetch("/api/state").then(r => r.json());
      state.data = data;
      document.getElementById("topic").textContent = data.state.topic;
      renderMetrics(data);
      renderModebar();
      renderPanelChrome();
      renderWorkflow(data);
      renderArtifacts(data);
      renderFacets(data);
      renderGraph(data);
      renderList(data);
      renderEvents(data);
      if (!state.fitted) { fitView(); state.fitted = true; }
      if (!state.selected) renderDetail(null);
    }
    document.addEventListener("click", (event) => {
      if (state.suppressClick) return;
      const mode = event.target.closest("[data-mode]");
      if (mode) { setMode(mode.dataset.mode); return; }
      if (event.target.closest("[id=panelClose]")) { closePanel(); return; }
      if (event.target.closest("[data-detail-close]")) { renderDetail(null); renderList(state.data); return; }
      const facet = event.target.closest("[data-facet]");
      if (facet) { state.facet = facet.dataset.facet; renderFacets(state.data); renderList(state.data); return; }
      const artifact = event.target.closest("[data-artifact]");
      if (artifact?.dataset.artifact) { setMode(artifact.dataset.artifact === "report" ? "report" : "sources"); renderFacets(state.data); renderList(state.data); return; }
      const item = event.target.closest("[data-kind][data-id]");
      if (item) select(item.dataset.kind, item.dataset.id);
    });
    document.getElementById("filter").addEventListener("input", () => renderList(state.data));
    document.getElementById("zoomOut").addEventListener("click", () => setZoom(state.view.scale - .12));
    document.getElementById("zoomIn").addEventListener("click", () => setZoom(state.view.scale + .12));
    document.getElementById("zoomFit").addEventListener("click", fitView);
    document.getElementById("resetLayout").addEventListener("click", () => {
      state.manual.clear();
      localStorage.removeItem(`research:${topicId}:positions:v2`);
      renderGraph(state.data);
      fitView();
    });
    const stage = document.getElementById("stage");
    document.addEventListener("pointerdown", (event) => {
      const node = event.target.closest(".node[data-kind][data-id]");
      if (!node) return;
      event.preventDefault();
      event.stopPropagation();
      const left = Number.parseFloat(node.style.left) || 0;
      const top = Number.parseFloat(node.style.top) || 0;
      state.nodeDrag = { kind: node.dataset.kind, id: node.dataset.id, x: event.clientX, y: event.clientY, left, top, moved: false };
      node.classList.add("grabbing");
    });
    document.addEventListener("pointermove", (event) => {
      if (!state.nodeDrag) return;
      event.preventDefault();
      const dx = (event.clientX - state.nodeDrag.x) / state.view.scale;
      const dy = (event.clientY - state.nodeDrag.y) / state.view.scale;
      if (Math.abs(dx) + Math.abs(dy) > 3) state.nodeDrag.moved = true;
      state.manual.set(state.nodeDrag.id, [Math.round(state.nodeDrag.left + dx), Math.round(state.nodeDrag.top + dy)]);
      renderGraph(state.data);
    });
    document.addEventListener("pointerup", () => {
      if (!state.nodeDrag) return;
      if (state.nodeDrag.moved) {
        state.suppressClick = true;
        localStorage.setItem(`research:${topicId}:positions:v2`, JSON.stringify([...state.manual]));
        setTimeout(() => { state.suppressClick = false; }, 0);
      } else {
        const picked = state.nodeDrag;
        state.suppressClick = true;
        setTimeout(() => { state.suppressClick = false; }, 0);
        state.nodeDrag = null;
        select(picked.kind, picked.id);
        return;
      }
      state.nodeDrag = null;
      renderGraph(state.data);
    });
    document.addEventListener("pointercancel", () => {
      state.nodeDrag = null;
      renderGraph(state.data);
    });
    stage.addEventListener("wheel", (event) => {
      event.preventDefault();
      setZoom(state.view.scale * (event.deltaY > 0 ? .9 : 1.1), { x: event.clientX - stage.getBoundingClientRect().left, y: event.clientY - stage.getBoundingClientRect().top });
    }, { passive: false });
    stage.addEventListener("pointerdown", (event) => {
      if (event.target.closest("[data-kind][data-id]")) return;
      stage.setPointerCapture(event.pointerId);
      stage.classList.add("dragging");
      state.drag = { x: event.clientX, y: event.clientY, viewX: state.view.x, viewY: state.view.y };
    });
    stage.addEventListener("pointermove", (event) => {
      if (!state.drag) return;
      state.view.x = state.drag.viewX + event.clientX - state.drag.x;
      state.view.y = state.drag.viewY + event.clientY - state.drag.y;
      applyView();
    });
    stage.addEventListener("pointerup", () => { state.drag = null; stage.classList.remove("dragging"); });
    stage.addEventListener("pointercancel", () => { state.drag = null; stage.classList.remove("dragging"); });
    window.addEventListener("resize", () => { if (state.data) { renderGraph(state.data); fitView(); } });
    tick(); if (!embeddedState) setInterval(tick, 1200);
  </script>
</body>
</html>"#
    .replace("__TOPIC_ID__", topic_id)
    .replace(
        "__EMBEDDED_STATE__",
        &embedded_state
            .map(serialize_script_json)
            .transpose()?
            .unwrap_or_else(|| "null".to_string()),
    ))
}

fn serialize_script_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?
        .replace("</", "<\\/")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::extract_mcp_text;
    use serde_json::json;
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use tempfile::tempdir;

    #[test]
    fn start_resume_and_export_project() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        let started = run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Rust async runtimes".to_string(),
                topic_id: Some("rust-async".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(2),
                max_sources: Some(2),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Deep),
                initial_direction: vec!["Tokio history".to_string()],
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();
        assert_eq!(started["status"], "paused");
        assert_eq!(started["round"], 1);

        let resumed = run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Resume {
                topic_id: "rust-async".to_string(),
                focus: None,
                local_project: Vec::new(),
                model: None,
            },
        })
        .unwrap();
        assert_eq!(resumed["status"], "completed");
        assert_eq!(resumed["round"], 2);

        let exported = run(Cli {
            config: None,
            root: Some(root),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Export {
                topic_id: "rust-async".to_string(),
                format: ExportFormat::Report,
            },
        })
        .unwrap();
        assert!(exported["path"].as_str().unwrap().ends_with("report.md"));
    }

    #[test]
    fn repeated_directions_merge_leads() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Rust embedded adoption".to_string(),
                topic_id: Some("rust-embedded".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(0),
                max_sources: None,
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();
        for direction in [
            "Find specific Rust embedded adoption percentage data",
            "Search Rust embedded market share figures and adoption rate",
        ] {
            run(Cli {
                config: None,
                root: Some(root.clone()),
                search_provider: Some(SearchProvider::Deterministic),
                exa_api_key: None,
                json: true,
                command: Command::AddDirection {
                    topic_id: "rust-embedded".to_string(),
                    direction: direction.to_string(),
                    reason: None,
                    priority: Priority::High,
                },
            })
            .unwrap();
        }
        let leads = Lab::new(
            root,
            SearchProvider::Deterministic,
            None,
            ResearchConfig::default(),
        )
        .read_jsonl::<LeadRecord>("rust-embedded", "leads.jsonl")
        .unwrap();
        assert_eq!(leads.len(), 1);
        assert_eq!(leads[0].priority, Priority::High);
    }

    #[test]
    fn read_report_supports_paging() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "CRISPR regulation".to_string(),
                topic_id: Some("crispr".to_string()),
                quality_hint: Some(QualityHint::Academic),
                max_rounds: Some(1),
                max_sources: Some(1),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();
        let page = run(Cli {
            config: None,
            root: Some(root),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Read {
                topic_id: "crispr".to_string(),
                target: ReadTarget::Report,
                note_id: None,
                offset: 1,
                limit: 200,
            },
        })
        .unwrap();
        assert!(
            page["content"]
                .as_str()
                .unwrap()
                .contains("CRISPR regulation")
        );
        assert!(
            page["content"]
                .as_str()
                .unwrap()
                .contains("Executive Summary")
        );
    }

    #[test]
    fn source_collection_writes_checkpoint_events() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Checkpointed research".to_string(),
                topic_id: Some("checkpointed".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(1),
                max_sources: Some(1),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();

        let sources = read_to_string(root.join("checkpointed").join("sources.jsonl")).unwrap();
        assert!(sources.contains("Research seed"));
        let chunks = read_to_string(root.join("checkpointed").join("chunks.jsonl")).unwrap();
        assert!(chunks.contains("chunk_"));
        assert!(chunks.contains("Checkpointed research"));
        let candidates =
            read_to_string(root.join("checkpointed").join("candidate_sources.jsonl")).unwrap();
        assert!(candidates.contains("rerank_score"));
        let events =
            read_to_string(root.join("checkpointed").join("logs").join("events.jsonl")).unwrap();
        assert!(events.contains("source.checkpointed"));
    }

    #[test]
    fn local_project_search_can_supply_accepted_evidence() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        let project = dir.path().join("target-project");
        write_file(
            project.join("README.md"),
            "Research local project provider records local project evidence for the final report.",
        )
        .unwrap();

        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Exa),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "research local project provider".to_string(),
                topic_id: Some("local-provider".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(1),
                max_sources: Some(2),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: vec![project.clone()],
                resume_existing: false,
            },
        })
        .unwrap();

        let sources = read_to_string(root.join("local-provider").join("sources.jsonl")).unwrap();
        assert!(sources.contains("local_project"));
        assert!(sources.contains("README.md"));
        let state: State = serde_json::from_str(
            &read_to_string(root.join("local-provider").join("state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(state.local_project_paths, vec![project]);
    }

    #[test]
    fn start_persists_graph_and_exposes_it_in_viewer_state() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Graph refactor".to_string(),
                topic_id: Some("graph-refactor".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(1),
                max_sources: Some(1),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();

        let graph: Value = serde_json::from_str(
            &read_to_string(root.join("graph-refactor").join("graph.json")).unwrap(),
        )
        .unwrap();
        let nodes = graph["nodes"].as_array().unwrap();
        assert!(nodes.iter().any(|node| node["kind"] == "root"));
        assert!(nodes.iter().any(|node| node["kind"] == "source"));
        assert!(nodes.iter().any(|node| node["kind"] == "claim"));
        assert!(nodes.iter().any(|node| node["kind"] == "lead"));

        let viewer = Lab::new(
            root,
            SearchProvider::Deterministic,
            None,
            ResearchConfig::default(),
        )
        .viewer_state("graph-refactor")
        .unwrap();
        assert_eq!(viewer["graph"], graph);
        assert_eq!(
            viewer["state"]["orchestration"],
            "single_pass_agent_pipeline"
        );
        assert_eq!(viewer["state"]["agent_roster"].as_array().unwrap().len(), 6);
        let agent_runs = viewer["agent_runs"].as_array().unwrap();
        assert!(agent_runs.iter().any(|run| run["role"] == "planner"));
        assert!(agent_runs.iter().any(|run| run["role"] == "searcher"));
        assert!(agent_runs.iter().any(|run| run["role"] == "verifier"));
        assert!(agent_runs.iter().any(|run| run["role"] == "reader"));
        assert!(agent_runs.iter().any(|run| run["role"] == "linker"));
        assert!(agent_runs.iter().any(|run| run["role"] == "writer"));
    }

    #[test]
    fn merged_leads_are_reflected_as_single_graph_node() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Lead graph sync".to_string(),
                topic_id: Some("lead-graph".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(0),
                max_sources: None,
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();

        for direction in [
            "Find specific lead graph sync data",
            "Search lead graph sync figures and additional data",
        ] {
            run(Cli {
                config: None,
                root: Some(root.clone()),
                search_provider: Some(SearchProvider::Deterministic),
                exa_api_key: None,
                json: true,
                command: Command::AddDirection {
                    topic_id: "lead-graph".to_string(),
                    direction: direction.to_string(),
                    reason: None,
                    priority: Priority::High,
                },
            })
            .unwrap();
        }

        let graph: Value = serde_json::from_str(
            &read_to_string(root.join("lead-graph").join("graph.json")).unwrap(),
        )
        .unwrap();
        let lead_nodes = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["kind"] == "lead")
            .count();
        let lead_edges = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| edge["kind"] == "tracks_lead")
            .count();
        assert_eq!(lead_nodes, 1);
        assert_eq!(lead_edges, 1);
    }

    #[test]
    fn save_state_does_not_rebuild_graph_from_jsonl_artifacts() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Graph persistence".to_string(),
                topic_id: Some("graph-persistence".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(0),
                max_sources: None,
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();

        let lab = Lab::new(
            root,
            SearchProvider::Deterministic,
            None,
            ResearchConfig::default(),
        );
        lab.write_jsonl(
            "graph-persistence",
            "claims.jsonl",
            &vec![ClaimRecord::new(
                "Artifact-only claim".to_string(),
                vec!["src-manual".to_string()],
                Vec::new(),
            )],
        )
        .unwrap();

        let state = lab.load_state("graph-persistence").unwrap();
        lab.save_state(&state).unwrap();

        let graph = lab.load_graph("graph-persistence").unwrap();
        assert_eq!(
            graph
                .nodes
                .iter()
                .filter(|node| node.kind == "claim")
                .count(),
            0
        );
    }

    #[test]
    fn config_commands_drive_cli_defaults() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config.json");
        let root = dir.path().join("configured-root");
        for (key, value) in [
            ("root", root.to_str().unwrap()),
            ("search_provider", "deterministic"),
            ("default_level", "quick"),
            ("default_quality_hint", "academic"),
            ("max_rounds", "0"),
            ("max_sources", "3"),
        ] {
            run(Cli {
                config: Some(config.clone()),
                root: None,
                search_provider: None,
                exa_api_key: None,
                json: true,
                command: Command::Config {
                    command: ConfigCommand::Set {
                        key: key.to_string(),
                        value: value.to_string(),
                    },
                },
            })
            .unwrap();
        }

        let started = run(Cli {
            config: Some(config.clone()),
            root: None,
            search_provider: None,
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Configured research".to_string(),
                topic_id: Some("configured".to_string()),
                quality_hint: None,
                max_rounds: None,
                max_sources: None,
                max_runtime_minutes: None,
                model: None,
                level: None,
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();
        assert_eq!(started["status"], "paused");
        assert_eq!(started["search_level"], "quick");
        assert!(root.join("configured").join("state.json").exists());

        let shown = run(Cli {
            config: Some(config),
            root: None,
            search_provider: None,
            exa_api_key: None,
            json: true,
            command: Command::Config {
                command: ConfigCommand::Show,
            },
        })
        .unwrap();
        assert_eq!(
            shown["config"]["profiles"]["default"]["max_accepted_sources"],
            3
        );
    }

    #[test]
    fn evidence_gate_rejects_wrong_identifier_candidates() {
        let state = State {
            topic_id: "rfc-9110".to_string(),
            topic: "RFC-9110 HTTP semantics".to_string(),
            status: Status::Running,
            quality_hint: QualityHint::General,
            created_at: now(),
            updated_at: now(),
            current_round: 1,
            max_rounds: 1,
            max_sources: 5,
            max_runtime_minutes: 5,
            min_accepted_sources: 2,
            target_accepted_sources: 5,
            stop_when_confident: true,
            stop_on_no_new_sources: true,
            research_model: None,
            search_provider: SearchProvider::Hybrid,
            search_level: SearchLevel::Quick,
            local_project_paths: Vec::new(),
            viewer_url: None,
            orchestration: "single_pass_agent_pipeline".to_string(),
            agent_roster: vec![
                "planner".to_string(),
                "searcher".to_string(),
                "verifier".to_string(),
                "reader".to_string(),
                "linker".to_string(),
                "writer".to_string(),
            ],
            active_directions: Vec::new(),
            source_count: 0,
            note_count: 0,
            claim_count: 0,
            link_count: 0,
            open_leads: Vec::new(),
            last_error: None,
        };
        let rejected = evidence_gate(
            &state,
            "RFC-9110 overview primary sources",
            &SearchHit {
                provider: SearchProvider::Minimax.to_string(),
                url: "https://example.com/rfc-2616".to_string(),
                title: "RFC-2616 HTTP/1.1".to_string(),
                summary: "Older HTTP semantics document superseded by newer references."
                    .to_string(),
                text: String::new(),
                published_date: None,
                author: None,
            },
            &[],
        );
        assert!(!rejected.accepted);

        let accepted = evidence_gate(
            &state,
            "RFC-9110 overview primary sources",
            &SearchHit {
                provider: SearchProvider::Exa.to_string(),
                url: "https://www.rfc-editor.org/rfc/rfc9110".to_string(),
                title: "RFC 9110: HTTP Semantics".to_string(),
                summary: "RFC-9110 defines HTTP semantics and core protocol behavior.".to_string(),
                text: String::new(),
                published_date: None,
                author: None,
            },
            &[],
        );
        assert!(accepted.accepted);
    }

    #[test]
    fn evidence_gate_novel_domain_bonus() {
        let state = State {
            topic_id: "test".to_string(),
            topic: "rust async".to_string(),
            status: Status::Running,
            quality_hint: QualityHint::General,
            created_at: now(),
            updated_at: now(),
            current_round: 1,
            max_rounds: 1,
            max_sources: 5,
            max_runtime_minutes: 5,
            min_accepted_sources: 2,
            target_accepted_sources: 5,
            stop_when_confident: true,
            stop_on_no_new_sources: true,
            research_model: None,
            search_provider: SearchProvider::Hybrid,
            search_level: SearchLevel::Quick,
            local_project_paths: Vec::new(),
            viewer_url: None,
            orchestration: "single_pass_agent_pipeline".to_string(),
            agent_roster: vec![
                "planner".to_string(),
                "searcher".to_string(),
                "verifier".to_string(),
                "reader".to_string(),
                "linker".to_string(),
                "writer".to_string(),
            ],
            active_directions: Vec::new(),
            source_count: 0,
            note_count: 0,
            claim_count: 0,
            link_count: 0,
            open_leads: Vec::new(),
            last_error: None,
        };
        let hit = SearchHit {
            provider: SearchProvider::Exa.to_string(),
            url: "https://example.com/rust-async".to_string(),
            title: "Async Rust".to_string(),
            summary: "tokio async runtime".to_string(),
            text: "async/await in Rust".to_string(),
            published_date: None,
            author: None,
        };
        // Without accepted domains — should get novelty bonus
        let gate = evidence_gate(&state, "rust async", &hit, &[]);
        assert!(
            gate.reasons.iter().any(|r| r.contains("novel domain")),
            "should get novelty bonus: {:?}",
            gate.reasons
        );
        // With same domain already accepted — no novelty bonus
        let gate2 = evidence_gate(&state, "rust async", &hit, &["example.com".to_string()]);
        assert!(
            !gate2.reasons.iter().any(|r| r.contains("novel domain")),
            "should NOT get novelty bonus: {:?}",
            gate2.reasons
        );
    }

    #[test]
    fn code_surface_config_is_canonical_and_auto_detects_technical_topics() {
        let mut config = ResearchConfig::default();
        config
            .set("search.providers.exa.api_key", "exa-key")
            .unwrap();
        config.set("search.providers.code.enabled", "true").unwrap();
        config.set("search.providers.code.auto", "true").unwrap();
        config
            .set("search.providers.code.tokens_num", "8000")
            .unwrap();
        assert_eq!(config.search.providers.code.api_key, None);
        let lab = Lab::new(
            PathBuf::from("unused"),
            SearchProvider::Hybrid,
            config.exa_api_key.clone(),
            config.clone(),
        );
        assert_eq!(lab.code_api_key().as_deref(), Some("exa-key"));
        assert_eq!(config.search.providers.code.enabled, Some(true));
        assert_eq!(config.search.providers.code.auto, Some(true));
        assert_eq!(config.search.providers.code.tokens_num, Some(8000));
        assert!(is_technical_research_text(
            "React Server Components cache API migration"
        ));
        assert!(!is_technical_research_text(
            "Benin Bronzes museum repatriation policy"
        ));
    }

    #[test]
    fn mcp_text_extraction_reads_exa_code_context_shape() {
        let value = json!({
            "result": {
                "content": [
                    { "type": "text", "text": "Use the official SDK client." }
                ]
            }
        });
        assert_eq!(
            extract_mcp_text(&value),
            "Use the official SDK client.".to_string()
        );
    }

    #[test]
    fn search_profiles_are_configurable_strategy_defaults() {
        let lab = Lab::new(
            PathBuf::from("unused"),
            SearchProvider::Deterministic,
            None,
            ResearchConfig {
                deep_min_accepted_sources: Some(8),
                deep_max_accepted_sources: Some(24),
                deep_max_rounds: Some(5),
                ..ResearchConfig::default()
            },
        );
        let quick = lab.search_profile(SearchLevel::Quick);
        assert_eq!(quick.min_accepted_sources, 2);
        assert_eq!(quick.max_accepted_sources, 6);
        assert!(quick.stop_when_confident);

        let deep = lab.search_profile(SearchLevel::Deep);
        assert_eq!(deep.min_accepted_sources, 8);
        assert_eq!(deep.max_accepted_sources, 24);
        assert_eq!(deep.max_rounds, 5);

        let research = lab.search_profile(SearchLevel::Research);
        assert_eq!(research.max_rounds, 8);
        assert!(!research.stop_when_confident);
    }

    #[test]
    fn viewer_html_replaces_topic_id_placeholder() {
        let html = viewer_html("my-research-topic", None).unwrap();
        assert!(html.contains("my-research-topic"));
        assert!(!html.contains("__TOPIC_ID__"));
        assert!(html.contains("<title>research · my-research-topic</title>"));
    }

    #[test]
    fn viewer_html_injects_embedded_state() {
        let state = json!({"topic": "test", "status": "running"});
        let html = viewer_html("t", Some(&state)).unwrap();
        assert!(html.contains("\"topic\":\"test\""));
        assert!(!html.contains("__EMBEDDED_STATE__"));
    }

    #[test]
    fn viewer_html_null_embedded_state_becomes_literal_null() {
        let html = viewer_html("t", None).unwrap();
        // The JS code expects: const embeddedState = null;
        assert!(html.contains("const embeddedState = null;"));
    }

    #[test]
    fn viewer_html_escapes_closing_script_tag_in_embedded_state() {
        let state = json!({"html": "</script>"});
        let html = viewer_html("t", Some(&state)).unwrap();
        // serialize_script_json escapes "</" to "<\/"
        assert!(html.contains("<\\/script>"));
        assert!(!html.contains("</script>\"}"));
    }

    #[test]
    fn viewer_state_returns_all_expected_keys_after_start() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "Viewer keys test".to_string(),
                topic_id: Some("viewer-keys".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(1),
                max_sources: Some(1),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();

        let viewer = Lab::new(
            root,
            SearchProvider::Deterministic,
            None,
            ResearchConfig::default(),
        )
        .viewer_state("viewer-keys")
        .unwrap();

        let expected_keys = [
            "state",
            "graph",
            "sources",
            "chunks",
            "search_attempts",
            "agent_runs",
            "candidate_sources",
            "accepted_sources",
            "rejected_sources",
            "claims",
            "links",
            "leads",
            "events",
            "report",
            "resume_summary",
        ];
        for key in &expected_keys {
            assert!(viewer.get(key).is_some(), "missing key: {key}");
        }
    }

    #[test]
    fn handle_http_routes_api_state_to_json() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "HTTP routing test".to_string(),
                topic_id: Some("http-route".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(1),
                max_sources: Some(1),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();

        let lab = Lab::new(
            root,
            SearchProvider::Deterministic,
            None,
            ResearchConfig::default(),
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        let server = listener.accept().unwrap().0;
        drop(listener);

        // Send HTTP request for /api/state
        let mut client = client;
        client.write_all(b"GET /api/state HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();

        lab.handle_http(server, "http-route").unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("application/json"));
        // Body should contain viewer state JSON
        assert!(response.contains("\"state\""));
        assert!(response.contains("\"graph\""));
    }

    #[test]
    fn handle_http_routes_root_to_html() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("research");
        run(Cli {
            config: None,
            root: Some(root.clone()),
            search_provider: Some(SearchProvider::Deterministic),
            exa_api_key: None,
            json: true,
            command: Command::Start {
                topic: "HTML routing test".to_string(),
                topic_id: Some("html-route".to_string()),
                quality_hint: Some(QualityHint::General),
                max_rounds: Some(1),
                max_sources: Some(1),
                max_runtime_minutes: None,
                model: None,
                level: Some(SearchLevel::Quick),
                initial_direction: Vec::new(),
                local_project: Vec::new(),
                resume_existing: false,
            },
        })
        .unwrap();

        let lab = Lab::new(
            root,
            SearchProvider::Deterministic,
            None,
            ResearchConfig::default(),
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        let server = listener.accept().unwrap().0;
        drop(listener);

        let mut client = client;
        client.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();

        lab.handle_http(server, "html-route").unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("text/html"));
        assert!(response.contains("<!doctype html>"));
        assert!(response.contains("html-route"));
    }
}
