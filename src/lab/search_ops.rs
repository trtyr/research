use super::*;

impl Lab {
    pub(crate) fn search(&self, state: &State, query: &str) -> Result<Vec<SearchHit>> {
        let local_hits = self.search_local_projects(state, query)?;
        let web_hits = match state.search_provider {
            SearchProvider::Deterministic => {
                let result = Ok(deterministic::search(state, query));
                self.record_search_attempt(state, query, "deterministic", &result, 0)?;
                result
            }
            SearchProvider::Exa => {
                let result = exa::search(query, state.max_sources, self.exa_api_key.as_deref());
                self.record_search_attempt(state, query, "exa", &result, 0)?;
                result
            }
            SearchProvider::Code => {
                let result = code::search(query, self.code_tokens_num(), self.code_api_key().as_deref(), self.config.timeouts.mcp_timeout_secs);
                self.record_search_attempt(state, query, "code", &result, 0)?;
                result
            }
            SearchProvider::Zhipu => {
                let result = zhipu::search(
                    query,
                    state.max_sources,
                    self.config.zhipu_mcp_url.as_deref(),
                    self.config.zhipu_api_key.as_deref(),
                    self.config.zhipu_tool.as_deref(),
                    self.config.zhipu_content_size.as_deref(),
                    self.config.timeouts.mcp_timeout_secs,
                );
                self.record_search_attempt(state, query, "zhipu", &result, 0)?;
                result
            }
            SearchProvider::Minimax => {
                let result = minimax::search(
                    query,
                    state.max_sources,
                    self.config.minimax_api_key.as_deref(),
                    self.config.minimax_api_host.as_deref(),
                    self.config.minimax_mcp_command.as_deref(),
                    self.config.minimax_mcp_args.as_deref(),
                );
                self.record_search_attempt(state, query, "minimax", &result, 0)?;
                result
            }
            SearchProvider::Kimi => {
                let result = kimi::search(
                    query,
                    self.config.kimi_api_key.as_deref().unwrap_or(""),
                    self.config.kimi_model.as_deref(),
                    self.config.timeouts.mcp_timeout_secs,
                );
                self.record_search_attempt(state, query, "kimi", &result, 0)?;
                result
            }
            SearchProvider::Hybrid => self.search_hybrid(state, query),
        };
        match web_hits {
            Ok(hits) => Ok(interleave_hits(local_hits, hits)),
            Err(error) if !local_hits.is_empty() => Ok(local_hits),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn search_hybrid(&self, state: &State, query: &str) -> Result<Vec<SearchHit>> {
        hybrid::search(
            hybrid::HybridInputs {
                query,
                max_sources: state.max_sources,
                run_code_surface: self.should_run_code_surface(state, query),
                exa_api_key: self.exa_api_key.as_deref(),
                code_tokens_num: self.code_tokens_num(),
                code_api_key: self.code_api_key().as_deref(),
                zhipu_mcp_url: self.config.zhipu_mcp_url.as_deref(),
                zhipu_api_key: self.config.zhipu_api_key.as_deref(),
                zhipu_tool: self.config.zhipu_tool.as_deref(),
                zhipu_content_size: self.config.zhipu_content_size.as_deref(),
                minimax_api_key: self.config.minimax_api_key.as_deref(),
                minimax_api_host: self.config.minimax_api_host.as_deref(),
                minimax_mcp_command: self.config.minimax_mcp_command.as_deref(),
                minimax_mcp_args: self.config.minimax_mcp_args.as_deref(),
                kimi_api_key: self.config.kimi_api_key.as_deref(),
                kimi_model: self.config.kimi_model.as_deref(),
                mcp_timeout_secs: self.config.timeouts.mcp_timeout_secs,
            },
            |provider, result| self.record_search_attempt(state, query, provider, result, 0),
        )
    }

    pub(crate) fn record_search_attempt(
        &self,
        state: &State,
        query: &str,
        provider: &str,
        result: &Result<Vec<SearchHit>>,
        accepted_source_count: usize,
    ) -> Result<()> {
        let attempt = SearchAttemptRecord {
            id: id("search"),
            provider: provider.to_string(),
            query: query.to_string(),
            status: if result.is_ok() { "ok" } else { "error" }.to_string(),
            result_count: result.as_ref().map(|hits| hits.len()).unwrap_or_default(),
            error: result.as_ref().err().map(ToString::to_string),
            accepted_source_count,
            was_productive: accepted_source_count > 0,
            created_at: now(),
        };
        self.append_jsonl(&state.topic_id, "search_attempts.jsonl", &attempt)?;
        self.event(
            &state.topic_id,
            "search.provider",
            json!({
                "provider": attempt.provider,
                "query": attempt.query,
                "status": attempt.status,
                "result_count": attempt.result_count,
                "error": attempt.error,
            }),
        )
    }

    pub(crate) fn record_agent_run(
        &self,
        state: &State,
        role: AgentRole,
        status: &str,
        input_summary: impl Into<String>,
        output_summary: impl Into<String>,
        artifact_paths: Vec<PathBuf>,
    ) -> Result<()> {
        let timestamp = now();
        let record = AgentRunRecord {
            id: id("agent"),
            role,
            round: state.current_round,
            status: status.to_string(),
            model: self.model_for_role(state, role),
            input_summary: input_summary.into(),
            output_summary: output_summary.into(),
            artifact_paths: artifact_paths
                .into_iter()
                .map(|path| path.display().to_string())
                .collect(),
            started_at: timestamp.clone(),
            completed_at: timestamp,
        };
        self.append_jsonl(&state.topic_id, "agent_runs.jsonl", &record)?;
        self.event(
            &state.topic_id,
            "agent.completed",
            json!({
                "role": role,
                "round": state.current_round,
                "status": record.status,
                "model": record.model,
                "output_summary": record.output_summary,
            }),
        )
    }

    pub(crate) fn search_local_projects(&self, state: &State, query: &str) -> Result<Vec<SearchHit>> {
        if state.local_project_paths.is_empty() {
            return Ok(Vec::new());
        }
        let max_files = self.config.local_project_max_files.unwrap_or(800) as usize;
        let mut files = Vec::new();
        for root in &state.local_project_paths {
            collect_local_project_files(root, max_files, &mut files)?;
            if files.len() >= max_files {
                break;
            }
        }
        Ok(dedupe_hits(
            files
                .iter()
                .filter_map(|file| local_project_hit(file, &state.topic, query).ok().flatten())
                .collect(),
        )
        .into_iter()
        .take(state.max_sources.min(20) as usize)
        .collect())
    }

    /// Read previously attempted queries from search_attempts.jsonl.
    pub(crate) fn past_queries(&self, topic_id: &str) -> Vec<String> {
        self.read_jsonl::<SearchAttemptRecord>(topic_id, "search_attempts.jsonl")
            .unwrap_or_default()
            .into_iter()
            .map(|attempt| attempt.query)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Read search attempts enriched with productivity info.
    #[allow(dead_code)]
    pub(crate) fn productive_queries(&self, topic_id: &str) -> Vec<String> {
        self.read_jsonl::<SearchAttemptRecord>(topic_id, "search_attempts.jsonl")
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.was_productive)
            .map(|a| a.query)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
