use super::*;

impl Lab {
    pub(crate) fn load_state(&self, topic_id: &str) -> Result<State> {
        serde_json::from_str(&read_to_string(
            self.project_root(topic_id).join("state.json"),
        )?)
        .with_context(|| format!("failed to parse state for {topic_id}"))
    }

    pub(crate) fn save_state(&self, state: &State) -> Result<()> {
        let mut state = state.clone();
        state.updated_at = now();
        write_file(
            self.project_root(&state.topic_id).join("state.json"),
            &serde_json::to_string_pretty(&state)?,
        )?;
        self.ensure_graph(&state).map(|_| ())
    }

    pub(crate) fn list_states(&self) -> Result<Vec<State>> {
        Ok(read_dir_names(&self.root)?
            .into_iter()
            .filter_map(|name| self.load_state(&name).ok())
            .collect())
    }

    pub(crate) fn read_jsonl<T: for<'de> Deserialize<'de>>(
        &self,
        topic_id: &str,
        file: &str,
    ) -> Result<Vec<T>> {
        self.storage.read_jsonl(topic_id, file)
    }

    pub(crate) fn write_jsonl<T: Serialize>(&self, topic_id: &str, file: &str, items: &[T]) -> Result<()> {
        self.storage.write_jsonl(topic_id, file, items)
    }

    pub(crate) fn append_jsonl<T: Serialize>(&self, topic_id: &str, file: &str, value: &T) -> Result<()> {
        self.storage.append_jsonl(topic_id, file, value)
    }

    pub(crate) fn event(&self, topic_id: &str, event_type: &str, data: Value) -> Result<()> {
        self.storage.event(topic_id, event_type, data)
    }

    pub(crate) fn open_leads(&self, topic_id: &str) -> Result<Vec<String>> {
        Ok(self
            .read_jsonl::<LeadRecord>(topic_id, "leads.jsonl")?
            .into_iter()
            .filter(|lead| lead.status == "open")
            .map(|lead| lead.direction)
            .collect())
    }

    pub(crate) fn write_viewer_snapshot(&self, topic_id: &str) -> Result<PathBuf> {
        let state = self.viewer_state(topic_id)?;
        let path = self.snapshot_path(topic_id);
        write_file(&path, &viewer_html(topic_id, Some(&state))?)?;
        Ok(path)
    }

    pub(crate) fn write_artifact_manifest(&self, topic_id: &str) -> Result<PathBuf> {
        let state = self.load_state(topic_id)?;
        let root = self.project_root(topic_id);
        let path = self.manifest_path(topic_id);
        write_file(
            &path,
            &serde_json::to_string_pretty(&json!({
                "topic_id": state.topic_id,
                "topic": state.topic,
                "status": state.status,
                "search": {
                    "level": state.search_level,
                    "provider": state.search_provider,
                    "round": state.current_round,
                    "max_rounds": state.max_rounds,
                    "target_sources": effective_target_sources(&state),
                    "orchestration": state.orchestration,
                    "agent_roster": state.agent_roster,
                },
                "entrypoints": {
                    "project_root": artifact_entry(&root),
                    "manifest": {
                        "path": path,
                        "exists": true,
                        "kind": "file",
                        "bytes": null,
                    },
                    "report": artifact_entry(root.join("report.md")),
                    "snapshot": artifact_entry(self.snapshot_path(topic_id)),
                    "resume_summary": artifact_entry(root.join("resume_summary.md")),
                    "last_viewer_url": state.viewer_url,
                },
                "search_process": {
                    "plan": artifact_entry(root.join("plan.md")),
                    "agenda": artifact_entry(root.join("agenda.md")),
                    "agent_runs": artifact_entry(root.join("agent_runs.jsonl")),
                    "events": artifact_entry(root.join("logs/events.jsonl")),
                    "search_attempts": artifact_entry(root.join("search_attempts.jsonl")),
                    "candidate_sources": artifact_entry(root.join("candidate_sources.jsonl")),
                    "accepted_sources": artifact_entry(root.join("accepted_sources.jsonl")),
                    "rejected_sources": artifact_entry(root.join("rejected_sources.jsonl")),
                },
                "evidence": {
                    "sources": artifact_entry(root.join("sources.jsonl")),
                    "chunks": artifact_entry(root.join("chunks.jsonl")),
                    "notes_dir": artifact_entry(root.join("notes")),
                    "claims": artifact_entry(root.join("claims.jsonl")),
                    "claims_markdown": artifact_entry(root.join("claims.md")),
                    "claim_events": artifact_entry(root.join("claim_events.jsonl")),
                    "evaluations": artifact_entry(root.join("evaluations.jsonl")),
                },
                "graph": {
                    "graph_json": artifact_entry(root.join("graph.json")),
                    "links": artifact_entry(root.join("links.jsonl")),
                    "links_markdown": artifact_entry(root.join("links.md")),
                    "leads": artifact_entry(root.join("leads.jsonl")),
                    "threads": artifact_entry(root.join("threads.md")),
                    "insights": artifact_entry(root.join("insights.md")),
                },
                "outputs": {
                    "report_review": artifact_entry(root.join("report_review.md")),
                    "answers": artifact_entry(root.join("answers.jsonl")),
                    "answers_markdown": artifact_entry(root.join("answers.md")),
                    "exports_dir": artifact_entry(root.join("exports")),
                },
                "updated_at": now(),
            }))?,
        )?;
        Ok(path)
    }

    pub(crate) fn ensure_project_dirs(&self, topic_id: &str) -> Result<()> {
        for dir in ["notes", "exports", "logs"] {
            fs::create_dir_all(self.project_root(topic_id).join(dir))?;
        }
        Ok(())
    }
}
