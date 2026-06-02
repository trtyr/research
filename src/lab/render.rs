use super::*;

impl Lab {
    pub(crate) fn render_claims(&self, topic_id: &str) -> Result<()> {
        let claims = self.read_jsonl::<ClaimRecord>(topic_id, "claims.jsonl")?;
        write_file(
            self.project_root(topic_id).join("claims.md"),
            &format!(
                "# Claims\n\n{}",
                claims
                    .iter()
                    .map(|claim| format!(
                        "- **{}** ({}, {}) {} [sources: {}]",
                        claim.id,
                        claim.confidence,
                        claim.status,
                        claim.text,
                        claim.source_ids.join(", ")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .if_empty("No claims recorded yet."),
            ),
        )
    }

    pub(crate) fn render_links(&self, topic_id: &str) -> Result<()> {
        let links = self.read_jsonl::<LinkRecord>(topic_id, "links.jsonl")?;
        write_file(
            self.project_root(topic_id).join("links.md"),
            &format!(
                "# Links\n\n{}",
                links
                    .iter()
                    .map(|link| format!(
                        "- **{}** {} {} {}: {}",
                        link.id, link.from, link.link_type, link.to, link.rationale
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .if_empty("No relationship edges recorded yet."),
            ),
        )
    }

    pub(crate) fn render_insights(&self, topic_id: &str) -> Result<()> {
        let links = self.read_jsonl::<LinkRecord>(topic_id, "links.jsonl")?;
        write_file(
            self.project_root(topic_id).join("insights.md"),
            &format!(
                "# Insights\n\n{}",
                links
                    .iter()
                    .filter(|link| link.link_type == "contradicts" || link.link_type == "causes")
                    .map(|link| format!("## {} {}\n\n{}", link.link_type, link.id, link.rationale))
                    .collect::<Vec<_>>()
                    .join("\n\n")
                    .if_empty("No insights recorded yet."),
            ),
        )
    }

    pub(crate) fn render_leads(&self, topic_id: &str) -> Result<()> {
        let leads = self.read_jsonl::<LeadRecord>(topic_id, "leads.jsonl")?;
        write_file(
            self.project_root(topic_id).join("threads.md"),
            &format!(
                "# Threads\n\n## Open Leads\n\n{}",
                leads
                    .iter()
                    .filter(|lead| lead.status == "open")
                    .map(|lead| format!(
                        "- **{} / {} / gain={}** {}{}",
                        lead.priority,
                        lead.kind,
                        lead.expected_information_gain,
                        lead.direction,
                        lead.reason
                            .as_ref()
                            .map(|reason| format!(" - {reason}"))
                            .unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .if_empty("- No open leads recorded yet."),
            ),
        )
    }

    pub(crate) fn render_answers(&self, topic_id: &str) -> Result<()> {
        let answers = self.read_jsonl::<Value>(topic_id, "answers.jsonl")?;
        write_file(
            self.project_root(topic_id).join("answers.md"),
            &format!(
                "# Answers\n\n{}",
                answers
                    .iter()
                    .map(|answer| format!(
                        "## {}\n\n{}",
                        answer
                            .get("question")
                            .and_then(Value::as_str)
                            .unwrap_or("Question"),
                        answer.get("answer").and_then(Value::as_str).unwrap_or("")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n\n")
                    .if_empty("No answers recorded yet."),
            ),
        )
    }

    pub(crate) fn render_resume_summary(&self, state: &State) -> Result<()> {
        let leads = self.read_jsonl::<LeadRecord>(&state.topic_id, "leads.jsonl")?;
        write_file(
            self.project_root(&state.topic_id).join("resume_summary.md"),
            &[
                format!("# Resume Summary: {}", state.topic),
                String::new(),
                format!("- Status: {}", state.status),
                format!("- Round: {}/{}", state.current_round, state.max_rounds),
                format!("- Orchestration: {}", state.orchestration),
                format!(
                    "- Agents: {}",
                    state.agent_roster.join(", ").if_empty("none")
                ),
                format!(
                    "- Sources / Claims / Links: {} / {} / {}",
                    state.source_count, state.claim_count, state.link_count
                ),
                String::new(),
                "## Recommended Next Leads".to_string(),
                String::new(),
                leads
                    .iter()
                    .filter(|lead| lead.status == "open")
                    .take(5)
                    .map(|lead| {
                        format!(
                            "- **{} / gain={}** {}",
                            lead.kind, lead.expected_information_gain, lead.direction
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .if_empty("- No open leads recorded."),
            ]
            .join("\n"),
        )
    }

    pub(crate) fn write_plan(&self, state: &State) -> Result<()> {
        write_file(
            self.project_root(&state.topic_id).join("plan.md"),
            &[
                format!("# {}", state.topic),
                String::new(),
                format!("- Topic ID: `{}`", state.topic_id),
                format!("- Status: {}", state.status),
                format!("- Quality hint: {}", state.quality_hint),
                format!("- Search level: {}", state.search_level),
                format!("- Search provider: {}", state.search_provider),
                format!("- Orchestration: {}", state.orchestration),
                format!(
                    "- Agent roster: {}",
                    state.agent_roster.join(", ").if_empty("none")
                ),
                format!(
                    "- Local projects: {}",
                    state
                        .local_project_paths
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                        .if_empty("none")
                ),
                format!(
                    "- Budget: up to {} rounds, target {} accepted sources, {} minutes",
                    state.max_rounds,
                    effective_target_sources(state),
                    state.max_runtime_minutes
                ),
                format!(
                    "- Stop policy: min accepted={}, stop when confident={}, stop on no new sources={}",
                    state.min_accepted_sources,
                    state.stop_when_confident,
                    state.stop_on_no_new_sources
                ),
                String::new(),
                "## Active Directions".to_string(),
                String::new(),
                state
                    .active_directions
                    .iter()
                    .map(|item| format!("- {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .if_empty("- General overview"),
            ]
            .join("\n"),
        )
    }

    pub(crate) fn render_agenda(&self, state: &State, queries: &[String]) -> String {
        [
            format!("# Agenda: {}", state.topic),
            String::new(),
            "## Next Queries".to_string(),
            String::new(),
            queries
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
                .if_empty("- No queries planned."),
            String::new(),
            "## Planning Decision".to_string(),
            String::new(),
            "Rust CLI deterministic planner selected queries from active directions, focus, and open leads.".to_string(),
        ]
        .join("\n")
    }

    pub(crate) fn render_note(&self, state: &State, source: &SourceRecord, hit: &SearchHit) -> String {
        [
            format!("# {}", source.title),
            String::new(),
            format!("- Source ID: `{}`", source.id),
            format!("- URL: {}", source.url),
            format!("- Query: {}", source.query),
            format!("- Accessed: {}", source.found_at),
            hit.published_date
                .as_ref()
                .map(|date| format!("- Published: {date}"))
                .unwrap_or_default(),
            hit.author
                .as_ref()
                .map(|author| format!("- Author: {author}"))
                .unwrap_or_default(),
            String::new(),
            "## Summary".to_string(),
            String::new(),
            source.summary.clone(),
            String::new(),
            "## Short Excerpt".to_string(),
            String::new(),
            source.short_excerpt.clone().unwrap_or_default(),
            String::new(),
            "## Retrieved Text".to_string(),
            String::new(),
            hit.text.chars().take(12_000).collect::<String>(),
            String::new(),
            "## Topic".to_string(),
            String::new(),
            state.topic.clone(),
        ]
        .join("\n")
    }

    pub(crate) fn write_project_report(&self, state: &State, content: &str) -> Result<()> {
        let body = if content.starts_with('#') {
            content.to_string()
        } else {
            format!("# {}\n\n{content}", state.topic)
        };
        let mut lines = body.lines().collect::<Vec<_>>();
        let metadata = format!(
            "> Generated: {} · Topic ID: `{}` · Level: {} · Provider: {}",
            now(),
            state.topic_id,
            state.search_level,
            state.search_provider
        );
        if lines
            .get(1)
            .is_some_and(|line| line.starts_with("> Generated:"))
        {
            lines[1] = &metadata;
            return write_file(
                self.project_root(&state.topic_id).join("report.md"),
                &lines.join("\n"),
            );
        }
        let body = if lines.is_empty() {
            metadata
        } else {
            let mut output = vec![lines[0], "", &metadata];
            output.extend(lines.iter().skip(1).copied());
            output.join("\n")
        };
        write_file(self.project_root(&state.topic_id).join("report.md"), &body)
    }

    pub(crate) fn refresh_mirrors(&self, state: &State) -> Result<()> {
        self.render_claims(&state.topic_id)?;
        self.render_links(&state.topic_id)?;
        self.render_leads(&state.topic_id)?;
        self.render_answers(&state.topic_id)?;
        self.render_resume_summary(state)?;
        Ok(())
    }

    pub(crate) fn status_value(&self, state: &State) -> Value {
        json!({
            "topic_id": state.topic_id,
            "topic": state.topic,
            "status": state.status,
            "search_provider": state.search_provider,
            "search_level": state.search_level,
            "orchestration": state.orchestration,
            "agent_roster": state.agent_roster,
            "local_project_paths": state.local_project_paths,
            "viewer_url": if state.status == Status::Completed { None } else { state.viewer_url.clone() },
            "last_viewer_url": state.viewer_url,
            "report_path": self.report_path(&state.topic_id),
            "snapshot_path": self.snapshot_path(&state.topic_id),
            "manifest_path": self.manifest_path(&state.topic_id),
            "round": state.current_round,
            "max_rounds": state.max_rounds,
            "target_sources": effective_target_sources(state),
            "min_accepted_sources": state.min_accepted_sources,
            "stop_when_confident": state.stop_when_confident,
            "stop_on_no_new_sources": state.stop_on_no_new_sources,
            "counts": {
                "sources": state.source_count,
                "notes": state.note_count,
                "claims": state.claim_count,
                "links": state.link_count,
                "leads": state.open_leads.len(),
            },
            "path": self.project_root(&state.topic_id),
            "model": state.research_model,
        })
    }
}
