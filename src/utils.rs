use std::collections::{BTreeSet, HashMap};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use lopdf::Document;
use rand::Rng;
use serde::Deserialize;
use serde_json::json;
use ulid::Ulid;

use crate::{EmbeddingConfig, RerankerConfig, Priority};

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn id(prefix: &str) -> String {
    format!("{}_{}", prefix, Ulid::new().to_string().to_lowercase())
}

pub(crate) fn slug(value: &str) -> String {
    let cleaned = value
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(80)
        .collect::<String>();
    if cleaned.is_empty() {
        format!("topic-{}", Ulid::new().to_string().to_lowercase())
    } else {
        cleaned
    }
}

pub(crate) fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn clean_text(value: &str) -> String {
    value
        .trim_matches(|c: char| c.is_whitespace() || ",，、。；;:：)]）】'\"“”‘’-—".contains(c))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn same_research_idea(left: &str, right: &str) -> bool {
    let left = normalize(left);
    let right = normalize(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left == right
        || (left.len() > 16 && right.len() > 16 && (left.contains(&right) || right.contains(&left)))
        || token_overlap(&left, &right) >= 0.64
}

pub(crate) fn token_overlap(left: &str, right: &str) -> f32 {
    let left = left
        .split_whitespace()
        .filter(|item| !is_stopword(item))
        .filter(|item| item.len() >= 3)
        .collect::<BTreeSet<_>>();
    let right = right
        .split_whitespace()
        .filter(|item| !is_stopword(item))
        .filter(|item| item.len() >= 3)
        .collect::<BTreeSet<_>>();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    left.intersection(&right).count() as f32 / left.len().min(right.len()) as f32
}

pub(crate) fn is_stopword(value: &str) -> bool {
    matches!(
        value,
        "find"
            | "search"
            | "specific"
            | "data"
            | "figures"
            | "figure"
            | "rate"
            | "rates"
            | "percentage"
            | "percent"
            | "market"
            | "share"
            | "adoption"
            | "research"
            | "evidence"
            | "source"
            | "sources"
            | "more"
            | "additional"
    )
}

pub(crate) fn text_overlaps(left: &str, right: &str) -> bool {
    token_overlap(&normalize(left), &normalize(right)) >= 0.25
}

pub(crate) fn unique_strings(items: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(normalize(item)))
        .collect()
}

pub(crate) fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::Low => 1,
        Priority::Normal => 2,
        Priority::High => 3,
    }
}

pub(crate) fn merge(left: Vec<String>, right: Vec<String>) -> Vec<String> {
    left.into_iter()
        .chain(right)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Okapi BM25 scorer for ranking search results.
///
/// Standard parameters: k1=1.5, b=0.75.
/// IDF is computed over the provided corpus (search result batch).
pub(crate) struct Bm25 {
    pub k1: f32,
    pub b: f32,
}

impl Default for Bm25 {
    fn default() -> Self {
        Self {
            k1: 1.5,
            b: 0.75,
        }
    }
}

/// Tokenize text into lowercase alphanumeric terms, filtering stopwords and short tokens.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    normalize(text)
        .split_whitespace()
        .filter(|t| !is_stopword(t) && t.len() >= 3)
        .map(|t| t.to_string())
        .collect()
}

/// Count term frequencies in a token list.
fn term_frequencies(tokens: &[String]) -> HashMap<String, u32> {
    let mut tf = HashMap::new();
    for token in tokens {
        *tf.entry(token.clone()).or_insert(0) += 1;
    }
    tf
}

/// BM25 corpus statistics: document count and document frequency per term.
pub(crate) struct Bm25Corpus {
    /// Number of documents in the corpus.
    pub n: f32,
    /// Average document length (in tokens).
    pub avgdl: f32,
    /// Document frequency: how many documents contain each term.
    pub df: HashMap<String, u32>,
}

impl Bm25Corpus {
    /// Build corpus statistics from a list of pre-tokenized documents.
    pub fn from_documents(doc_tokens: &[Vec<String>]) -> Self {
        let n = doc_tokens.len() as f32;
        let total_len: usize = doc_tokens.iter().map(|t| t.len()).sum();
        let avgdl = if n > 0.0 { total_len as f32 / n } else { 0.0 };
        let mut df: HashMap<String, u32> = HashMap::new();
        for tokens in doc_tokens {
            let unique: BTreeSet<&String> = tokens.iter().collect();
            for term in unique {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }
        Self { n, avgdl, df }
    }

    /// Compute BM25 score for a query against a document.
    ///
    /// `query_tokens` and `doc_tokens` should be pre-tokenized.
    pub fn score(&self, scorer: &Bm25, query_tokens: &[String], doc_tokens: &[String]) -> f32 {
        if self.n == 0.0 || self.avgdl == 0.0 || query_tokens.is_empty() || doc_tokens.is_empty() {
            return 0.0;
        }
        let doc_tf = term_frequencies(doc_tokens);
        let dl = doc_tokens.len() as f32;
        let mut score = 0.0_f32;
        let query_set: BTreeSet<&String> = query_tokens.iter().collect();
        for term in query_set {
            let f = *doc_tf.get(term).unwrap_or(&0) as f32;
            let n_t = *self.df.get(term).unwrap_or(&0) as f32;
            // IDF with floor at 0 (Robertson-Sparck Jones variant)
            let idf = ((self.n - n_t + 0.5) / (n_t + 0.5) + 1.0).ln().max(0.0);
            let tf_norm = (f * (scorer.k1 + 1.0))
                / (f + scorer.k1 * (1.0 - scorer.b + scorer.b * dl / self.avgdl));
            score += idf * tf_norm;
        }
        score
    }
}

/// Compute cosine similarity between two vectors.
pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Call OpenAI-compatible embedding API. Returns one vector per input text.
pub(crate) fn embed_texts(config: &EmbeddingConfig, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let api_url = config
        .api_url
        .as_deref()
        .context("embedding.api_url not configured")?;
    let api_key = config
        .api_key
        .as_deref()
        .context("embedding.api_key not configured")?;
    let model = config
        .model
        .as_deref()
        .unwrap_or("text-embedding-3-small");

    let client = reqwest::blocking::Client::new();
    let resp = retry(2, || {
        let resp = client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": model,
                "input": texts,
            }))
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .context("embedding API request failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            bail!("embedding API error {status}: {body}");
        }
        Ok(resp)
    })?;

    let body: EmbeddingResponse = resp.json().context("failed to parse embedding response")?;
    if body.data.len() != texts.len() {
        bail!(
            "embedding API returned {} vectors for {} texts",
            body.data.len(),
            texts.len()
        );
    }
    // Sort by index to guarantee ordering
    let mut sorted = body.data;
    sorted.sort_by_key(|d| d.index);
    Ok(sorted.into_iter().map(|d| d.embedding).collect())
}

/// Embed a single text. Convenience wrapper around `embed_texts`.
pub(crate) fn embed_single(config: &EmbeddingConfig, text: &str) -> Result<Vec<f32>> {
    let mut result = embed_texts(config, &[text.to_string()])?;
    Ok(result.pop().unwrap_or_default())
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

// ── Reranker ─────────────────────────────────────────────────────────────────

/// Call a cross-encoder reranker API (Cohere-compatible `/v1/rerank` endpoint).
/// Returns relevance scores in the same order as `documents`, or an error.
pub(crate) fn rerank(
    config: &RerankerConfig,
    query: &str,
    documents: &[String],
) -> Result<Vec<f32>> {
    let api_url = config
        .api_url
        .as_deref()
        .context("reranker.api_url not configured")?;
    let api_key = config
        .api_key
        .as_deref()
        .context("reranker.api_key not configured")?;
    let model = config
        .model
        .as_deref()
        .unwrap_or("BAAI/bge-reranker-v2-m3");

    let body = json!({
        "model": model,
        "query": query,
        "documents": documents,
    });

    let client = reqwest::blocking::Client::new();
    let resp = retry(2, || {
        let resp = client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .context("reranker API request failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            bail!("reranker API error {}: {}", status, text.chars().take(200).collect::<String>());
        }
        Ok(resp)
    })?;

    let parsed: RerankerResponse = resp
        .json()
        .context("failed to parse reranker response")?;

    // Results are sorted by index; convert to score vector in original document order
    let mut scores = vec![0.0f32; documents.len()];
    for r in &parsed.results {
        if r.index < scores.len() {
            scores[r.index] = r.relevance_score as f32;
        }
    }
    Ok(scores)
}

#[derive(Deserialize)]
struct RerankerResponse {
    results: Vec<RerankerResult>,
}

#[derive(Deserialize)]
struct RerankerResult {
    index: usize,
    relevance_score: f64,
}

// ── PDF text extraction ─────────────────────────────────────────────────────

/// Check if a URL likely points to a PDF document.
pub(crate) fn is_pdf_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.ends_with(".pdf") || lower.contains(".pdf?") || lower.contains(".pdf#")
}

/// Fetch a PDF from a URL and extract its text content.
/// Returns the extracted text, or an error if the fetch or parse fails.
pub(crate) fn fetch_pdf_text(url: &str, timeout_secs: u64) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let resp = client
        .get(url)
        .header("User-Agent", "research-cli/0.1.0")
        .send()
        .context("failed to fetch PDF")?;
    if !resp.status().is_success() {
        bail!("PDF fetch failed with {}: {}", resp.status(), url);
    }
    let bytes = resp.bytes().context("failed to read PDF response body")?;
    extract_pdf_text_from_bytes(&bytes)
}

/// Extract text from PDF bytes using lopdf.
pub(crate) fn extract_pdf_text_from_bytes(bytes: &[u8]) -> Result<String> {
    let doc = Document::load_mem(bytes).context("failed to parse PDF")?;
    let mut text = String::new();
    let pages = doc.get_pages();
    for (&page_num, _page_id) in pages.iter() {
        if let Ok(page_text) = doc.extract_text(&[page_num]) {
            text.push_str(&page_text);
            text.push('\n');
        }
    }
    if text.trim().is_empty() {
        bail!("PDF contained no extractable text");
    }
    Ok(text)
}

#[cfg(test)]
mod bm25_tests {
    use super::*;

    #[test]
    fn bm25_scores_relevant_document_higher() {
        let scorer = Bm25::default();
        let docs = vec![
            tokenize("the quick brown fox jumps over the lazy dog"),
            tokenize("rust programming language memory safety"),
            tokenize("search engine optimization and web ranking"),
        ];
        let corpus = Bm25Corpus::from_documents(&docs);
        let query = tokenize("rust memory safety");

        let score_relevant = corpus.score(&scorer, &query, &docs[1]);
        let score_irrelevant = corpus.score(&scorer, &query, &docs[0]);
        assert!(
            score_relevant > score_irrelevant,
            "relevant={} should be > irrelevant={}",
            score_relevant,
            score_irrelevant
        );
    }

    #[test]
    fn bm25_exact_match_scores_highest() {
        let scorer = Bm25::default();
        let docs = vec![
            tokenize("BM25 is a ranking function used by search engines"),
            tokenize("Okapi BM25 improves upon TF-IDF with term saturation"),
            tokenize("machine learning and neural networks"),
        ];
        let corpus = Bm25Corpus::from_documents(&docs);
        let query = tokenize("BM25 ranking search");

        let scores: Vec<f32> = docs
            .iter()
            .map(|d| corpus.score(&scorer, &query, d))
            .collect();
        // First two documents are both relevant; third is not
        assert!(scores[0] > scores[2]);
        assert!(scores[1] > scores[2]);
    }

    #[test]
    fn bm25_handles_empty_corpus() {
        let scorer = Bm25::default();
        let docs: Vec<Vec<String>> = vec![];
        let corpus = Bm25Corpus::from_documents(&docs);
        let query = tokenize("test");
        // Should not panic
        let score = corpus.score(&scorer, &query, &[]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn bm25_handles_empty_query() {
        let scorer = Bm25::default();
        let docs = vec![tokenize("some text")];
        let corpus = Bm25Corpus::from_documents(&docs);
        let query: Vec<String> = vec![];
        let score = corpus.score(&scorer, &query, &docs[0]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn bm25_longer_document_not_punished_unfairly() {
        let scorer = Bm25::default();
        // Short doc with exact match
        let short = tokenize("rust safety");
        // Long doc with match buried in noise
        let mut long_tokens = tokenize("the quick brown fox jumps over the lazy dog in the park");
        long_tokens.extend(tokenize("rust safety guarantees prevent memory bugs"));
        let docs = vec![short.clone(), long_tokens.clone()];
        let corpus = Bm25Corpus::from_documents(&docs);
        let query = tokenize("rust safety");

        let score_short = corpus.score(&scorer, &query, &short);
        let score_long = corpus.score(&scorer, &query, &long_tokens);
        // Both should have positive scores; short may be higher due to length normalization
        assert!(score_short > 0.0);
        assert!(score_long > 0.0);
    }

    #[test]
    fn cosine_similarity_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_opposite_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_empty_vectors() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn cosine_similarity_similar_vectors_score_high() {
        let a = vec![1.0, 1.0, 1.0];
        let b = vec![1.0, 1.0, 0.9];
        let sim = cosine_similarity(&a, &b);
        assert!(sim > 0.95, "expected >0.95, got {}", sim);
    }

    #[test]
    fn embedding_api_siliconflow() {
        let api_key = match std::env::var("SILICONFLOW_API_KEY") {
            Ok(k) => k,
            Err(_) => {
                eprintln!("SILICONFLOW_API_KEY not set, skipping");
                return;
            }
        };
        let cfg = EmbeddingConfig {
            api_url: Some("https://api.siliconflow.cn/v1/embeddings".into()),
            api_key: Some(api_key),
            model: Some("Qwen/Qwen3-VL-Embedding-8B".into()),
        };
        let texts = vec![
            "Rust async runtime comparison Tokio vs async-std".to_string(),
            "Tokio performance benchmarks for async Rust".to_string(),
            "Italian pasta recipe with tomato sauce".to_string(),
        ];
        let vectors = embed_texts(&cfg, &texts).unwrap();
        assert_eq!(vectors.len(), 3);
        assert!(vectors[0].len() > 100, "embedding dim should be large, got {}", vectors[0].len());

        let sim_related = cosine_similarity(&vectors[0], &vectors[1]);
        let sim_unrelated = cosine_similarity(&vectors[0], &vectors[2]);
        eprintln!("dim={}, related={:.4}, unrelated={:.4}", vectors[0].len(), sim_related, sim_unrelated);
        assert!(
            sim_related > sim_unrelated,
            "semantic: related ({:.4}) should > unrelated ({:.4})",
            sim_related,
            sim_unrelated
        );
    }
}

#[cfg(test)]
mod pdf_tests {
    use super::*;

    #[test]
    fn is_pdf_url_detects_pdf() {
        assert!(is_pdf_url("https://example.com/paper.pdf"));
        assert!(is_pdf_url("https://example.com/paper.pdf?download=1"));
        assert!(is_pdf_url("https://example.com/paper.pdf#page=5"));
        assert!(!is_pdf_url("https://example.com/page.html"));
        assert!(!is_pdf_url("https://example.com/search?q=pdf"));
    }

    #[test]
    fn extract_pdf_text_from_bytes_rejects_garbage() {
        let result = extract_pdf_text_from_bytes(b"this is not a pdf");
        assert!(result.is_err());
    }
}

/// Uses `same_research_idea` for comparison (token overlap ≥ 0.64 or substring match).
pub(crate) fn deduplicate_queries(queries: Vec<String>, past: &[String]) -> Vec<String> {
    queries
        .into_iter()
        .filter(|query| {
            !past
                .iter()
                .any(|past_query| same_research_idea(query, past_query))
        })
        .collect()
}

// ── Retry utilities ──────────────────────────────────────────────────────────

/// Retry a fallible operation with exponential backoff.
///
/// `max_retries` is the number of retry attempts (so total attempts = max_retries + 1).
/// Base delay is 1 second, doubled each retry, capped at 30 seconds.
/// Retries on any error; returns the first successful result or the last error.
pub(crate) fn retry<T, F>(max_retries: u32, mut op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut last_err = None;
    for attempt in 0..=max_retries {
        match op() {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt < max_retries {
                    let delay_ms = compute_delay_ms(attempt);
                    eprintln!(
                        "retrying (attempt {}/{}), waiting {}ms: {}",
                        attempt + 1,
                        max_retries,
                        delay_ms,
                        e
                    );
                    thread::sleep(Duration::from_millis(delay_ms));
                }
                last_err = Some(e);
            }
        }
    }
    // SAFETY: loop runs at least once (attempt 0), so last_err is always Some.
    Err(last_err.unwrap())
}

/// Retry an HTTP request, only retrying on 429 (rate limit) or 5xx server errors.
/// Other errors are returned immediately.
pub(crate) fn retry_http<T, F>(max_retries: u32, mut op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut last_err = None;
    for attempt in 0..=max_retries {
        match op() {
            Ok(val) => return Ok(val),
            Err(e) => {
                let msg = e.to_string();
                if !is_retryable_http_error(&msg) {
                    // Non-retryable error: return immediately
                    return Err(e);
                }
                if attempt < max_retries {
                    let delay_ms = compute_delay_ms(attempt);
                    eprintln!(
                        "retrying (attempt {}/{}), waiting {}ms: {}",
                        attempt + 1,
                        max_retries,
                        delay_ms,
                        e
                    );
                    thread::sleep(Duration::from_millis(delay_ms));
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap())
}

/// Compute exponential backoff delay with ±25% jitter, capped at 30 seconds.
fn compute_delay_ms(attempt: u32) -> u64 {
    let base_ms: u64 = 1000;
    let exponential = base_ms.saturating_mul(1u64 << attempt.min(30));
    let capped = exponential.min(30_000);
    // Add ±25% jitter
    let jitter_range = capped / 4; // 25%
    if jitter_range == 0 {
        return capped;
    }
    let mut rng = rand::rng();
    let jitter: i64 = rng.random_range(-(jitter_range as i64)..=(jitter_range as i64));
    ((capped as i64 + jitter).max(0) as u64).max(1)
}

/// Check if an error message indicates a retryable HTTP error (429 or 5xx).
///
/// Uses word-boundary matching to avoid false positives from status codes
/// appearing in URLs (e.g. "/api/v5/500"), ports (":5000"), or other contexts.
fn is_retryable_http_error(msg: &str) -> bool {
    // Split message into whitespace-separated tokens and check for exact matches.
    // This avoids false positives from substring matching on URLs, ports, etc.
    // Also strip common trailing punctuation (":", ",", ".") from tokens.
    let words: Vec<&str> = msg.split_whitespace().collect();
    for word in &words {
        // Strip trailing punctuation like "500:" or "500," or "500)."
        let trimmed = word.trim_end_matches(|c: char| !c.is_ascii_digit());
        if trimmed.is_empty() {
            continue;
        }
        // Check 429 (rate limit)
        if trimmed == "429" {
            return true;
        }
        // Check 5xx server errors
        if trimmed.len() == 3 && trimmed.starts_with('5') {
            if trimmed[1..].chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn retry_succeeds_on_first_try() {
        let result = retry(3, || Ok(42));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn retry_succeeds_after_failures() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();
        let result = retry(5, move || {
            let count = attempts_clone.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                bail!("failure {}", count);
            }
            Ok("success")
        });
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts.load(Ordering::SeqCst), 3); // failed twice, succeeded third
    }

    #[test]
    fn retry_exhausts_all_attempts() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();
        let result: Result<()> = retry(2, move || {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            bail!("always fails");
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "always fails");
        assert_eq!(attempts.load(Ordering::SeqCst), 3); // 1 initial + 2 retries
    }

    #[test]
    fn retry_zero_retries() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();
        let result: Result<()> = retry(0, move || {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            bail!("single failure");
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "single failure");
        assert_eq!(attempts.load(Ordering::SeqCst), 1); // exactly one attempt
    }

    #[test]
    fn is_retryable_detects_429() {
        assert!(is_retryable_http_error("HTTP status: 429 Too Many Requests"));
        assert!(is_retryable_http_error("got 429"));
        assert!(is_retryable_http_error("error: 429."));
    }

    #[test]
    fn is_retryable_detects_5xx() {
        assert!(is_retryable_http_error("server error 500"));
        assert!(is_retryable_http_error("bad gateway: 502"));
        assert!(is_retryable_http_error("service unavailable 503,"));
        assert!(is_retryable_http_error("gateway timeout 504"));
        assert!(is_retryable_http_error("HTTP 599"));
    }

    #[test]
    fn is_retryable_ignores_codes_in_urls() {
        assert!(!is_retryable_http_error("request to https://api.example.com/v5/500 failed"));
        assert!(!is_retryable_http_error("error at /api/v502/endpoint"));
        assert!(!is_retryable_http_error("connected to localhost:5000"));
    }

    #[test]
    fn is_retryable_ignores_non_retryable() {
        assert!(!is_retryable_http_error("400 Bad Request"));
        assert!(!is_retryable_http_error("401 Unauthorized"));
        assert!(!is_retryable_http_error("404 Not Found"));
        assert!(!is_retryable_http_error("connection refused"));
    }
}
