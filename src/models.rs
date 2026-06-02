use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Priority, QualityHint, SearchLevel, SearchProvider, Status};
use crate::utils::{clean_text, id, normalize, now};


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    pub topic_id: String,
    pub topic: String,
    pub status: Status,
    pub quality_hint: QualityHint,
    pub created_at: String,
    pub updated_at: String,
    pub current_round: u32,
    pub max_rounds: u32,
    pub max_sources: u32,
    pub max_runtime_minutes: u32,
    #[serde(default)]
    pub min_accepted_sources: u32,
    #[serde(default)]
    pub target_accepted_sources: u32,
    #[serde(default)]
    pub stop_when_confident: bool,
    #[serde(default)]
    pub stop_on_no_new_sources: bool,
    pub research_model: Option<String>,
    #[serde(default)]
    pub search_provider: SearchProvider,
    #[serde(default)]
    pub search_level: SearchLevel,
    #[serde(default)]
    pub local_project_paths: Vec<PathBuf>,
    #[serde(default)]
    pub viewer_url: Option<String>,
    #[serde(default)]
    pub orchestration: String,
    #[serde(default)]
    pub agent_roster: Vec<String>,
    pub active_directions: Vec<String>,
    pub source_count: usize,
    pub note_count: usize,
    pub claim_count: usize,
    pub link_count: usize,
    pub open_leads: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceRecord {
    pub id: String,
    pub note_id: Option<String>,
    pub content_key: Option<String>,
    pub url: String,
    pub title: String,
    pub query: String,
    pub quality_hint: QualityHint,
    pub source_type: String,
    pub found_at: String,
    pub fetched_at: Option<String>,
    pub status: String,
    pub summary: String,
    pub short_excerpt: Option<String>,
    pub credibility_score: f32,
    pub source_authority: String,
    pub source_freshness: String,
    pub bias_risk: String,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub provider: String,
    pub url: String,
    pub title: String,
    pub summary: String,
    pub text: String,
    pub published_date: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RankedSearchHit {
    pub hit: SearchHit,
    pub rank: usize,
    pub rerank_score: u32,
    pub rerank_reasons: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CandidateSourceRecord {
    pub id: String,
    pub provider: String,
    pub query: String,
    pub url: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub rank: usize,
    #[serde(default)]
    pub rerank_score: u32,
    #[serde(default)]
    pub rerank_reasons: Vec<String>,
    pub score: u32,
    pub decision: String,
    pub reasons: Vec<String>,
    pub found_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceChunkRecord {
    pub id: String,
    pub source_id: String,
    pub chunk_index: usize,
    pub query: String,
    pub url: String,
    pub title: String,
    pub text: String,
    pub token_count: usize,
    pub relevance_score: f32,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SearchAttemptRecord {
    pub id: String,
    pub provider: String,
    pub query: String,
    pub status: String,
    pub result_count: usize,
    pub error: Option<String>,
    #[serde(default)]
    pub accepted_source_count: usize,
    #[serde(default)]
    pub was_productive: bool,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Planner,
    Searcher,
    Verifier,
    Reader,
    Linker,
    Writer,
    Reviewer,
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AgentRole::Planner => "planner",
            AgentRole::Searcher => "searcher",
            AgentRole::Verifier => "verifier",
            AgentRole::Reader => "reader",
            AgentRole::Linker => "linker",
            AgentRole::Writer => "writer",
            AgentRole::Reviewer => "reviewer",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentRunRecord {
    pub id: String,
    pub role: AgentRole,
    pub round: u32,
    pub status: String,
    pub model: Option<String>,
    pub input_summary: String,
    pub output_summary: String,
    pub artifact_paths: Vec<String>,
    pub started_at: String,
    pub completed_at: String,
}

#[derive(Clone, Debug)]
pub struct GateDecision {
    pub accepted: bool,
    pub score: u32,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClaimRecord {
    pub id: String,
    pub text: String,
    pub source_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: String,
    pub status: String,
    pub tags: Vec<String>,
    pub merged_from: Vec<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

impl ClaimRecord {
    pub(crate) fn new(text: String, source_ids: Vec<String>, tags: Vec<String>) -> Self {
        Self {
            id: id("claim"),
            text: clean_text(&text),
            source_ids,
            evidence_ids: Vec::new(),
            confidence: "low".to_string(),
            status: "active".to_string(),
            tags,
            merged_from: Vec::new(),
            created_at: now(),
            updated_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LeadRecord {
    pub id: String,
    pub direction: String,
    pub reason: Option<String>,
    pub priority: Priority,
    pub kind: String,
    pub expected_information_gain: f32,
    pub evaluation: String,
    pub status: String,
    pub merged_from: Vec<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

impl LeadRecord {
    pub(crate) fn new(direction: String, reason: Option<String>, priority: Priority) -> Self {
        let kind = if normalize(&format!("{} {}", direction, reason.clone().unwrap_or_default()))
            .contains("contradict")
        {
            "breakthrough"
        } else {
            "fill_gap"
        }
        .to_string();
        let expected_information_gain = match priority {
            Priority::High => 0.72,
            Priority::Normal => 0.58,
            Priority::Low => 0.34,
        };
        Self {
            id: id("lead"),
            direction: clean_text(&direction),
            reason,
            priority,
            kind: kind.clone(),
            expected_information_gain,
            evaluation: format!(
                "{kind} lead with deterministic estimated information gain {expected_information_gain}."
            ),
            status: "open".to_string(),
            merged_from: Vec::new(),
            created_at: now(),
            updated_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LinkRecord {
    pub id: String,
    pub from: String,
    pub to: String,
    pub link_type: String,
    pub rationale: String,
    pub source_ids: Vec<String>,
    pub confidence: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct GraphData {
    pub root_id: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct GraphEdge {
    pub id: String,
    pub kind: String,
    pub from: String,
    pub to: String,
    pub rationale: Option<String>,
    pub created_at: String,
}
