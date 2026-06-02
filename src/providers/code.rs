use anyhow::Result;

use crate::models::SearchHit;
use crate::providers::exa;

pub(crate) fn search(query: &str, tokens_num: u32, api_key: Option<&str>, timeout_secs: u64) -> Result<Vec<SearchHit>> {
    exa::search_code_context(query, tokens_num, api_key, timeout_secs)
}
