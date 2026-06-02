use crate::{SearchHit, SearchProvider, State};
use crate::utils::slug;

pub(crate) fn search(state: &State, query: &str) -> Vec<SearchHit> {
    vec![SearchHit {
        provider: SearchProvider::Deterministic.to_string(),
        url: format!("research://deterministic/{}", slug(query)),
        title: format!("Research seed: {query}"),
        summary: format!("Seed context for '{}' under topic '{}'.", query, state.topic),
        text: format!("{} requires investigation of {}.", state.topic, query),
        published_date: None,
        author: None,
    }]
}
