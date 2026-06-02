use std::collections::BTreeMap;

use crate::{CandidateSourceRecord, ClaimRecord, SourceChunkRecord, SourceRecord};

pub(crate) fn render_numbered_findings(claims: &[ClaimRecord]) -> String {
    claims
        .iter()
        .enumerate()
        .map(|(index, claim)| format!("{}. {}", index + 1, claim.text))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn render_evidence_base(sources: &[SourceRecord]) -> String {
    sources
        .iter()
        .map(|source| {
            format!(
                "- {} ({}) — {}",
                source.title,
                source.url,
                source.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn render_chunk_excerpts(chunks: &[SourceChunkRecord]) -> String {
    chunks
        .iter()
        .take(12)
        .map(|chunk| format!("- [{}] {}", chunk.source_id, chunk.text))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn render_search_quality(candidates: &[CandidateSourceRecord]) -> String {
    if candidates.is_empty() {
        return "No candidate sources were evaluated.".to_string();
    }
    let accepted = candidates.iter().filter(|item| item.decision == "accepted").count();
    let rejected = candidates.iter().filter(|item| item.decision == "rejected").count();
    let average = candidates.iter().map(|item| item.score as f32).sum::<f32>() / candidates.len() as f32;
    format!(
        "Accepted: {accepted}\nRejected: {rejected}\nAverage gate score: {:.1}",
        average
    )
}

pub(crate) fn render_perspectives(
    claims: &[ClaimRecord],
    links: &[crate::LinkRecord],
    sources: &[SourceRecord],
) -> String {
    let mut tag_counts = BTreeMap::new();
    for claim in claims {
        for tag in &claim.tags {
            *tag_counts.entry(tag.clone()).or_insert(0usize) += 1;
        }
    }
    let perspectives = tag_counts
        .into_iter()
        .map(|(tag, count)| format!("- {tag}: {count} claim(s)"))
        .collect::<Vec<_>>();
    [
        format!("Claims: {}", claims.len()),
        format!("Links: {}", links.len()),
        format!("Sources: {}", sources.len()),
        if perspectives.is_empty() {
            "- No claim tags available for perspective breakdown.".to_string()
        } else {
            perspectives.join("\n")
        },
    ]
    .join("\n")
}

pub(crate) fn render_limitations(
    sources: &[SourceRecord],
    claims: &[ClaimRecord],
    open_leads: usize,
) -> String {
    let mut notes = Vec::new();
    if sources.is_empty() {
        notes.push("- No accepted sources were collected.".to_string());
    }
    if claims.is_empty() {
        notes.push("- No claims were extracted.".to_string());
    }
    if open_leads > 0 {
        notes.push(format!("- {open_leads} open lead(s) remain unresolved."));
    }
    if notes.is_empty() {
        notes.push("- No material limitations detected by the deterministic reviewer.".to_string());
    }
    notes.join("\n")
}
