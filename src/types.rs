use std::path::PathBuf;

use crate::{AgentRole, QualityHint, SearchLevel};

pub struct StartInput {
    pub topic: String,
    pub topic_id: Option<String>,
    pub quality_hint: Option<QualityHint>,
    pub max_rounds: Option<u32>,
    pub max_sources: Option<u32>,
    pub max_runtime_minutes: Option<u32>,
    pub model: Option<String>,
    pub level: Option<SearchLevel>,
    pub initial_directions: Vec<String>,
    pub local_project_paths: Vec<PathBuf>,
    pub resume_existing: bool,
}

#[derive(Clone, Debug)]
pub struct SearchProfile {
    pub min_accepted_sources: u32,
    pub max_accepted_sources: u32,
    pub max_rounds: u32,
    pub max_runtime_minutes: u32,
    pub stop_when_confident: bool,
    pub stop_on_no_new_sources: bool,
    pub orchestration: &'static str,
    pub agents: Vec<AgentRole>,
}
