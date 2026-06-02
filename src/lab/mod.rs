use std::{collections::BTreeSet, fs, io::{Read, Write}, net::TcpStream, path::PathBuf, time::{Duration, Instant}};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    artifact_entry, AgentRole, AgentRunRecord, CandidateSourceRecord, ClaimRecord, EmptyText,
    ExportFormat, GraphData, LeadRecord, LinkRecord, Priority, ReadTarget, ResearchConfig, SearchAttemptRecord,
    SearchHit, SearchLevel, SearchProfile, SearchProvider, SourceChunkRecord, SourceRecord,
    StartInput, State, Status, Storage, chunk_source,
    credibility_score, dedupe_hits, effective_target_sources, evidence_gate, expand_tilde,
    extract_claims, extract_model_text, extract_root_domain, follow_up_leads, interleave_hits,
    is_optional_jsonl_target, is_technical_research_text, likely_claim_fragment,
    read_dir_names, read_target_name, read_to_string,
    rerank_hits,
    short, short_excerpt, should_complete_after_round, source_authority,
    strip_links_for_claims, unique_paths, viewer_html, write_file, ModelMessageResponse,
};
use crate::graph_utils::{claim_graph_node, graph_edge, lead_graph_node, link_key, push_graph_edge, root_graph_id, root_graph_node, source_graph_node, upsert_graph_edge, upsert_graph_node};
use crate::local_search::{collect_local_project_files, local_project_hit};
use crate::providers::{code, deterministic, exa, hybrid, kimi, minimax, zhipu};
use crate::rendering::{render_chunk_excerpts, render_evidence_base, render_limitations, render_numbered_findings, render_perspectives, render_search_quality};
use crate::utils::{deduplicate_queries, id, merge, normalize, now, priority_rank, same_research_idea, slug, text_overlaps, token_overlap, unique_strings};
use crate::viewer;

mod graph;
mod storage_ops;
mod search_ops;
mod render;

pub(crate) struct Lab {
    pub(crate) root: PathBuf,
    pub(crate) search_provider: SearchProvider,
    pub(crate) exa_api_key: Option<String>,
    pub(crate) config: ResearchConfig,
    pub(crate) storage: Storage,
}

impl Lab {
    pub(crate) fn new(
        root: PathBuf,
        search_provider: SearchProvider,
        exa_api_key: Option<String>,
        config: ResearchConfig,
    ) -> Self {
        Self {
            storage: Storage::new(root.clone()),
            root,
            search_provider,
            exa_api_key,
            config,
        }
    }

    pub(crate) fn search_profile(&self, level: SearchLevel) -> SearchProfile {
        let base = match level {
            SearchLevel::Quick => SearchProfile {
                min_accepted_sources: 2,
                max_accepted_sources: 6,
                max_rounds: 1,
                max_runtime_minutes: 8,
                stop_when_confident: true,
                stop_on_no_new_sources: true,
                orchestration: "single_pass_agent_pipeline",
                agents: vec![
                    AgentRole::Planner,
                    AgentRole::Searcher,
                    AgentRole::Verifier,
                    AgentRole::Reader,
                    AgentRole::Linker,
                    AgentRole::Writer,
                ],
            },
            SearchLevel::Deep => SearchProfile {
                min_accepted_sources: 6,
                max_accepted_sources: 18,
                max_rounds: 3,
                max_runtime_minutes: 45,
                stop_when_confident: true,
                stop_on_no_new_sources: true,
                orchestration: "multi_role_reviewed_pipeline",
                agents: vec![
                    AgentRole::Planner,
                    AgentRole::Searcher,
                    AgentRole::Verifier,
                    AgentRole::Reader,
                    AgentRole::Linker,
                    AgentRole::Writer,
                    AgentRole::Reviewer,
                ],
            },
            SearchLevel::Research => SearchProfile {
                min_accepted_sources: 12,
                max_accepted_sources: 50,
                max_rounds: 8,
                max_runtime_minutes: 180,
                stop_when_confident: false,
                stop_on_no_new_sources: true,
                orchestration: "microkernel_multi_agent_research",
                agents: vec![
                    AgentRole::Planner,
                    AgentRole::Searcher,
                    AgentRole::Verifier,
                    AgentRole::Reader,
                    AgentRole::Linker,
                    AgentRole::Writer,
                    AgentRole::Reviewer,
                ],
            },
        };
        match level {
            SearchLevel::Quick => SearchProfile {
                min_accepted_sources: self
                    .config
                    .quick_min_accepted_sources
                    .unwrap_or(base.min_accepted_sources),
                max_accepted_sources: self
                    .config
                    .quick_max_accepted_sources
                    .unwrap_or(base.max_accepted_sources),
                max_rounds: self.config.quick_max_rounds.unwrap_or(base.max_rounds),
                max_runtime_minutes: self
                    .config
                    .quick_max_runtime_minutes
                    .unwrap_or(base.max_runtime_minutes),
                ..base
            },
            SearchLevel::Deep => SearchProfile {
                min_accepted_sources: self
                    .config
                    .deep_min_accepted_sources
                    .unwrap_or(base.min_accepted_sources),
                max_accepted_sources: self
                    .config
                    .deep_max_accepted_sources
                    .unwrap_or(base.max_accepted_sources),
                max_rounds: self.config.deep_max_rounds.unwrap_or(base.max_rounds),
                max_runtime_minutes: self
                    .config
                    .deep_max_runtime_minutes
                    .unwrap_or(base.max_runtime_minutes),
                ..base
            },
            SearchLevel::Research => SearchProfile {
                min_accepted_sources: self
                    .config
                    .research_min_accepted_sources
                    .unwrap_or(base.min_accepted_sources),
                max_accepted_sources: self
                    .config
                    .research_max_accepted_sources
                    .unwrap_or(base.max_accepted_sources),
                max_rounds: self.config.research_max_rounds.unwrap_or(base.max_rounds),
                max_runtime_minutes: self
                    .config
                    .research_max_runtime_minutes
                    .unwrap_or(base.max_runtime_minutes),
                ..base
            },
        }
    }

    pub(crate) fn code_api_key(&self) -> Option<String> {
        self.config
            .search
            .providers
            .code
            .api_key
            .clone()
            .or_else(|| self.exa_api_key.clone())
    }

    pub(crate) fn code_tokens_num(&self) -> u32 {
        self.config
            .search
            .providers
            .code
            .tokens_num
            .unwrap_or(5_000)
            .clamp(1_000, 50_000)
    }

    pub(crate) fn should_run_code_surface(&self, state: &State, query: &str) -> bool {
        if self.config.search.providers.code.enabled == Some(false) {
            return false;
        }
        if self.config.search.providers.code.auto == Some(false) {
            return true;
        }
        is_technical_research_text(&format!("{} {}", state.topic, query))
    }
    pub(crate) fn start(&self, input: StartInput) -> Result<Value> {
        let topic_id = slug(input.topic_id.as_deref().unwrap_or(&input.topic));
        let project = self.project_root(&topic_id);
        let level = input
            .level
            .or(self.config.default_level)
            .unwrap_or(SearchLevel::Deep);
        let profile = self.search_profile(level);
        let quality_hint = input
            .quality_hint
            .or(self.config.default_quality_hint)
            .unwrap_or_default();
        if project.join("state.json").exists() && !input.resume_existing {
            bail!(
                "research project already exists: {topic_id}. Use --resume-existing or status/read."
            )
        }
        if !project.join("state.json").exists() {
            self.ensure_project_dirs(&topic_id)?;
            let local_project_paths = unique_paths(
                input
                    .local_project_paths
                    .into_iter()
                    .chain(self.config.local_project_paths.clone().unwrap_or_default())
                    .map(expand_tilde)
                    .collect(),
            );
            let state = State {
                topic_id: topic_id.clone(),
                topic: input.topic.trim().to_string(),
                status: Status::Running,
                quality_hint,
                created_at: now(),
                updated_at: now(),
                current_round: 0,
                max_rounds: input
                    .max_rounds
                    .or(self.config.max_rounds)
                    .unwrap_or(profile.max_rounds),
                max_sources: input
                    .max_sources
                    .or(self.config.max_sources)
                    .unwrap_or(profile.max_accepted_sources),
                max_runtime_minutes: input
                    .max_runtime_minutes
                    .or(self.config.max_runtime_minutes)
                    .unwrap_or(profile.max_runtime_minutes),
                min_accepted_sources: profile.min_accepted_sources,
                target_accepted_sources: input
                    .max_sources
                    .or(self.config.max_sources)
                    .unwrap_or(profile.max_accepted_sources),
                stop_when_confident: profile.stop_when_confident,
                stop_on_no_new_sources: profile.stop_on_no_new_sources,
                research_model: input.model.or_else(|| self.config.default_model.clone()),
                search_provider: self.search_provider,
                search_level: level,
                local_project_paths,
                viewer_url: None,
                orchestration: profile.orchestration.to_string(),
                agent_roster: profile.agents.iter().map(ToString::to_string).collect(),
                active_directions: input
                    .initial_directions
                    .into_iter()
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect(),
                source_count: 0,
                note_count: 0,
                claim_count: 0,
                link_count: 0,
                open_leads: Vec::new(),
                last_error: None,
            };
            self.save_state(&state)?;
            self.write_plan(&state)?;
            self.write_project_report(&state, "No synthesis has been generated yet.")?;
            self.refresh_mirrors(&state)?;
            self.event(
                &topic_id,
                "project.created",
                json!({ "topic": state.topic }),
            )?;
        }
        let mut state = self.load_state(&topic_id)?;
        self.ensure_viewer(&mut state)?;
        let state = if state.max_rounds == 0 {
            state.status = Status::Paused;
            self.save_state(&state)?;
            state
        } else {
            self.run_round(state, None)?
        };
        if state.status == Status::Completed {
            self.write_viewer_snapshot(&state.topic_id)?;
        }
        self.write_artifact_manifest(&state.topic_id)?;
        Ok(self.status_value(&state))
    }

    pub(crate) fn status(&self, topic_id: Option<String>) -> Result<Value> {
        if let Some(topic_id) = topic_id {
            let mut state = self.load_state(&topic_id)?;
            if state.status == Status::Completed {
                self.write_viewer_snapshot(&state.topic_id)?;
            } else {
                self.ensure_viewer(&mut state)?;
            }
            self.write_artifact_manifest(&state.topic_id)?;
            self.render_resume_summary(&state)?;
            return Ok(json!({
                "action": "status",
                "project": self.status_value(&state),
                "resume_summary": read_to_string(self.project_root(&state.topic_id).join("resume_summary.md"))?,
            }));
        }
        let mut states = self.list_states()?;
        states.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(json!({
            "action": "status",
            "root": self.root,
            "projects": states.iter().map(|state| self.status_value(state)).collect::<Vec<_>>(),
        }))
    }

    pub(crate) fn read(
        &self,
        topic_id: &str,
        target: ReadTarget,
        note_id: Option<String>,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        let state = self.load_state(topic_id)?;
        if matches!(target, ReadTarget::Notes) && note_id.is_none() {
            let notes_dir = self.project_root(topic_id).join("notes");
            let notes = read_dir_names(&notes_dir)?
                .into_iter()
                .filter(|item| item.ends_with(".md"))
                .collect::<Vec<_>>();
            return Ok(json!({
                "action": "read",
                "topic_id": state.topic_id,
                "target": "notes",
                "notes": notes,
            }));
        }
        let path = if matches!(target, ReadTarget::Notes) {
            self.project_root(topic_id).join("notes").join(format!(
                "{}.md",
                note_id.unwrap_or_default().trim_end_matches(".md")
            ))
        } else {
            self.artifact_path(topic_id, target)
        };
        let content = if path.exists() {
            read_to_string(&path)?
        } else if is_optional_jsonl_target(target) {
            String::new()
        } else {
            read_to_string(&path)?
        };
        let start = offset.saturating_sub(1);
        let end = content.len().min(start + limit);
        Ok(json!({
            "action": "read",
            "topic_id": state.topic_id,
            "target": read_target_name(target),
            "path": path,
            "offset": offset,
            "more": end < content.len(),
            "content": content.get(start..end).unwrap_or_default(),
            "next_offset": if end < content.len() { Some(end + 1) } else { None },
        }))
    }

    pub(crate) fn add_direction(
        &self,
        topic_id: &str,
        direction: String,
        reason: Option<String>,
        priority: Priority,
    ) -> Result<Value> {
        let mut state = self.load_state(topic_id)?;
        self.append_leads(
            topic_id,
            vec![LeadRecord::new(direction.clone(), reason.clone(), priority)],
        )?;
        if !state
            .active_directions
            .iter()
            .any(|item| same_research_idea(item, &direction))
        {
            state.active_directions.push(direction.clone());
        }
        state.open_leads = self.open_leads(topic_id)?;
        self.save_state(&state)?;
        self.write_plan(&state)?;
        self.refresh_mirrors(&state)?;
        self.event(
            topic_id,
            "direction.added",
            json!({ "direction": direction, "reason": reason, "priority": priority }),
        )?;
        Ok(self.status_value(&state))
    }

    /// Add an explicit search query as a lead and run it immediately.
    pub(crate) fn search_query(
        &self,
        topic_id: &str,
        query: String,
        reason: Option<String>,
    ) -> Result<Value> {
        let mut state = self.load_state(topic_id)?;
        // Add as a lead so it's tracked
        self.append_leads(
            topic_id,
            vec![LeadRecord::new(
                query.clone(),
                reason.clone(),
                Priority::High,
            )],
        )?;
        state.open_leads = self.open_leads(topic_id)?;
        // Ensure we have rounds available
        if state.current_round >= state.max_rounds {
            state.max_rounds = state.current_round + 1;
        }
        if state.status == Status::Completed || state.status == Status::Stopped {
            state.status = Status::Running;
        }
        self.save_state(&state)?;
        self.ensure_viewer(&mut state)?;
        // Run a focused round with the explicit query
        let state = self.run_round(state, Some(query.clone()))?;
        if state.status == Status::Completed {
            self.write_viewer_snapshot(&state.topic_id)?;
        }
        self.write_artifact_manifest(&state.topic_id)?;
        self.event(
            topic_id,
            "search.query",
            json!({ "query": query, "reason": reason }),
        )?;
        Ok(self.status_value(&state))
    }

    pub(crate) fn set_status(&self, topic_id: &str, status: Status) -> Result<Value> {
        let mut state = self.load_state(topic_id)?;
        state.status = status;
        self.save_state(&state)?;
        self.event(
            topic_id,
            "project.status",
            json!({ "status": state.status }),
        )?;
        Ok(self.status_value(&state))
    }

    pub(crate) fn resume(
        &self,
        topic_id: &str,
        focus: Option<String>,
        local_project_paths: Vec<PathBuf>,
        model: Option<String>,
    ) -> Result<Value> {
        let mut state = self.load_state(topic_id)?;
        let has_new_leads = !self.open_leads(topic_id).unwrap_or_default().is_empty();
        let has_new_directions = !state.active_directions.is_empty();
        if state.current_round >= state.max_rounds && focus.is_none() && !has_new_leads && !has_new_directions {
            state.status = Status::Completed;
            self.save_state(&state)?;
            self.write_viewer_snapshot(&state.topic_id)?;
            self.write_artifact_manifest(&state.topic_id)?;
            return Ok(json!({
                "message": format!("Project already completed: max_rounds reached ({}/{}). Use add-direction to add new research directions.", state.current_round, state.max_rounds),
                "project": self.status_value(&state),
            }));
        }
        if state.current_round >= state.max_rounds {
            // Extend rounds for incremental research: allow at least 1 more round
            state.max_rounds = state.current_round + 1;
        }
        if model.is_some() {
            state.research_model = model;
        }
        if !local_project_paths.is_empty() {
            state.local_project_paths = unique_paths(
                state
                    .local_project_paths
                    .into_iter()
                    .chain(local_project_paths.into_iter().map(expand_tilde))
                    .collect(),
            );
        }
        state.status = Status::Running;
        self.save_state(&state)?;
        self.ensure_viewer(&mut state)?;
        let state = self.run_round(state, focus)?;
        if state.status == Status::Completed {
            self.write_viewer_snapshot(&state.topic_id)?;
        }
        self.write_artifact_manifest(&state.topic_id)?;
        Ok(self.status_value(&state))
    }

    pub(crate) fn find_links(&self, topic_id: &str) -> Result<Value> {
        let mut state = self.load_state(topic_id)?;
        let claims = self.read_jsonl::<ClaimRecord>(topic_id, "claims.jsonl")?;
        let mut links = self.read_jsonl::<LinkRecord>(topic_id, "links.jsonl")?;
        let mut created = Vec::new();
        let mut seen = links
            .iter()
            .map(|item| link_key(&item.from, &item.to, &item.link_type))
            .collect::<BTreeSet<_>>();
        for pair in claims.windows(2) {
            let from = pair[0].id.clone();
            let to = pair[1].id.clone();
            let link_type = if text_overlaps(&pair[0].text, &pair[1].text) {
                "supports"
            } else {
                "extends"
            }
            .to_string();
            if !seen.insert(link_key(&from, &to, &link_type)) {
                continue;
            }
            let link = LinkRecord {
                id: id("link"),
                from,
                to,
                link_type,
                rationale: "Deterministic relationship inferred from adjacent collected claims; replace with model linker in the next integration phase.".to_string(),
                source_ids: pair
                    .iter()
                    .flat_map(|claim| claim.source_ids.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                confidence: "low".to_string(),
                created_at: now(),
            };
            created.push(link.clone());
            links.push(link);
        }
        self.write_jsonl(topic_id, "links.jsonl", &links)?;
        self.record_links_in_graph(topic_id, &created)?;
        state.link_count = links.len();
        self.save_state(&state)?;
        self.render_links(topic_id)?;
        self.render_insights(topic_id)?;
        self.event(topic_id, "links.refreshed", json!({ "links": links.len() }))?;
        Ok(json!({
            "action": "find_links",
            "topic_id": topic_id,
            "links": links,
        }))
    }

    pub(crate) fn ask(&self, topic_id: &str, question: String) -> Result<Value> {
        let state = self.load_state(topic_id)?;
        let claims = self.read_jsonl::<ClaimRecord>(topic_id, "claims.jsonl")?;
        let chunks = self.read_jsonl::<SourceChunkRecord>(topic_id, "chunks.jsonl")?;
        let key = normalize(&question);
        let mut related = claims
            .iter()
            .map(|claim| (token_overlap(&normalize(&claim.text), &key), claim))
            .filter(|(_, claim)| !likely_claim_fragment(&claim.text))
            .filter(|(score, _)| *score >= 0.12)
            .collect::<Vec<_>>();
        related.sort_by(|left, right| right.0.total_cmp(&left.0));
        let related = related
            .into_iter()
            .take(6)
            .map(|(_, claim)| claim)
            .collect::<Vec<_>>();
        let mut related_chunks = chunks
            .iter()
            .map(|chunk| {
                (
                    token_overlap(&normalize(&format!("{} {}", chunk.title, chunk.text)), &key),
                    chunk,
                )
            })
            .filter(|(score, _)| *score >= 0.08)
            .collect::<Vec<_>>();
        related_chunks.sort_by(|left, right| right.0.total_cmp(&left.0));
        let related_chunks = related_chunks
            .into_iter()
            .take(4)
            .map(|(_, chunk)| chunk)
            .collect::<Vec<_>>();
        let answer = if related.is_empty() && related_chunks.is_empty() {
            "当前项目没有足够的直接证据来可靠回答这个问题。".to_string()
        } else {
            [
                format!(
                    "找到 {} 条相关结论和 {} 个相关证据片段。",
                    related.len(),
                    related_chunks.len()
                ),
                related
                    .iter()
                    .map(|claim| format!("- 结论 `{}`: {}", claim.id, claim.text))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .if_empty("- 暂无直接匹配的结论。"),
                related_chunks
                    .iter()
                    .map(|chunk| {
                        format!(
                            "- 证据 `{}` / 来源 `{}`: {}",
                            chunk.id,
                            chunk.source_id,
                            short(&strip_links_for_claims(&chunk.text), 220)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .if_empty("- 暂无直接匹配的证据片段。"),
            ]
            .join("\n")
        };
        self.append_jsonl(
            topic_id,
            "answers.jsonl",
            &json!({
                "id": id("answer"),
                "question": question,
                "answer": answer,
                "claim_ids": related.iter().map(|claim| claim.id.clone()).collect::<Vec<_>>(),
                "source_ids": related.iter().flat_map(|claim| claim.source_ids.clone()).chain(related_chunks.iter().map(|chunk| chunk.source_id.clone())).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>(),
                "chunk_ids": related_chunks.iter().map(|chunk| chunk.id.clone()).collect::<Vec<_>>(),
                "insight_ids": Vec::<String>::new(),
                "created_at": now(),
            }),
        )?;
        self.render_answers(topic_id)?;
        Ok(json!({
            "action": "ask",
            "topic_id": state.topic_id,
            "answer": answer,
        }))
    }

    pub(crate) fn refine(&self, topic_id: &str) -> Result<Value> {
        let state = self.load_state(topic_id)?;
        let leads = self.read_jsonl::<LeadRecord>(topic_id, "leads.jsonl")?;
        let gaps = self.read_jsonl::<Value>(topic_id, "gaps.jsonl")?;
        let links = self.read_jsonl::<LinkRecord>(topic_id, "links.jsonl")?;
        let content = [
            format!("# Refinement: {}", state.topic),
            String::new(),
            format!("Generated: {}", now()),
            String::new(),
            "## Gaps".to_string(),
            String::new(),
            if gaps.is_empty() {
                "- No gaps recorded yet.".to_string()
            } else {
                gaps.iter()
                    .take(12)
                    .map(|gap| {
                        format!(
                            "- {}",
                            gap.get("text")
                                .and_then(Value::as_str)
                                .unwrap_or("Unknown gap")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            String::new(),
            "## Contradictions".to_string(),
            String::new(),
            links
                .iter()
                .filter(|link| link.link_type == "contradicts")
                .take(12)
                .map(|link| format!("- {} -> {}: {}", link.from, link.to, link.rationale))
                .collect::<Vec<_>>()
                .join("\n")
                .if_empty("- No contradictions identified."),
            String::new(),
            "## Stale Leads".to_string(),
            String::new(),
            "- No stale leads identified by deterministic CLI mode.".to_string(),
            String::new(),
            "## Next Directions".to_string(),
            String::new(),
            leads
                .iter()
                .filter(|lead| lead.status == "open")
                .take(12)
                .map(|lead| format!("- **{}** {}", lead.priority, lead.direction))
                .collect::<Vec<_>>()
                .join("\n")
                .if_empty("- No next directions identified."),
        ]
        .join("\n");
        write_file(self.project_root(topic_id).join("refine.md"), &content)?;
        self.event(topic_id, "project.refined", json!({}))?;
        Ok(json!({ "action": "refine", "topic_id": topic_id, "content": content }))
    }

    pub(crate) fn synthesize(&self, topic_id: &str) -> Result<Value> {
        let state = self.load_state(topic_id)?;
        let sources = self.read_jsonl::<SourceRecord>(topic_id, "sources.jsonl")?;
        let chunks = self.read_jsonl::<SourceChunkRecord>(topic_id, "chunks.jsonl")?;
        let candidates =
            self.read_jsonl::<CandidateSourceRecord>(topic_id, "candidate_sources.jsonl")?;
        let claims = self.read_jsonl::<ClaimRecord>(topic_id, "claims.jsonl")?;
        let links = self.read_jsonl::<LinkRecord>(topic_id, "links.jsonl")?;
        let leads = self.read_jsonl::<LeadRecord>(topic_id, "leads.jsonl")?;
        let high_confidence_sources = sources
            .iter()
            .filter(|source| source.credibility_score >= 0.7)
            .count();
        let open_leads = leads
            .iter()
            .filter(|lead| lead.status == "open")
            .collect::<Vec<_>>();
        let contradictions = links
            .iter()
            .filter(|link| link.link_type == "contradicts")
            .collect::<Vec<_>>();
        let report = if sources.is_empty() {
            self.render_no_accepted_evidence_report(&state)
        } else if let Some(report) = self.generate_model_report(
            &state,
            &sources,
            &chunks,
            &candidates,
            &claims,
            &links,
            &leads,
        )? {
            report
        } else {
            self.render_deterministic_report(
                &state,
                &sources,
                &chunks,
                &candidates,
                &claims,
                &links,
                &open_leads,
                &contradictions,
                high_confidence_sources,
            )
        };
        self.write_project_report(&state, &report)?;
        self.record_agent_run(
            &state,
            AgentRole::Writer,
            "completed",
            format!(
                "sources={} claims={} links={} leads={}",
                sources.len(),
                claims.len(),
                links.len(),
                leads.len()
            ),
            "synthesized final human-readable report",
            vec![self.project_root(&state.topic_id).join("report.md")],
        )?;
        self.write_artifact_manifest(&state.topic_id)?;
        self.event(topic_id, "project.synthesized", json!({}))?;
        Ok(json!({ "action": "synthesize", "topic_id": topic_id, "content": report }))
    }

    pub(crate) fn export(&self, topic_id: &str, format: ExportFormat) -> Result<Value> {
        let state = self.load_state(topic_id)?;
        let report = read_to_string(self.project_root(topic_id).join("report.md"))?;
        let filename = format!(
            "{}-{}.md",
            Utc::now().format("%Y%m%d%H%M%S"),
            match format {
                ExportFormat::Report => "report",
                ExportFormat::Outline => "outline",
                ExportFormat::KnowledgeBase => "knowledge_base",
            }
        );
        let content = match format {
            ExportFormat::Report => report,
            ExportFormat::Outline => format!(
                "# Outline: {}\n\n## Source Research\n\n{report}",
                state.topic
            ),
            ExportFormat::KnowledgeBase => format!(
                "# {}\n\n<!-- Imported from research export. Review before adding to the knowledge base. -->\n\n{report}",
                state.topic
            ),
        };
        let path = self.project_root(topic_id).join("exports").join(filename);
        write_file(&path, &content)?;
        self.event(
            topic_id,
            "project.exported",
            json!({ "path": path, "format": format }),
        )?;
        self.write_artifact_manifest(topic_id)?;
        Ok(json!({ "action": "export", "topic_id": topic_id, "path": path }))
    }

    fn run_round(&self, mut state: State, focus: Option<String>) -> Result<State> {
        let started_at = Instant::now();
        if state.current_round >= state.max_rounds {
            state.status = Status::Completed;
            self.save_state(&state)?;
            return Ok(state);
        }
        state.current_round += 1;
        state.status = Status::Running;
        self.save_state(&state)?;
        self.event(
            &state.topic_id,
            "round.started",
            json!({ "round": state.current_round, "focus": focus }),
        )?;
        let focus_summary = focus.as_deref().unwrap_or("none").to_string();
        let queries = self.plan_queries(&state, focus);
        write_file(
            self.project_root(&state.topic_id).join("agenda.md"),
            &self.render_agenda(&state, &queries),
        )?;
        self.record_agent_run(
            &state,
            AgentRole::Planner,
            "completed",
            format!("round={} focus={}", state.current_round, focus_summary),
            format!("planned {} query direction(s)", queries.len()),
            vec![self.project_root(&state.topic_id).join("agenda.md")],
        )?;
        let mut sources = self.read_jsonl::<SourceRecord>(&state.topic_id, "sources.jsonl")?;
        let mut source_keys = sources
            .iter()
            .map(|source| {
                source
                    .content_key
                    .clone()
                    .unwrap_or_else(|| normalize(&source.summary))
            })
            .collect::<BTreeSet<_>>();
        let mut collected = 0usize;
        let target_sources = effective_target_sources(&state);
        let remaining_sources = (target_sources as usize).saturating_sub(sources.len());
        for query in queries.iter().take(remaining_sources.max(1)) {
            if started_at.elapsed().as_secs() > u64::from(state.max_runtime_minutes) * 60 {
                state.last_error =
                    Some("runtime budget reached before all queries completed".to_string());
                break;
            }
            if collected >= remaining_sources {
                break;
            }
            let search_result = self.search(&state, query);
            self.record_agent_run(
                &state,
                AgentRole::Searcher,
                if search_result.is_ok() {
                    "completed"
                } else {
                    "error"
                },
                query.clone(),
                search_result
                    .as_ref()
                    .map(|hits| format!("retrieved {} candidate hit(s)", hits.len()))
                    .unwrap_or_else(|error| format!("search failed: {error}")),
                vec![
                    self.project_root(&state.topic_id)
                        .join("search_attempts.jsonl"),
                    self.project_root(&state.topic_id)
                        .join("candidate_sources.jsonl"),
                ],
            )?;
            for ranked in rerank_hits(&state, query, search_result?, &self.config.embedding, &self.config.reranker) {
                if started_at.elapsed().as_secs() > u64::from(state.max_runtime_minutes) * 60 {
                    state.last_error =
                        Some("runtime budget reached before all sources completed".to_string());
                    break;
                }
                if collected >= remaining_sources {
                    break;
                }
                let hit = &ranked.hit;
                let accepted_domains: Vec<String> = sources.iter().map(|s| extract_root_domain(&s.url)).collect();
                let gate = evidence_gate(&state, query, hit, &accepted_domains);
                let candidate = CandidateSourceRecord {
                    id: id("candidate"),
                    provider: hit.provider.clone(),
                    query: query.clone(),
                    url: hit.url.clone(),
                    title: hit.title.clone(),
                    summary: hit.summary.clone(),
                    rank: ranked.rank,
                    rerank_score: ranked.rerank_score,
                    rerank_reasons: ranked.rerank_reasons.clone(),
                    score: gate.score,
                    decision: if gate.accepted {
                        "accepted".to_string()
                    } else {
                        "rejected".to_string()
                    },
                    reasons: gate.reasons.clone(),
                    found_at: now(),
                };
                self.append_jsonl(&state.topic_id, "candidate_sources.jsonl", &candidate)?;
                self.record_agent_run(
                    &state,
                    AgentRole::Verifier,
                    "completed",
                    format!("candidate={} provider={}", candidate.id, candidate.provider),
                    format!(
                        "{} source with score {} ({})",
                        candidate.decision,
                        candidate.score,
                        candidate.reasons.join("; ")
                    ),
                    vec![
                        self.project_root(&state.topic_id)
                            .join("candidate_sources.jsonl"),
                        self.project_root(&state.topic_id).join(if gate.accepted {
                            "accepted_sources.jsonl"
                        } else {
                            "rejected_sources.jsonl"
                        }),
                    ],
                )?;
                if !gate.accepted {
                    self.append_jsonl(&state.topic_id, "rejected_sources.jsonl", &candidate)?;
                    self.event(
                        &state.topic_id,
                        "source.rejected",
                        json!({
                            "candidate_id": candidate.id,
                            "provider": candidate.provider,
                            "url": candidate.url,
                            "score": candidate.score,
                            "reasons": candidate.reasons,
                        }),
                    )?;
                    continue;
                }
                self.append_jsonl(&state.topic_id, "accepted_sources.jsonl", &candidate)?;
                let content_key = normalize(&format!("{} {} {}", hit.url, hit.title, hit.summary));
                if !source_keys.insert(content_key.clone()) {
                    continue;
                }
                let source_id = id("src");
                let note_id = id("note");
                let source = SourceRecord {
                    id: source_id.clone(),
                    note_id: Some(note_id.clone()),
                    content_key: Some(content_key),
                    url: hit.url.clone(),
                    title: hit.title.clone(),
                    query: query.clone(),
                    quality_hint: state.quality_hint,
                    source_type: hit.provider.clone(),
                    found_at: now(),
                    fetched_at: Some(now()),
                    status: "fetched".to_string(),
                    summary: hit.summary.clone(),
                    short_excerpt: Some(short_excerpt(&hit.text)),
                    credibility_score: credibility_score(&hit.url),
                    source_authority: source_authority(&hit.url),
                    source_freshness: hit
                        .published_date
                        .as_ref()
                        .map(|_| "known".to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    bias_risk: "unknown".to_string(),
                };
                let chunks = chunk_source(&state, query, &source, hit);
                sources.push(source.clone());
                self.record_source_in_graph(&state, &source)?;
                for chunk in &chunks {
                    self.append_jsonl(&state.topic_id, "chunks.jsonl", chunk)?;
                }
                write_file(
                    self.project_root(&state.topic_id)
                        .join("notes")
                        .join(format!("{note_id}.md")),
                    &self.render_note(&state, &source, &hit),
                )?;
                self.upsert_claims(
                    &state.topic_id,
                    extract_claims(&state.topic, query, &source, hit, &chunks)
                        .into_iter()
                        .map(|text| {
                            ClaimRecord::new(
                                text,
                                vec![source_id.clone()],
                                vec![format!(
                                    "query:{}",
                                    query.chars().take(80).collect::<String>()
                                )],
                            )
                        })
                        .collect(),
                )?;
                self.append_leads(
                    &state.topic_id,
                    follow_up_leads(&state, query, &source, &chunks),
                )?;
                self.append_jsonl(
                    &state.topic_id,
                    "evaluations.jsonl",
                    &json!({
                    "id": id("evaluation"),
                    "source_id": source_id,
                    "quality": "medium",
                    "source_kind": "unknown",
                    "credibility_score": source.credibility_score,
                    "source_authority": source.source_authority,
                    "source_freshness": source.source_freshness,
                    "bias_risk": source.bias_risk,
                    "rationale": format!("Source collected through {} search surface.", hit.provider),
                    "created_at": now(),
                    }),
                )?;
                self.record_agent_run(
                    &state,
                    AgentRole::Reader,
                    "completed",
                    format!("source={} query={}", source.id, query),
                    format!(
                        "wrote {} chunk(s), note {}, and extracted evidence-backed claims/leads",
                        chunks.len(),
                        note_id
                    ),
                    vec![
                        self.project_root(&state.topic_id).join("sources.jsonl"),
                        self.project_root(&state.topic_id).join("chunks.jsonl"),
                        self.project_root(&state.topic_id)
                            .join("notes")
                            .join(format!("{note_id}.md")),
                        self.project_root(&state.topic_id).join("claims.jsonl"),
                        self.project_root(&state.topic_id).join("leads.jsonl"),
                    ],
                )?;
                collected += 1;
                self.write_jsonl(&state.topic_id, "sources.jsonl", &sources)?;
                state.source_count = sources.len();
                state.note_count =
                    read_dir_names(self.project_root(&state.topic_id).join("notes"))?.len();
                state.claim_count = self
                    .read_jsonl::<ClaimRecord>(&state.topic_id, "claims.jsonl")?
                    .len();
                state.link_count = self
                    .read_jsonl::<LinkRecord>(&state.topic_id, "links.jsonl")?
                    .len();
                state.open_leads = self.open_leads(&state.topic_id)?;
                self.save_state(&state)?;
                self.event(
                    &state.topic_id,
                    "source.checkpointed",
                    json!({
                        "round": state.current_round,
                        "source_id": source.id,
                        "note_id": note_id,
                        "query": query,
                        "sources": state.source_count,
                        "claims": state.claim_count,
                        "leads": state.open_leads.len(),
                    }),
                )?;
            }
        }
        // Quality gate: if 0 sources collected and we have prior context, retry with fresh queries
        if collected == 0 && state.current_round > 1 {
            let state_summary = self.build_state_summary(&state);
            if let Some(retry_queries) = self.ai_decompose_queries(
                &state.topic,
                &state.active_directions,
                &[],
                &queries.iter().map(|q| q.clone()).collect::<Vec<_>>(),
                3,
                &state_summary,
            ) {
                for retry_query in retry_queries.iter().take(2) {
                    if collected > 0 { break; }
                    if let Ok(hits) = self.search(&state, retry_query) {
                        for ranked in rerank_hits(&state, retry_query, hits, &self.config.embedding, &self.config.reranker) {
                            if collected > 0 { break; }
                            let accepted_domains: Vec<String> = sources.iter().map(|s| extract_root_domain(&s.url)).collect();
                            let gate = evidence_gate(&state, retry_query, &ranked.hit, &accepted_domains);
                            if gate.accepted {
                                let content_key = normalize(&format!("{} {} {}", ranked.hit.url, ranked.hit.title, ranked.hit.summary));
                                if source_keys.insert(content_key.clone()) {
                                    let source_id = id("src");
                                    let note_id = id("note");
                                    let source = SourceRecord {
                                        id: source_id.clone(),
                                        note_id: Some(note_id.clone()),
                                        content_key: Some(content_key),
                                        url: ranked.hit.url.clone(),
                                        title: ranked.hit.title.clone(),
                                        query: retry_query.clone(),
                                        quality_hint: state.quality_hint,
                                        source_type: ranked.hit.provider.clone(),
                                        found_at: now(),
                                        fetched_at: Some(now()),
                                        status: "fetched".to_string(),
                                        summary: ranked.hit.summary.clone(),
                                        short_excerpt: Some(short_excerpt(&ranked.hit.text)),
                                        credibility_score: credibility_score(&ranked.hit.url),
                                        source_authority: source_authority(&ranked.hit.url),
                                        source_freshness: ranked.hit.published_date.as_ref().map(|_| "known".to_string()).unwrap_or_else(|| "unknown".to_string()),
                                        bias_risk: "unknown".to_string(),
                                    };
                                    let chunks = chunk_source(&state, retry_query, &source, &ranked.hit);
                                    sources.push(source.clone());
                                    self.record_source_in_graph(&state, &source)?;
                                    for chunk in &chunks { self.append_jsonl(&state.topic_id, "chunks.jsonl", chunk)?; }
                                    write_file(self.project_root(&state.topic_id).join("notes").join(format!("{note_id}.md")), &self.render_note(&state, &source, &ranked.hit))?;
                                    self.upsert_claims(&state.topic_id, extract_claims(&state.topic, retry_query, &source, &ranked.hit, &chunks).into_iter().map(|text| ClaimRecord::new(text, vec![source_id.clone()], vec![format!("query:{}", retry_query.chars().take(80).collect::<String>())])).collect())?;
                                    self.append_leads(&state.topic_id, follow_up_leads(&state, retry_query, &source, &chunks))?;
                                    collected += 1;
                                    self.write_jsonl(&state.topic_id, "sources.jsonl", &sources)?;
                                    state.source_count = sources.len();
                                    self.save_state(&state)?;
                                }
                            }
                        }
                    }
                }
            }
        }
        self.write_jsonl(&state.topic_id, "sources.jsonl", &sources)?;
        self.find_links(&state.topic_id)?;
        self.record_agent_run(
            &state,
            AgentRole::Linker,
            "completed",
            format!("round={} sources={}", state.current_round, sources.len()),
            "refreshed graph links and relationship mirrors",
            vec![
                self.project_root(&state.topic_id).join("links.jsonl"),
                self.project_root(&state.topic_id).join("links.md"),
                self.graph_path(&state.topic_id),
            ],
        )?;
        state.source_count = sources.len();
        state.note_count = read_dir_names(self.project_root(&state.topic_id).join("notes"))?.len();
        state.claim_count = self
            .read_jsonl::<ClaimRecord>(&state.topic_id, "claims.jsonl")?
            .len();
        state.link_count = self
            .read_jsonl::<LinkRecord>(&state.topic_id, "links.jsonl")?
            .len();
        state.open_leads = self.open_leads(&state.topic_id)?;
        state.status = if should_complete_after_round(&state, collected) {
            Status::Completed
        } else {
            Status::Paused
        };
        self.save_state(&state)?;
        self.write_plan(&state)?;
        self.refresh_mirrors(&state)?;
        self.synthesize(&state.topic_id)?;
        if self.has_agent(&state, AgentRole::Reviewer) {
            self.review_report(&state)?;
        }
        self.event(
            &state.topic_id,
            "round.completed",
            json!({ "round": state.current_round, "collected": collected }),
        )?;
        Ok(state)
    }

    fn review_report(&self, state: &State) -> Result<()> {
        let report = read_to_string(self.project_root(&state.topic_id).join("report.md"))?;
        let sources = self.read_jsonl::<SourceRecord>(&state.topic_id, "sources.jsonl")?;
        let claims = self.read_jsonl::<ClaimRecord>(&state.topic_id, "claims.jsonl")?;
        let candidates =
            self.read_jsonl::<CandidateSourceRecord>(&state.topic_id, "candidate_sources.jsonl")?;
        let rejected = candidates
            .iter()
            .filter(|candidate| candidate.decision == "rejected")
            .count();
        let missing_source_refs = claims
            .iter()
            .filter(|claim| claim.source_ids.is_empty())
            .count();
        let review = [
            format!("# Report Review: {}", state.topic),
            String::new(),
            format!("Generated: {}", now()),
            String::new(),
            "## Reviewer Verdict".to_string(),
            String::new(),
            if sources.is_empty() {
                "- The report has no accepted evidence and should be treated as incomplete."
                    .to_string()
            } else if missing_source_refs > 0 {
                format!(
                    "- The report is usable but {} claim(s) lack source references and need follow-up.",
                    missing_source_refs
                )
            } else {
                "- The report is ready for human review; claims have source linkage and rejected candidates are preserved for audit."
                    .to_string()
            },
            String::new(),
            "## Coverage Checks".to_string(),
            String::new(),
            format!("- Accepted sources: {}", sources.len()),
            format!("- Extracted claims: {}", claims.len()),
            format!("- Rejected/weak candidates retained for audit: {}", rejected),
            format!("- Report length: {} characters", report.chars().count()),
            String::new(),
            "## Follow-up Advice".to_string(),
            String::new(),
            if state.search_level == SearchLevel::Research {
                "- Continue with focused resume rounds for unresolved leads, contradictions, and missing primary-source evidence."
                    .to_string()
            } else {
                "- Use `resume --focus` or rerun at `research` level if the user needs a stronger adversarial review."
                    .to_string()
            },
        ]
        .join("\n");
        let path = self.project_root(&state.topic_id).join("report_review.md");
        write_file(&path, &review)?;
        self.record_agent_run(
            state,
            AgentRole::Reviewer,
            "completed",
            format!(
                "report={} sources={} claims={} rejected_candidates={}",
                self.project_root(&state.topic_id)
                    .join("report.md")
                    .display(),
                sources.len(),
                claims.len(),
                rejected
            ),
            "reviewed final report for source coverage, auditability, and follow-up needs",
            vec![path],
        )
    }

    fn render_no_accepted_evidence_report(&self, state: &State) -> String {
        [
            format!("# {}", state.topic),
            String::new(),
            "## No Accepted Evidence".to_string(),
            String::new(),
            "This research round did not find any candidate source that passed the evidence gate."
                .to_string(),
            String::new(),
            "The search providers may have returned unrelated pages, weak matches, or results for different topics. Those candidates were persisted in `candidate_sources.jsonl` and rejected candidates were persisted in `rejected_sources.jsonl` for inspection."
                .to_string(),
            String::new(),
            "## Recommended Next Step".to_string(),
            String::new(),
            "- Re-run with a more specific query, add authoritative domains, or use hybrid search so another provider can supply acceptable evidence.".to_string(),
        ]
        .join("\n")
    }

    fn render_deterministic_report(
        &self,
        state: &State,
        sources: &[SourceRecord],
        chunks: &[SourceChunkRecord],
        candidates: &[CandidateSourceRecord],
        claims: &[ClaimRecord],
        links: &[LinkRecord],
        open_leads: &[&LeadRecord],
        contradictions: &[&LinkRecord],
        high_confidence_sources: usize,
    ) -> String {
        [
            format!("# {}", state.topic),
            String::new(),
            "## Executive Summary".to_string(),
            String::new(),
            format!(
                "This report presents the current best answer on **{}** based on the collected evidence. It is intended for a human reader who needs conclusions, context, and caveats rather than raw research logs.",
                state.topic
            ),
            format!(
                "The analysis currently draws on {} source(s), {} extracted claim(s), and {} relationship edge(s).",
                sources.len(),
                claims.len(),
                links.len()
            ),
            format!(
                "{} source(s) currently look higher-authority by deterministic scoring. {} unresolved research direction(s) remain relevant for follow-up.",
                high_confidence_sources,
                open_leads.len()
            ),
            String::new(),
            "## Research Scope".to_string(),
            String::new(),
            format!("- Topic: {}", state.topic),
            format!("- Topic ID: `{}`", state.topic_id),
            format!("- Generated: {}", now()),
            format!("- Status: {}", state.status),
            format!("- Search level: {}", state.search_level),
            format!("- Search provider: {}", state.search_provider),
            state
                .research_model
                .as_ref()
                .map(|model| format!("Research model: `{model}`"))
                .unwrap_or_default(),
            String::new(),
            "## Key Findings".to_string(),
            String::new(),
            render_numbered_findings(&claims),
            String::new(),
            "## Multi-Perspective Analysis".to_string(),
            String::new(),
            render_perspectives(&claims, &links, &sources),
            String::new(),
            "## Evidence Base".to_string(),
            String::new(),
            render_evidence_base(&sources),
            String::new(),
            "## Evidence Excerpts".to_string(),
            String::new(),
            render_chunk_excerpts(chunks),
            String::new(),
            "## Search Quality".to_string(),
            String::new(),
            render_search_quality(candidates),
            String::new(),
            "## Claim Details".to_string(),
            String::new(),
            claims
                .iter()
                .map(|claim| {
                    format!(
                        "### {}\n\n{}\n\n- Confidence: {}\n- Status: {}\n- Sources: {}",
                        claim.id,
                        claim.text,
                        claim.confidence,
                        claim.status,
                        claim.source_ids.join(", ").if_empty("none")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n")
                .if_empty("No claims have been extracted yet."),
            String::new(),
            "## Relationship Map".to_string(),
            String::new(),
            links
                .iter()
                .map(|link| {
                    format!(
                        "- **{}** {} {} {}: {}",
                        link.id, link.from, link.link_type, link.to, link.rationale
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
                .if_empty("- No relationship edges have been identified yet."),
            String::new(),
            "## Tensions, Counterpoints, And Uncertainty".to_string(),
            String::new(),
            contradictions
                .iter()
                .map(|link| format!("- {} -> {}: {}", link.from, link.to, link.rationale))
                .collect::<Vec<_>>()
                .join("\n")
                .if_empty("- No explicit contradictions have been identified yet. Treat this as absence of detected contradiction, not proof that no disagreement exists."),
            String::new(),
            "## Confidence And Limitations".to_string(),
            String::new(),
            render_limitations(&sources, &claims, open_leads.len()),
            String::new(),
            "## Recommended Next Steps".to_string(),
            String::new(),
            open_leads
                .iter()
                .map(|lead| format!("- {}", lead.direction))
                .collect::<Vec<_>>()
                .join("\n")
                .if_empty("- No open follow-up leads are currently recorded."),
        ]
        .join("\n")
    }

    /// Build a compressed research summary across all rounds for writer context.
    fn build_research_summary(&self, state: &State, claims: &[ClaimRecord], sources: &[SourceRecord]) -> String {
        let mut parts = Vec::new();
        parts.push(format!("研究主题：{}", state.topic));
        parts.push(format!("已完成 {} 轮搜索，共 {} 个源、{} 条结论", state.current_round, sources.len(), claims.len()));

        // Group claims by status
        let supported: Vec<&str> = claims.iter().filter(|c| c.status == "supported").map(|c| c.text.as_str()).collect();
        let contested: Vec<&str> = claims.iter().filter(|c| c.status == "contested").map(|c| c.text.as_str()).collect();
        let unverified: Vec<&str> = claims.iter().filter(|c| c.status == "unverified").map(|c| c.text.as_str()).collect();
        if !supported.is_empty() {
            parts.push(format!("已证实结论（{}）：{}", supported.len(), supported.iter().take(8).map(|c| format!("- {}", c)).collect::<Vec<_>>().join(" ")));
        }
        if !contested.is_empty() {
            parts.push(format!("有争议结论（{}）：{}", contested.len(), contested.iter().take(5).map(|c| format!("- {}", c)).collect::<Vec<_>>().join(" ")));
        }
        if !unverified.is_empty() {
            parts.push(format!("待验证结论（{}）：{}", unverified.len(), unverified.iter().take(5).map(|c| format!("- {}", c)).collect::<Vec<_>>().join(" ")));
        }

        // Source authority distribution
        let academic = sources.iter().filter(|s| s.source_authority == "academic").count();
        let official = sources.iter().filter(|s| s.source_authority == "official").count();
        let media = sources.iter().filter(|s| s.source_authority == "media").count();
        parts.push(format!("源分布：学术{}、官方{}、媒体{}", academic, official, media));

        parts.join("\n")
    }

    fn generate_model_report(
        &self,
        state: &State,
        sources: &[SourceRecord],
        chunks: &[SourceChunkRecord],
        candidates: &[CandidateSourceRecord],
        claims: &[ClaimRecord],
        links: &[LinkRecord],
        leads: &[LeadRecord],
    ) -> Result<Option<String>> {
        let Some((api_url, api_key, model)) = self.config.resolve_agent("writer") else {
            return Ok(None);
        };
        let endpoint = format!("{}/v1/chat/completions", api_url.trim_end_matches('/'));
        let research_summary = self.build_research_summary(state, claims, sources);
        let prompt = json!({
            "topic": state.topic,
            "topic_id": state.topic_id,
            "status": state.status,
            "search_level": state.search_level,
            "generated_at": now(),
            "research_summary": research_summary,
            "sources": sources.iter().take(20).map(|source| json!({
                "id": source.id,
                "title": source.title,
                "url": source.url,
                "summary": source.summary,
                "excerpt": source.short_excerpt,
                "credibility_score": source.credibility_score,
                "authority": source.source_authority,
                "freshness": source.source_freshness,
            })).collect::<Vec<_>>(),
            "source_chunks": chunks.iter()
                .filter(|chunk| chunk.relevance_score >= 0.05)
                .take(60)
                .map(|chunk| json!({
                    "id": chunk.id,
                    "source_id": chunk.source_id,
                    "chunk_index": chunk.chunk_index,
                    "title": chunk.title,
                    "url": chunk.url,
                    "text": chunk.text,
                    "relevance_score": chunk.relevance_score,
                }))
                .collect::<Vec<_>>(),
            "search_quality": candidates.iter().take(80).map(|candidate| json!({
                "id": candidate.id,
                "provider": candidate.provider,
                "query": candidate.query,
                "title": candidate.title,
                "url": candidate.url,
                "decision": candidate.decision,
                "gate_score": candidate.score,
                "gate_reasons": candidate.reasons,
                "rerank_score": candidate.rerank_score,
                "rerank_reasons": candidate.rerank_reasons,
            })).collect::<Vec<_>>(),
            "claims": claims.iter().take(40).map(|claim| json!({
                "id": claim.id,
                "text": claim.text,
                "source_ids": claim.source_ids,
                "confidence": claim.confidence,
                "status": claim.status,
            })).collect::<Vec<_>>(),
            "links": links.iter().take(40).map(|link| json!({
                "id": link.id,
                "from": link.from,
                "to": link.to,
                "type": link.link_type,
                "rationale": link.rationale,
                "source_ids": link.source_ids,
                "confidence": link.confidence,
            })).collect::<Vec<_>>(),
            "open_leads": leads.iter().filter(|lead| lead.status == "open").take(20).map(|lead| json!({
                "direction": lead.direction,
                "reason": lead.reason,
                "priority": lead.priority,
                "expected_information_gain": lead.expected_information_gain,
            })).collect::<Vec<_>>(),
        });
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(self.config.timeouts.ai_timeout_secs))
            .build()?
            .post(endpoint)
            .bearer_auth(api_key)
            .json(&json!({
                "model": model,
                "max_tokens": 4000,
                "messages": [{
                    "role": "user",
                    "content": format!(
                        "你是资深研究分析师。请基于下面 JSON 资料，写一份给人看的中文深度研究报告。主题可能来自任何领域，不要套用固定行业模板；请根据 topic 自适应报告结构。不要复述内部日志、checkpoint 或工具流程。报告必须专业、多视角、有深度，至少包含：执行摘要、研究范围、关键发现、证据矩阵、多视角分析、反方证据或不确定性、结论、行动建议、引用索引。不要自行编写报告日期或生成时间，系统会在报告头部添加。过滤网页噪声、营销文字和无关内容。所有事实必须能回溯到 source id、chunk id 或 URL；不确定的地方明确标注，不要编造。资料：{}",
                        serde_json::to_string(&prompt)?
                    )
                }]
            }))
            .send()
            .context("failed to call configured research model API")?;
        if !response.status().is_success() {
            bail!(
                "research model API failed with {}: {}",
                response.status(),
                response.text().unwrap_or_default()
            )
        }
        Ok(Some(extract_model_text(
            response.json::<ModelMessageResponse>()?,
        )))
    }

    /// Build a research state summary for planner context.
    fn build_state_summary(&self, state: &State) -> String {
        let claims = self.read_jsonl::<ClaimRecord>(&state.topic_id, "claims.jsonl").unwrap_or_default();
        let attempts = self.read_jsonl::<SearchAttemptRecord>(&state.topic_id, "search_attempts.jsonl").unwrap_or_default();
        let productive: Vec<&str> = attempts.iter().filter(|a| a.was_productive).map(|a| a.query.as_str()).collect();
        let failed: Vec<&str> = attempts.iter().filter(|a| !a.was_productive && a.result_count == 0).map(|a| a.query.as_str()).collect();
        let mut parts = vec![
            format!("已接受 {} 个源，提取 {} 条结论，{} 个开放线索", state.source_count, state.claim_count, state.open_leads.len()),
        ];
        if !productive.is_empty() {
            parts.push(format!("有效查询：{}", productive.iter().take(5).map(|q| format!("\"{}\"", q)).collect::<Vec<_>>().join(", ")));
        }
        if !failed.is_empty() {
            parts.push(format!("无效查询（结果为0）：{}", failed.iter().take(5).map(|q| format!("\"{}\"", q)).collect::<Vec<_>>().join(", ")));
        }
        if !claims.is_empty() {
            let top_claims: Vec<&str> = claims.iter().take(3).map(|c| c.text.as_str()).collect();
            parts.push(format!("已有关键结论：{}", top_claims.join("; ")));
        }
        parts.join("\n")
    }

    /// Use AI to decompose a topic into targeted search queries.
    /// Falls back to None if AI is not configured or fails.
    fn ai_decompose_queries(
        &self,
        topic: &str,
        directions: &[String],
        leads: &[String],
        past_queries: &[String],
        max_queries: u32,
        state_summary: &str,
    ) -> Option<Vec<String>> {
        let (api_url, api_key, model) = self.config.resolve_agent("planner")?;
        let endpoint = format!("{}/v1/chat/completions", api_url.trim_end_matches('/'));
        let direction_context = if directions.is_empty() {
            String::new()
        } else {
            format!("\n研究方向：\n{}", directions.iter().enumerate().map(|(i, d)| format!("{}. {}", i + 1, d)).collect::<Vec<_>>().join("\n"))
        };
        let lead_context = if leads.is_empty() {
            String::new()
        } else {
            format!("\n待探索线索：\n{}", leads.iter().enumerate().map(|(i, l)| format!("{}. {}", i + 1, l)).collect::<Vec<_>>().join("\n"))
        };
        let past_context = if past_queries.is_empty() {
            String::new()
        } else {
            format!("\n\n已经搜索过的查询（请避免重复或过于相似的查询）：\n{}", past_queries.iter().take(20).enumerate().map(|(i, q)| format!("{}. {}", i + 1, q)).collect::<Vec<_>>().join("\n"))
        };
        let summary_context = if state_summary.is_empty() {
            String::new()
        } else {
            format!("\n\n当前研究状态：\n{}", state_summary)
        };
        let prompt = format!(
            "你是一个搜索引擎查询优化专家。请将以下研究主题分解为 {max_queries} 个多样化的搜索查询，覆盖不同角度和维度。\n\n\
             主题：{topic}{direction_context}{lead_context}{past_context}{summary_context}\n\n\
             要求：\n\
             - 每个查询应该是独立的搜索词，适合直接输入搜索引擎\n\
             - 覆盖：概述/定义、核心机制、最新进展、争议/批评、实际应用/案例\n\
             - 避免重复或过于相似的查询\n\
             - 如果有研究状态信息，请基于已有结论设计补充查询，填补证据空白\n\
             - 使用英文搜索词（覆盖面更广），除非主题本身是中文特有的\n\n\
             请只返回 JSON 数组，不要其他内容。例如：[\"query 1\", \"query 2\", \"query 3\"]"
        );
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(self.config.timeouts.ai_timeout_secs))
            .build()
            .ok()?
            .post(endpoint)
            .bearer_auth(api_key)
            .json(&json!({
                "model": model,
                "max_tokens": 1000,
                "messages": [{"role": "user", "content": prompt}]
            }))
            .send()
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let text = extract_model_text(response.json::<ModelMessageResponse>().ok()?);
        // Try to parse JSON array from response
        let trimmed = text.trim();
        // Find the first [ and last ]
        let start = trimmed.find('[')?;
        let end = trimmed.rfind(']')? + 1;
        let arr: Vec<String> = serde_json::from_str(&trimmed[start..end]).ok()?;
        if arr.is_empty() {
            return None;
        }
        Some(arr)
    }

    fn plan_queries(&self, state: &State, focus: Option<String>) -> Vec<String> {
        let mut queries = Vec::new();

        // Collect context for AI decomposition
        let mut directions = Vec::new();
        let mut leads = Vec::new();

        // Get past queries for deduplication
        let past = self.past_queries(&state.topic_id);

        // Build research state summary for planner context
        let state_summary = self.build_state_summary(state);

        // Adaptive pipeline: reduce query count in later rounds
        let adaptive_max = if state.current_round <= 1 {
            state.max_sources
        } else {
            state.max_sources.saturating_sub(state.current_round).max(3)
        };

        if let Some(focus) = focus {
            // Focused round: deterministic queries for precision
            queries.push(format!(
                "{} {} evidence primary sources",
                state.topic, focus
            ));
            queries.push(format!(
                "{} {} criticism counter evidence",
                state.topic, focus
            ));
        } else if state.active_directions.is_empty() {
            // No directions yet: try AI decomposition, fallback to overview
            if let Some(ai_queries) = self.ai_decompose_queries(
                &state.topic,
                &[],
                &[],
                &past,
                adaptive_max,
                &state_summary,
            ) {
                queries.extend(ai_queries);
            } else {
                queries.push(format!("{} overview primary sources", state.topic));
            }
        } else {
            directions = state.active_directions.clone();
        }

        // Collect leads
        for lead in self
            .open_leads(&state.topic_id)
            .unwrap_or_default()
            .into_iter()
            .take(4)
        {
            leads.push(lead);
        }

        // If we have directions but no queries yet (non-focused round with directions),
        // try AI decomposition with full context
        if queries.is_empty() && !directions.is_empty() {
            if let Some(ai_queries) = self.ai_decompose_queries(
                &state.topic,
                &directions,
                &leads,
                &past,
                adaptive_max,
                &state_summary,
            ) {
                queries.extend(ai_queries);
            } else {
                // Fallback: deterministic queries from directions
                queries.extend(
                    directions
                        .iter()
                        .map(|direction| format!("{} {}", state.topic, direction)),
                );
            }
        }

        // Append lead-based queries (both AI and fallback paths)
        for lead in leads {
            queries.push(format!("{} {}", state.topic, lead));
        }

        // Deduplicate against past queries and within the set
        let queries = deduplicate_queries(unique_strings(queries), &past);

        queries
            .into_iter()
            .take(state.max_sources as usize)
            .collect()
    }

    fn model_for_role(&self, state: &State, role: AgentRole) -> Option<String> {
        match role {
            AgentRole::Writer | AgentRole::Reviewer => state.research_model.clone(),
            _ => None,
        }
    }

    fn has_agent(&self, state: &State, role: AgentRole) -> bool {
        state.agent_roster.iter().any(|r| r == &role.to_string())
    }

    pub(crate) fn ensure_viewer(&self, state: &mut State) -> Result<()> {
        viewer::ensure_viewer(&self.root, &self.config, state, |state| self.save_state(state), |topic_id, event_type, data| self.event(topic_id, event_type, data))
    }

    pub(crate) fn serve(&self, topic_id: &str, host: Option<String>, port: Option<u16>) -> Result<Value> {
        viewer::serve(topic_id, host, port, &self.config, |stream, topic_id| self.handle_http(stream, topic_id))
    }

    pub(crate) fn handle_http(&self, mut stream: TcpStream, topic_id: &str) -> Result<()> {
        let mut buffer = [0_u8; 4096];
        let size = stream.read(&mut buffer)?;
        let request = String::from_utf8_lossy(&buffer[..size]);
        let path = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let (content_type, body) = if path.starts_with("/api/state") {
            (
                "application/json; charset=utf-8",
                serde_json::to_string(&self.viewer_state(topic_id)?)?,
            )
        } else {
            ("text/html; charset=utf-8", viewer_html(topic_id, None)?)
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes())?;
        Ok(())
    }

    pub(crate) fn viewer_state(&self, topic_id: &str) -> Result<Value> {
        let state = self.load_state(topic_id)?;
        let graph = self
            .load_graph(topic_id)
            .or_else(|_| self.sync_graph(topic_id))?;
        let sources = self.read_jsonl::<SourceRecord>(topic_id, "sources.jsonl")?;
        let chunks = self.read_jsonl::<SourceChunkRecord>(topic_id, "chunks.jsonl")?;
        let claims = self.read_jsonl::<ClaimRecord>(topic_id, "claims.jsonl")?;
        let links = self.read_jsonl::<LinkRecord>(topic_id, "links.jsonl")?;
        let leads = self.read_jsonl::<LeadRecord>(topic_id, "leads.jsonl")?;
        let events = self.read_jsonl::<Value>(topic_id, "logs/events.jsonl")?;
        let candidates =
            self.read_jsonl::<CandidateSourceRecord>(topic_id, "candidate_sources.jsonl")?;
        let accepted =
            self.read_jsonl::<CandidateSourceRecord>(topic_id, "accepted_sources.jsonl")?;
        let rejected =
            self.read_jsonl::<CandidateSourceRecord>(topic_id, "rejected_sources.jsonl")?;
        Ok(json!({
            "state": state,
            "graph": graph,
            "sources": sources,
            "chunks": chunks,
            "search_attempts": self.read_jsonl::<SearchAttemptRecord>(topic_id, "search_attempts.jsonl")?,
            "agent_runs": self.read_jsonl::<AgentRunRecord>(topic_id, "agent_runs.jsonl")?,
            "candidate_sources": candidates,
            "accepted_sources": accepted,
            "rejected_sources": rejected,
            "claims": claims,
            "links": links,
            "leads": leads,
            "events": events,
            "report": read_to_string(self.project_root(topic_id).join("report.md")).unwrap_or_default(),
            "resume_summary": read_to_string(self.project_root(topic_id).join("resume_summary.md")).unwrap_or_default(),
        }))
    }

    pub(crate) fn snapshot_path(&self, topic_id: &str) -> PathBuf {
        self.project_root(topic_id).join("snapshot.html")
    }

    pub(crate) fn report_path(&self, topic_id: &str) -> PathBuf {
        self.project_root(topic_id).join("report.md")
    }

    pub(crate) fn manifest_path(&self, topic_id: &str) -> PathBuf {
        self.project_root(topic_id).join("manifest.json")
    }

    pub(crate) fn project_root(&self, topic_id: &str) -> PathBuf {
        self.root.join(slug(topic_id))
    }

    pub(crate) fn artifact_path(&self, topic_id: &str, target: ReadTarget) -> PathBuf {
        let root = self.project_root(topic_id);
        match target {
            ReadTarget::State => root.join("state.json"),
            ReadTarget::Agenda => root.join("agenda.md"),
            ReadTarget::Questions => root.join("questions.md"),
            ReadTarget::Sources => root.join("sources.jsonl"),
            ReadTarget::Chunks => root.join("chunks.jsonl"),
            ReadTarget::CandidateSources => root.join("candidate_sources.jsonl"),
            ReadTarget::AcceptedSources => root.join("accepted_sources.jsonl"),
            ReadTarget::RejectedSources => root.join("rejected_sources.jsonl"),
            ReadTarget::SearchAttempts => root.join("search_attempts.jsonl"),
            ReadTarget::AgentRuns => root.join("agent_runs.jsonl"),
            ReadTarget::Notes => root.join("notes"),
            ReadTarget::Claims => root.join("claims.md"),
            ReadTarget::Evidence => root.join("evidence.md"),
            ReadTarget::Entities => root.join("entities.md"),
            ReadTarget::Links => root.join("links.md"),
            ReadTarget::Insights => root.join("insights.md"),
            ReadTarget::Leads => root.join("leads.jsonl"),
            ReadTarget::Timeline => root.join("timeline.md"),
            ReadTarget::Gaps => root.join("gaps.md"),
            ReadTarget::Evaluations => root.join("evaluations.md"),
            ReadTarget::Decisions => root.join("decisions.md"),
            ReadTarget::Threads => root.join("threads.md"),
            ReadTarget::Report => root.join("report.md"),
            ReadTarget::ReportReview => root.join("report_review.md"),
            ReadTarget::Refine => root.join("refine.md"),
            ReadTarget::ResumeSummary => root.join("resume_summary.md"),
            ReadTarget::ClaimEvents => root.join("claim_events.md"),
            ReadTarget::Answers => root.join("answers.md"),
            ReadTarget::Events => root.join("logs").join("events.jsonl"),
            ReadTarget::Plan => root.join("plan.md"),
        }
    }

    pub(crate) fn graph_path(&self, topic_id: &str) -> PathBuf {
        self.project_root(topic_id).join("graph.json")
    }

    pub(crate) fn append_leads(&self, topic_id: &str, leads: Vec<LeadRecord>) -> Result<()> {
        let mut existing = self.read_jsonl::<LeadRecord>(topic_id, "leads.jsonl")?;
        let mut touched = Vec::new();
        for lead in leads {
            if let Some(found) = existing
                .iter_mut()
                .find(|item| same_research_idea(&item.direction, &lead.direction))
            {
                if priority_rank(lead.priority) > priority_rank(found.priority) {
                    found.priority = lead.priority;
                }
                found.merged_from.push(lead.id);
                found.updated_at = Some(now());
                touched.push(found.clone());
            } else {
                touched.push(lead.clone());
                existing.push(lead);
            }
        }
        self.write_jsonl(topic_id, "leads.jsonl", &existing)?;
        self.record_leads_in_graph(topic_id, &touched)
    }

    pub(crate) fn upsert_claims(&self, topic_id: &str, claims: Vec<ClaimRecord>) -> Result<()> {
        let mut existing = self.read_jsonl::<ClaimRecord>(topic_id, "claims.jsonl")?;
        let mut touched = Vec::new();
        for claim in claims {
            if let Some(found) = existing
                .iter_mut()
                .find(|item| same_research_idea(&item.text, &claim.text))
            {
                found.source_ids = merge(found.source_ids.clone(), claim.source_ids);
                found.tags = merge(found.tags.clone(), claim.tags);
                found.merged_from.push(claim.id);
                found.updated_at = Some(now());
                touched.push(found.clone());
            } else {
                self.append_jsonl(
                    topic_id,
                    "claim_events.jsonl",
                    &json!({
                        "id": id("claim_event"),
                        "claim_id": claim.id,
                        "event": "created",
                        "to_confidence": claim.confidence,
                        "to_status": claim.status,
                        "source_ids": claim.source_ids,
                        "link_ids": Vec::<String>::new(),
                        "reason": "Created by deterministic Rust CLI round.",
                        "created_at": now(),
                    }),
                )?;
                touched.push(claim.clone());
                existing.push(claim);
            }
        }
        self.write_jsonl(topic_id, "claims.jsonl", &existing)?;
        self.record_claims_in_graph(topic_id, &touched)
    }
}
