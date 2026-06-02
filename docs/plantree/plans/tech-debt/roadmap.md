# Tech Debt Roadmap

## Scope

Address 5 technical debt items identified in code review (7.5/10 score).

## Tasks (ordered by risk, low → high)

### 1. Config boilerplate [Done]
- **Problem**: `set()`/`normalize()`/`unset()` each duplicate ~50 key match arms
- **Fix**: `config_key_set!`/`config_key_unset!` macros for set/unset; `sync_field!` macro for normalize
- **Risk**: Low — mechanical refactor, tests cover behavior
- **Files**: `src/config.rs`
- **Result**: 897 → 638 lines (-29%), 13 tests pass

### 2. Hardcoded timeouts [Done]
- **Problem**: 500ms viewer, 45s MCP, 120s AI synthesis are magic numbers
- **Fix**: Added `TimeoutConfig` struct with 3 fields; threaded through all HTTP call sites
- **Risk**: Low — additive change
- **Files**: `src/config.rs`, `src/lib.rs`, `src/lab.rs`, `src/viewer.rs`, `src/providers/{code,zhipu,hybrid}.rs`
- **Result**: 0 hardcoded timeouts remain, 13 tests pass

### 3. Provider logic in lib.rs [Done]
- **Problem**: `search_exa`/`search_zhipu`/`search_minimax` live as private fns in lib.rs, providers are thin facades
- **Fix**: Moved implementations into `providers/{exa,zhipu,minimax}.rs`; made `dedupe_hits`/`short_excerpt`/`short`/`interleave_hits` `pub(crate)`
- **Risk**: Medium — visibility changes, import rewiring
- **Files**: `src/lib.rs`, `src/providers/{exa,zhipu,minimax,code}.rs`
- **Result**: lib.rs 3310 → 2986 lines (-10%), 13 tests pass, 0 warnings

### 4. Lab god module [Done]
- **Problem**: 59 pub methods, 2514 lines, 1 impl block
- **Fix**: Split into `lab/mod.rs` + 4 sub-modules: `graph.rs`, `storage_ops.rs`, `search_ops.rs`, `render.rs`
- **Risk**: High — large structural change
- **Files**: `src/lab/mod.rs`, `src/lab/graph.rs`, `src/lab/storage_ops.rs`, `src/lab/search_ops.rs`, `src/lab/render.rs`
- **Result**: mod.rs 2521 → 1629 lines (-35%), 4 sub-modules total 892 lines, 13 tests pass

### 5. Viewer test coverage [Done]
- **Problem**: `#[cfg(test)]` disables viewer entirely — 0% test coverage
- **Fix**: Made `handle_http` `pub(crate)`; added 7 tests for viewer_html, viewer_state, HTTP routing
- **Risk**: Medium — test infrastructure needed
- **Files**: `src/lib.rs`, `src/lab/mod.rs`
- **Result**: 13 → 20 tests pass, viewer HTML/state/routing all covered

## Product Improvements

### BM25 Reranker [Done]
- **Problem**: `token_overlap` with 12 stopwords = toy-level relevance
- **Fix**: `Bm25` struct (k1=1.5, b=0.75), `Bm25Corpus`, `tokenize()` in `utils.rs`
- **Result**: 5 BM25 tests, 25 total

### Concurrent Provider Search [Done]
- **Problem**: Hybrid search ran providers sequentially
- **Fix**: `std::thread::scope` in `providers/hybrid.rs` for parallel execution
- **Result**: 25 tests pass

### AI Query Decomposition [Done]
- **Problem**: Template-based queries miss nuances
- **Fix**: `ai_decompose_queries()` in `lab/mod.rs` — AI generates diverse queries, fallback to deterministic
- **Result**: 25 tests pass

### Incremental Research [Done]
- **Problem**: No query dedup, resume blocked after max_rounds
- **Fix**: `past_queries()`, `deduplicate_queries()`, `Search` CLI command
- **Result**: 25 tests pass

### Semantic Reranking [Done]
- **Problem**: BM25 is lexical-only — can't match synonyms
- **Fix**: 60% BM25 + 40% semantic (embedding cosine similarity) blend in `rerank_hits`
- **Config**: `EmbeddingConfig` (api_url, api_key, model) in `ResearchConfig`
- **Parser**: `collect_minimax_hits` rewritten for new MCP format
- **Path fix**: Config moved from `~/.config/opencode/research/` to `~/.config/research/`
- **Result**: 33 tests pass, E2E verified with hybrid provider

### Zhipu MCP Fix [Done]
- **Problem**: Zhipu MCP returns double-escaped JSON (text field contains JSON string → JSON array); `search_query` param name wrong
- **Fix**: `collect_zhipu_hits` parses twice (first `from_str::<String>`, then `from_str::<Vec<Value>>`); `zhipu_tool_arguments` uses `search_query` instead of `query`
- **Result**: Zhipu returns 10 results (6 accepted), all 3 providers working, 33 tests pass

### Cross-Encoder Reranker [Done]
- **Problem**: Bi-encoder embeddings are fast but less accurate than cross-encoder reranking
- **Fix**: 
  - `RerankerConfig` struct in `config.rs` (api_url, api_key, model)
  - `rerank()` function in `utils.rs` — calls Cohere-compatible `/v1/rerank` endpoint
  - Updated `rerank_hits()` in `lib.rs`: 30% BM25 + 70% reranker (cross-encoder) when available
  - Fallback chain: cross-encoder → bi-encoder → BM25 only
- **Config**: SiliconFlow `Qwen/Qwen3-Reranker-8B`
- **Result**: 33 tests pass, reranker reasons show in output

### Provider-Based Agent Config [Done]
- **Problem**: All AI roles shared one API endpoint/key; can't assign different models per role
- **Fix**: 
  - `AiProviderEntry` struct (api_url, api_key, models HashMap) in `config.rs`
  - `AgentRoleConfig` struct (provider, model) in `config.rs`
  - `resolve_agent()` method: provider+model alias → (api_url, api_key, model_name)
  - `lab/mod.rs`: writer uses `resolve_agent("writer")`, planner uses `resolve_agent("planner")`
- **Config**: `ai_providers.<name>` + `agents.<role>` with fallback to legacy `ai` config
- **Result**: 33 tests pass, planner uses "fast" model, writer uses "smart" model

### Context Engineering & Harness Engineering [Done]
- **Problem**: 8 optimization items across context engineering (planner thin context, reader too wide, no query feedback, no writer compression) and harness engineering (no quality gate, simple evidence gate, fixed pipeline, blind provider routing)
- **Fix**:
  - Query feedback signals: `accepted_source_count` + `was_productive` in `SearchAttemptRecord`
  - Reader trimming: relevance-based top-K paragraph selection in `chunk_source()`
  - Evidence gate: differentiated authority scores + archived penalty + freshness bonus
  - Provider smart routing: `QueryProfile` + `analyze_query()` in `hybrid.rs`
  - Planner context: `build_state_summary()` + state summary in AI prompt
  - Quality gate: retry with fresh queries when 0 sources accepted
  - Adaptive pipeline: reduce query count in later rounds
  - Writer compression: `build_research_summary()` injected into report prompt
- **Result**: 33 tests pass, 0 warnings

### Fault Tolerance & PDF & Search Quality [Done]
- **Problem**: No retry on API failures, can't parse PDF search results, cross-provider duplicates waste source slots, no domain diversity bonus
- **Fix**:
  - Retry with exponential backoff: `retry()` + `retry_http()` in `utils.rs`; wrapped embedding, reranker, exa, zhipu HTTP calls
  - PDF text extraction: `lopdf` crate, `is_pdf_url()` + `fetch_pdf_text()` + `extract_pdf_text_from_bytes()` in `utils.rs`; integrated into `chunk_source()` in `lib.rs`
  - Cross-provider dedup: `normalize_url_for_dedup()` strips tracking params; `rerank_hits()` deduplicates by normalized URL before scoring
  - Evidence gate novelty: `extract_root_domain()` + +8 bonus for novel domains; `accepted_domains` param in `evidence_gate()`
- **Result**: 40 tests pass (7 new: 4 retry + 2 PDF + 1 evidence gate novelty), 0 warnings

### Kimi Search Provider [Done]
- **Problem**: Need to expand search sources beyond Exa, Zhipu, Minimax
- **Fix**: 
  - `src/providers/kimi.rs` (94 lines): New provider using Moonshot AI's builtin `$web_search` function
  - API: `https://api.moonshot.cn/v1/chat/completions` (OpenAI-compatible)
  - Model: `kimi-k2.6` (recommended for web search)
  - Flow: send chat completion with `$web_search` tool → Kimi returns tool_calls → pass arguments back → Kimi executes search internally → returns final answer
  - Config: `KimiConfig` struct (api_key, model) in `config.rs`
  - Hybrid: added to `HybridInputs` with conditional spawn (only when api_key configured)
- **Result**: 40 tests pass, Kimi available as `--search-provider kimi` or in hybrid mode

### Error Audit Fixes [Done]
- **Problem**: Error audit found 13 issues across 6 dimensions (message quality, propagation, visibility, security, recovery, test coverage)
- **P0 fixes**:
  - UTF-8 slice panic: `&text[..text.len().min(200)]` → `text.chars().take(200).collect::<String>()` in utils.rs + kimi.rs
  - HTTP error false positives: `is_retryable_http_error` now uses word-boundary matching instead of substring
  - JSONL silent drop: `read_jsonl` now logs malformed lines with line number + first 100 chars
- **P1 fixes**:
  - Provider error context: exa.rs, zhipu.rs, minimax.rs bare `?` → `.context()` with operation description
  - Viewer: bail adds `pkill` hint for zombie processes; eprintln flood throttled to first-error-only
  - Minimax stdout truncation: error output capped at 500 chars
  - Lib.rs: embedding failure now logs warning instead of silent `None`
  - Kimi: "empty response" message explains possible causes + shows query
  - Config: "unknown key" errors now list available top-level keys
  - Hybrid: thread panic captures payload via `downcast_ref`
- **Tests**: 4 new tests for `is_retryable_http_error` (429/5xx detection, URL false positive rejection, non-retryable exclusion)
- **Result**: 44 tests pass (40→44), 0 warnings
