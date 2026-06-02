use super::*;

impl Lab {
    pub(crate) fn load_graph(&self, topic_id: &str) -> Result<GraphData> {
        serde_json::from_str(&read_to_string(self.graph_path(topic_id))?)
            .with_context(|| format!("failed to parse graph for {topic_id}"))
    }

    pub(crate) fn save_graph(&self, topic_id: &str, graph: &GraphData) -> Result<()> {
        write_file(
            self.graph_path(topic_id),
            &serde_json::to_string_pretty(graph)?,
        )
    }

    pub(crate) fn ensure_graph(&self, state: &State) -> Result<GraphData> {
        let mut graph = self
            .load_graph(&state.topic_id)
            .or_else(|_| self.sync_graph(&state.topic_id))?;
        graph.root_id = root_graph_id(&state.topic_id);
        upsert_graph_node(&mut graph.nodes, root_graph_node(state));
        graph.updated_at = now();
        self.save_graph(&state.topic_id, &graph)?;
        Ok(graph)
    }

    pub(crate) fn mutate_graph<F>(&self, topic_id: &str, update: F) -> Result<()>
    where
        F: FnOnce(&mut GraphData),
    {
        let mut graph = self
            .load_graph(topic_id)
            .or_else(|_| self.sync_graph(topic_id))?;
        update(&mut graph);
        graph.updated_at = now();
        self.save_graph(topic_id, &graph)
    }

    pub(crate) fn sync_graph(&self, topic_id: &str) -> Result<GraphData> {
        let graph = self.build_graph(&self.load_state(topic_id)?)?;
        self.save_graph(topic_id, &graph)?;
        Ok(graph)
    }

    pub(crate) fn record_source_in_graph(&self, state: &State, source: &SourceRecord) -> Result<()> {
        self.ensure_graph(state)?;
        self.mutate_graph(&state.topic_id, |graph| {
            upsert_graph_node(&mut graph.nodes, source_graph_node(source));
            upsert_graph_edge(
                &mut graph.edges,
                graph_edge(
                    &graph.root_id,
                    &source.id,
                    "collects_source",
                    Some(source.query.clone()),
                    &source.found_at,
                ),
            );
        })
    }

    pub(crate) fn record_claims_in_graph(&self, topic_id: &str, claims: &[ClaimRecord]) -> Result<()> {
        let state = self.load_state(topic_id)?;
        self.ensure_graph(&state)?;
        self.mutate_graph(topic_id, |graph| {
            for claim in claims {
                upsert_graph_node(&mut graph.nodes, claim_graph_node(claim));
                upsert_graph_edge(
                    &mut graph.edges,
                    graph_edge(
                        &graph.root_id,
                        &claim.id,
                        "tracks_claim",
                        Some(claim.status.clone()),
                        &claim.created_at,
                    ),
                );
                for source_id in &claim.source_ids {
                    upsert_graph_edge(
                        &mut graph.edges,
                        graph_edge(
                            &claim.id,
                            source_id,
                            "supported_by",
                            None,
                            &claim.created_at,
                        ),
                    );
                }
            }
        })
    }

    pub(crate) fn record_leads_in_graph(&self, topic_id: &str, leads: &[LeadRecord]) -> Result<()> {
        let state = self.load_state(topic_id)?;
        let sources = self.read_jsonl::<SourceRecord>(topic_id, "sources.jsonl")?;
        self.ensure_graph(&state)?;
        self.mutate_graph(topic_id, |graph| {
            for lead in leads {
                upsert_graph_node(&mut graph.nodes, lead_graph_node(lead));
                upsert_graph_edge(
                    &mut graph.edges,
                    graph_edge(
                        &graph.root_id,
                        &lead.id,
                        "tracks_lead",
                        lead.reason.clone(),
                        &lead.created_at,
                    ),
                );
                for source in &sources {
                    if lead
                        .reason
                        .as_deref()
                        .unwrap_or_default()
                        .contains(&source.id)
                    {
                        upsert_graph_edge(
                            &mut graph.edges,
                            graph_edge(
                                &source.id,
                                &lead.id,
                                "suggests_lead",
                                lead.reason.clone(),
                                &lead.created_at,
                            ),
                        );
                    }
                }
            }
        })
    }

    pub(crate) fn record_links_in_graph(&self, topic_id: &str, links: &[LinkRecord]) -> Result<()> {
        let state = self.load_state(topic_id)?;
        self.ensure_graph(&state)?;
        self.mutate_graph(topic_id, |graph| {
            for link in links {
                upsert_graph_edge(
                    &mut graph.edges,
                    graph_edge(
                        &link.from,
                        &link.to,
                        &link.link_type,
                        Some(link.rationale.clone()),
                        &link.created_at,
                    ),
                );
            }
        })
    }

    pub(crate) fn build_graph(&self, state: &State) -> Result<GraphData> {
        let root_id = root_graph_id(&state.topic_id);
        let sources = self.read_jsonl::<SourceRecord>(&state.topic_id, "sources.jsonl")?;
        let claims = self.read_jsonl::<ClaimRecord>(&state.topic_id, "claims.jsonl")?;
        let leads = self.read_jsonl::<LeadRecord>(&state.topic_id, "leads.jsonl")?;
        let links = self.read_jsonl::<LinkRecord>(&state.topic_id, "links.jsonl")?;
        let mut nodes = vec![root_graph_node(state)];
        nodes.extend(sources.iter().map(source_graph_node));
        nodes.extend(claims.iter().map(claim_graph_node));
        nodes.extend(leads.iter().map(lead_graph_node));
        let mut edges = Vec::new();
        let mut seen = BTreeSet::new();
        for source in &sources {
            push_graph_edge(
                &mut edges,
                &mut seen,
                root_id.clone(),
                source.id.clone(),
                "collects_source".to_string(),
                Some(source.query.clone()),
                source.found_at.clone(),
            );
        }
        for claim in &claims {
            push_graph_edge(
                &mut edges,
                &mut seen,
                root_id.clone(),
                claim.id.clone(),
                "tracks_claim".to_string(),
                Some(claim.status.clone()),
                claim.created_at.clone(),
            );
            for source_id in &claim.source_ids {
                push_graph_edge(
                    &mut edges,
                    &mut seen,
                    claim.id.clone(),
                    source_id.clone(),
                    "supported_by".to_string(),
                    None,
                    claim.created_at.clone(),
                );
            }
        }
        for lead in &leads {
            push_graph_edge(
                &mut edges,
                &mut seen,
                root_id.clone(),
                lead.id.clone(),
                "tracks_lead".to_string(),
                lead.reason.clone(),
                lead.created_at.clone(),
            );
            for source in &sources {
                if lead
                    .reason
                    .as_deref()
                    .unwrap_or_default()
                    .contains(&source.id)
                {
                    push_graph_edge(
                        &mut edges,
                        &mut seen,
                        source.id.clone(),
                        lead.id.clone(),
                        "suggests_lead".to_string(),
                        lead.reason.clone(),
                        lead.created_at.clone(),
                    );
                }
            }
        }
        for link in &links {
            push_graph_edge(
                &mut edges,
                &mut seen,
                link.from.clone(),
                link.to.clone(),
                link.link_type.clone(),
                Some(link.rationale.clone()),
                link.created_at.clone(),
            );
        }
        Ok(GraphData {
            root_id,
            nodes,
            edges,
            updated_at: now(),
        })
    }
}
