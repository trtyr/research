use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::utils::now;

pub(crate) fn artifact_entry(path: impl AsRef<Path>) -> Value {
    let path = path.as_ref();
    json!({
        "path": path,
        "exists": path.exists(),
    })
}

pub(crate) fn is_optional_jsonl_target(target: crate::ReadTarget) -> bool {
    matches!(
        target,
        crate::ReadTarget::CandidateSources
            | crate::ReadTarget::AcceptedSources
            | crate::ReadTarget::RejectedSources
            | crate::ReadTarget::SearchAttempts
            | crate::ReadTarget::AgentRuns
            | crate::ReadTarget::Claims
            | crate::ReadTarget::Leads
            | crate::ReadTarget::Links
            | crate::ReadTarget::Evaluations
            | crate::ReadTarget::ClaimEvents
            | crate::ReadTarget::Answers
            | crate::ReadTarget::Events
    )
}

pub(crate) fn read_target_name(target: crate::ReadTarget) -> &'static str {
    match target {
        crate::ReadTarget::State => "state",
        crate::ReadTarget::Agenda => "agenda",
        crate::ReadTarget::Questions => "questions",
        crate::ReadTarget::Sources => "sources",
        crate::ReadTarget::Chunks => "chunks",
        crate::ReadTarget::CandidateSources => "candidate_sources",
        crate::ReadTarget::AcceptedSources => "accepted_sources",
        crate::ReadTarget::RejectedSources => "rejected_sources",
        crate::ReadTarget::SearchAttempts => "search_attempts",
        crate::ReadTarget::AgentRuns => "agent_runs",
        crate::ReadTarget::Notes => "notes",
        crate::ReadTarget::Claims => "claims",
        crate::ReadTarget::Evidence => "evidence",
        crate::ReadTarget::Entities => "entities",
        crate::ReadTarget::Links => "links",
        crate::ReadTarget::Insights => "insights",
        crate::ReadTarget::Leads => "leads",
        crate::ReadTarget::Timeline => "timeline",
        crate::ReadTarget::Gaps => "gaps",
        crate::ReadTarget::Evaluations => "evaluations",
        crate::ReadTarget::Decisions => "decisions",
        crate::ReadTarget::ResumeSummary => "resume_summary",
        crate::ReadTarget::ClaimEvents => "claim_events",
        crate::ReadTarget::Answers => "answers",
        crate::ReadTarget::Events => "events",
        crate::ReadTarget::Threads => "threads",
        crate::ReadTarget::Refine => "refine",
        crate::ReadTarget::Report => "report",
        crate::ReadTarget::ReportReview => "report_review",
        crate::ReadTarget::Plan => "plan",
    }
}

pub(crate) fn write_file(path: impl AsRef<Path>, content: &str) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn read_to_string(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

pub(crate) fn read_dir_names(path: impl AsRef<Path>) -> Result<Vec<String>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path).with_context(|| format!("failed to list {}", path.display()))? {
        let entry = entry?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

#[derive(Clone, Debug)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn project_root(&self, topic_id: &str) -> PathBuf {
        self.root.join(topic_id)
    }

    pub fn read_jsonl<T: for<'de> Deserialize<'de>>(
        &self,
        topic_id: &str,
        file: &str,
    ) -> Result<Vec<T>> {
        let path = self.project_root(topic_id).join(file);
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(read_to_string(path)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .enumerate()
            .filter_map(|(i, line)| match serde_json::from_str::<T>(line) {
                Ok(val) => Some(val),
                Err(e) => {
                    eprintln!(
                        "Warning: skipping malformed JSONL line {}: {} (content: {:.100})",
                        i + 1,
                        e,
                        line
                    );
                    None
                }
            })
            .collect())
    }

    pub fn write_jsonl<T: Serialize>(&self, topic_id: &str, file: &str, items: &[T]) -> Result<()> {
        write_file(
            self.project_root(topic_id).join(file),
            &items
                .iter()
                .map(serde_json::to_string)
                .collect::<std::result::Result<Vec<_>, _>>()?
                .join("\n"),
        )
    }

    pub fn append_jsonl<T: Serialize>(&self, topic_id: &str, file: &str, value: &T) -> Result<()> {
        let path = self.project_root(topic_id).join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        use std::io::Write as _;
        writeln!(file, "{}", serde_json::to_string(value)?)?;
        Ok(())
    }

    pub fn event(&self, topic_id: &str, event_type: &str, data: Value) -> Result<()> {
        self.append_jsonl(
            topic_id,
            "logs/events.jsonl",
            &json!({
                "time": now(),
                "type": event_type,
                "data": data,
            }),
        )
    }
}
