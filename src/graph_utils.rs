use crate::{ClaimRecord, GraphEdge, GraphNode, LeadRecord, SourceRecord, State};

pub(crate) fn link_key(from: &str, to: &str, link_type: &str) -> String {
    format!("{from}->{to}:{link_type}")
}

pub(crate) fn root_graph_id(topic_id: &str) -> String {
    format!("question:{topic_id}")
}

pub(crate) fn root_graph_node(state: &State) -> GraphNode {
    GraphNode {
        id: root_graph_id(&state.topic_id),
        kind: "root".to_string(),
        title: state.topic.clone(),
        detail: Some(format!(
            "status={}, level={}",
            state.status, state.search_level
        )),
        created_at: Some(state.created_at.clone()),
        updated_at: Some(state.updated_at.clone()),
    }
}

pub(crate) fn source_graph_node(source: &SourceRecord) -> GraphNode {
    GraphNode {
        id: source.id.clone(),
        kind: "source".to_string(),
        title: source.title.clone(),
        detail: Some(source.url.clone()),
        created_at: Some(source.found_at.clone()),
        updated_at: source.fetched_at.clone(),
    }
}

pub(crate) fn claim_graph_node(claim: &ClaimRecord) -> GraphNode {
    GraphNode {
        id: claim.id.clone(),
        kind: "claim".to_string(),
        title: claim.text.clone(),
        detail: Some(format!("confidence={}, status={}", claim.confidence, claim.status)),
        created_at: Some(claim.created_at.clone()),
        updated_at: claim.updated_at.clone(),
    }
}

pub(crate) fn lead_graph_node(lead: &LeadRecord) -> GraphNode {
    GraphNode {
        id: lead.id.clone(),
        kind: "lead".to_string(),
        title: lead.direction.clone(),
        detail: Some(format!("priority={}, status={}", lead.priority, lead.status)),
        created_at: Some(lead.created_at.clone()),
        updated_at: lead.updated_at.clone(),
    }
}

pub(crate) fn graph_edge(
    from: &str,
    to: &str,
    kind: &str,
    rationale: Option<String>,
    created_at: impl Into<String>,
) -> GraphEdge {
    GraphEdge {
        id: format!("edge:{}", link_key(from, to, kind)),
        kind: kind.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        rationale,
        created_at: created_at.into(),
    }
}

pub(crate) fn upsert_graph_node(nodes: &mut Vec<GraphNode>, node: GraphNode) {
    if let Some(found) = nodes.iter_mut().find(|item| item.id == node.id) {
        *found = node;
    } else {
        nodes.push(node);
    }
}

pub(crate) fn upsert_graph_edge(edges: &mut Vec<GraphEdge>, edge: GraphEdge) {
    if let Some(found) = edges.iter_mut().find(|item| item.id == edge.id) {
        *found = edge;
    } else {
        edges.push(edge);
    }
}

pub(crate) fn push_graph_edge(
    edges: &mut Vec<GraphEdge>,
    seen: &mut std::collections::BTreeSet<String>,
    from: String,
    to: String,
    kind: String,
    rationale: Option<String>,
    created_at: String,
) {
    if seen.insert(link_key(&from, &to, &kind)) {
        edges.push(graph_edge(&from, &to, &kind, rationale, created_at));
    }
}
