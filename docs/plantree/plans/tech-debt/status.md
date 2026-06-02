# Tech Debt Status

## Current: All phases complete — error audit fixes done

### Error Audit Fixes
- **Trigger**: Full error audit across 18 source files, 6 dimensions
- **P0 fixes**: UTF-8 slice panic (utils.rs + kimi.rs), HTTP error false positives (utils.rs), JSONL silent drop (storage.rs)
- **P1 fixes**: Provider error context (exa/zhipu/minimax), viewer zombie hint + eprintln throttle, minimax stdout truncation, embedding failure logging, kimi empty response message, config unknown key hints, hybrid thread panic payload
- **Tests**: 4 new `is_retryable_http_error` tests added (44 total)
- **Files changed**: utils.rs, storage.rs, exa.rs, zhipu.rs, minimax.rs, kimi.rs, hybrid.rs, lib.rs, viewer.rs, config.rs

### Tech Debt Tasks (all complete ✅)

#### Task 1: Config Boilerplate
- Added `sync_field!` macro for bidirectional flat↔nested sync in `normalize()`
- Added `config_key_set!` macro for `set()` match arms
- Added `config_key_unset!` macro for `unset()` match arms
- Result: `config.rs` 897 → 638 lines (-29%), 13/13 tests pass

#### Task 2: Hardcoded Timeouts
- Added `TimeoutConfig` struct to `config.rs` with 3 fields: `mcp_timeout_secs` (45), `ai_timeout_secs` (120), `viewer_timeout_ms` (500)
- Threaded `timeout_secs` through `search_exa_code_context()`, `search_zhipu()`, `viewer_responds()`
- Updated `providers/code.rs`, `providers/zhipu.rs`, `providers/hybrid.rs` to pass timeout
- Updated `lab.rs` to use `self.config.timeouts.*` for AI synthesis and search dispatch
- Updated `viewer.rs` to pass `config.timeouts.viewer_timeout_ms` to `viewer_responds()`
- Result: 0 hardcoded timeouts remain, 13/13 tests pass

#### Task 3: Provider Logic in lib.rs
- Moved `search_exa` + `ExaSearchResponse`/`ExaResult` → `providers/exa.rs`
- Moved `search_exa_code_context` → `providers/exa.rs` as `search_code_context`
- Moved `search_zhipu` + `zhipu_request` → `providers/zhipu.rs`
- Moved `search_minimax` → `providers/minimax.rs`
- Updated `providers/code.rs` to call `exa::search_code_context` directly
- Made `dedupe_hits`, `short_excerpt`, `short`, `interleave_hits` `pub(crate)` in lib.rs
- Cleaned up unused imports in lib.rs and config.rs
- Result: lib.rs 3310 → 2986 lines (-10%), 13/13 tests pass, 0 warnings

#### Task 4: Lab God Module
- Split Lab's 74 methods (2521 lines, 1 impl block) into 5 files:
  - `lab/mod.rs` (1629L): core lifecycle, AI, planner, verifier, writer, reviewer
  - `lab/graph.rs` (245L): `load_graph`, `save_graph`, `build_graph`, `build_graph_nodes`, `build_graph_edges`, `graph_path`, `node_for_source`, `record_claims_in_graph`, `record_source_in_graph`, `update_graph_from_state`
  - `lab/storage_ops.rs` (141L): `ensure_project_dirs`, `load_state`, `save_state`, `list_states`, `read_jsonl`, `write_jsonl`, `append_jsonl`, `event`, `open_leads`, `write_viewer_snapshot`, `write_artifact_manifest`
  - `lab/search_ops.rs` (170L): `search`, `search_hybrid`, `record_search_attempt`, `record_agent_run`, `search_local_projects`
  - `lab/render.rs` (336L): `render_claims`, `render_leads`, `render_evidence_base`, `render_numbered_findings`, `render_open_questions`, `render_status_overview`, `render_resume_summary`, `render_source_log`, `render_section`, `generate_model_report`, `render_deterministic_report`, `render_markdown_report`, `status_value`
- Key issues fixed: stray `}` from deletion, `model_for_role` type mismatch (`Option<String>` not `String`), visibility of search methods
- Result: mod.rs 2521 → 1629 lines (-35%), 13/13 tests pass, 0 warnings

#### Task 5: Viewer Test Coverage
- Made `handle_http` `pub(crate)` in `lab/mod.rs` for testability
- Added 7 new tests covering viewer HTML generation, state completeness, and HTTP routing:
  - `viewer_html_replaces_topic_id_placeholder` — verifies `__TOPIC_ID__` substitution
  - `viewer_html_injects_embedded_state` — verifies JSON state injection into HTML
  - `viewer_html_null_embedded_state_becomes_literal_null` — verifies `None` → `null`
  - `viewer_html_escapes_closing_script_tag_in_embedded_state` — verifies XSS prevention
  - `viewer_state_returns_all_expected_keys_after_start` — verifies all 15 JSON keys present
  - `handle_http_routes_api_state_to_json` — TCP test: `/api/state` → JSON response
  - `handle_http_routes_root_to_html` — TCP test: `/` → HTML response
- Result: 13 → 20 tests pass, 0 warnings

### Product Improvements

#### Phase 1: BM25 Reranker [Done]
- **Problem**: `rerank_hits` used `token_overlap` with 12 stopwords — toy-level relevance
- **Fix**: Added `Bm25` struct (k1=1.5, b=0.75), `Bm25Corpus`, `tokenize()` to `utils.rs`
- IDF: Robertson-Sparck Jones variant; edge case: early return 0.0 for empty corpus
- `rerank_hits` now uses BM25 scoring with bonuses (has text +50, has summary +20), noise penalty -200
- `token_overlap` kept for evidence_gate, local_search, dedup, claim matching
- Result: 5 new BM25 tests, 25 total tests pass

#### Phase 2: Concurrent Provider Search [Done]
- **Problem**: Hybrid search ran providers sequentially — slow when multiple MCP providers
- **Fix**: Rewrote `providers/hybrid.rs` to use `std::thread::scope` for parallel execution
- Results collected after all threads join, then recorded sequentially via callback
- Thread panics caught and converted to errors
- Result: 25 tests pass

#### Phase 3: AI Query Decomposition [Done]
- **Problem**: `plan_queries` was purely deterministic — template-based queries miss nuances
- **Fix**: Added `ai_decompose_queries()` to `lab/mod.rs` — calls AI to generate diverse search queries
- Falls back to deterministic when AI not configured or fails
- Used for initial rounds (no directions) and direction-based rounds; focused rounds stay deterministic
- Result: 25 tests pass

#### Phase 4: Incremental Research [Done]
- **Problem**: No query deduplication (same queries searched across rounds), resume blocked after max_rounds, no way to add explicit search queries
- **Fix**:
  - Added `past_queries()` to `search_ops.rs` — reads `search_attempts.jsonl` history
  - Added `deduplicate_queries()` to `utils.rs` — filters queries using `same_research_idea` (token overlap ≥ 0.64)
  - Updated `plan_queries()` in `lab/mod.rs` — passes past queries to AI decomposition context, filters output through dedup
  - Updated `ai_decompose_queries()` — accepts `past_queries` parameter, tells AI to avoid already-tried queries
  - Updated `resume()` in `lab/mod.rs` — allows continuation when new leads/directions exist, even after max_rounds
  - Added `Search` CLI command — `research search --topic-id X --query "..." --reason "..."` adds explicit query as lead and runs immediately
- Result: 25 tests pass, 0 warnings

#### Phase 6: Zhipu MCP Fix [Done]

**Date**: 2026-06-02
**Status**: Complete

### Changes
- `src/mcp.rs`: `collect_zhipu_hits` now handles double-escaped JSON (text field contains JSON string → parse as String → parse inner as array)
- `src/mcp.rs`: `zhipu_tool_arguments` uses `search_query` instead of `query` (zhipu API requirement)
- `src/providers/zhipu.rs`: removed debug output

### Result
- Zhipu MCP: 10 results returned, 6 accepted after evidence gate
- All 3 providers working: minimax ✅, zhipu ✅, exa ❌ (credits exhausted)
- 33 tests pass, 0 warnings

---

## Phase 5: Semantic Reranking [Done]
- **Problem**: BM25 is lexical-only — can't match synonyms or semantic similarity
- **Fix**:
  - Added `EmbeddingConfig` struct to `config.rs` — `api_url`, `api_key`, `model` (default "text-embedding-3-small")
  - Added `embedding: EmbeddingConfig` field to `ResearchConfig`
  - Added `cosine_similarity()`, `embed_texts()`, `embed_single()` to `utils.rs`
  - Modified `rerank_hits` in `lib.rs`: **60% BM25 + 40% semantic** blend
  - Graceful fallback: if embedding API fails, uses BM25 only
- **Config path fix**: Changed from `~/.config/opencode/research/` to `~/.config/research/`
  - `DEFAULT_CONFIG_RELATIVE_PATH` in lib.rs: `"research/config.json"`
  - `default_project_root()` in config.rs: `"research/projects"`
- **Minimax parser fix**: `collect_minimax_hits` in `mcp.rs` rewritten to handle new MCP response format
  - Parses `result.structuredContent.text` → JSON → `organic` array
  - Handles both `url`/`link` and `snippet`/`text` field names
- **E2E verification**:
  - Hybrid mode test: 6 claims, 2 sources, 5 links accepted
  - Rerank reasons include `"semantic 0.442"`, `"semantic 0.475"` etc.
  - Embedding API: SiliconFlow (`Qwen/Qwen3-VL-Embedding-8B`)
- **API keys configured** (in `~/.config/research/config.json`):
  - exa: `10721bba-...` (credits exhausted — 402)
  - zhipu: `5ec9c856...` (needs `zhipu_mcp_url`)
  - minimax: configured and working
  - embedding: SiliconFlow configured and working
- Result: 33 tests pass, 0 warnings

## Phase 7: Cross-Encoder Reranker [Done]

**Date**: 2026-06-02
**Status**: Complete

### Problem
BM25 is lexical-only, embedding is bi-encoder (separate query/doc encoding). A cross-encoder reranker that processes (query, document) pairs together provides much higher relevance scoring.

### Changes
- `src/config.rs`: Added `RerankerConfig` struct — `api_url`, `api_key`, `model`
- `src/config.rs`: Added `reranker: RerankerConfig` field to `ResearchConfig`
- `src/config.rs`: Added set/unset keys: `reranker_api_url`, `reranker_api_key`, `reranker_model`
- `src/utils.rs`: Added `rerank()` function — calls Cohere-compatible `/v1/rerank` endpoint
- `src/lib.rs`: Updated `rerank_hits()` to accept `RerankerConfig` parameter
- `src/lib.rs`: New blending logic:
  - If reranker available: **30% BM25 + 70% reranker**
  - If only embedding available: **60% BM25 + 40% embedding**
  - If neither: 100% BM25
- `src/lab/mod.rs`: Updated call site to pass `&self.config.reranker`

### Config
- Reranker: SiliconFlow (`https://api.siliconflow.cn/v1/rerank`, model `Qwen/Qwen3-Reranker-8B`)
- Uses same API key as embedding

### Result
- 33 tests pass, 0 warnings
- Reranker reason strings: `"reranker 0.998"`, `"reranker 0.000"` etc.
- Graceful fallback: if reranker API fails, falls back to embedding, then BM25

## Phase 8: Provider-Based Agent Config [Done]

**Date**: 2026-06-02
**Status**: Complete

### Problem
All AI roles (planner, writer, linker, reader) shared a single API endpoint and key. Users couldn't assign different models to different roles without duplicating API URL/key config.

### Solution: "Provider → Model" Architecture
- **Providers** (`ai_providers`): named channels with `api_url`, `api_key`, and `models` (alias → model name map)
- **Agents** (`agents`): per-role config referencing `provider` name + `model` alias
- Fallback: if agent not configured, falls back to legacy `ai` config

### Config Structure
```json
"ai_providers": {
    "siliconflow": {
        "api_url": "https://api.siliconflow.cn",
        "api_key": "sk-...",
        "models": {"fast": "Qwen/Qwen3-8B", "smart": "Qwen/Qwen3-32B"}
    }
},
"agents": {
    "planner": {"provider": "siliconflow", "model": "fast"},
    "writer": {"provider": "siliconflow", "model": "smart"}
}
```

### Changes
- `src/config.rs`: Added `AiProviderEntry` struct (api_url, api_key, models HashMap)
- `src/config.rs`: Added `AgentRoleConfig` struct (provider, model)
- `src/config.rs`: Added `ai_providers: HashMap<String, AiProviderEntry>` and `agents: HashMap<String, AgentRoleConfig>` to `ResearchConfig`
- `src/config.rs`: Added `resolve_agent(&self, role: &str) -> Option<(String, String, String)>` — resolves provider+model alias → (api_url, api_key, model_name)
- `src/config.rs`: Added set/unset support for `ai_providers.<name>.api_url`, `ai_providers.<name>.api_key`, `ai_providers.<name>.models.<alias>`, `agents.<role>.provider`, `agents.<role>.model`
- `src/lab/mod.rs`: `generate_model_report` uses `self.config.resolve_agent("writer")`
- `src/lab/mod.rs`: `ai_decompose_queries` uses `self.config.resolve_agent("planner")`

### Result
- 33 tests pass, 0 warnings
- Planner uses "fast" model (Qwen/Qwen3-8B), writer uses "smart" model (Qwen/Qwen3-32B)
- Old `ai` config still works as fallback

## Phase 9: Context Engineering & Harness Engineering [Done]

**Date**: 2026-06-02
**Status**: Complete

### Problem
8 optimization items identified across context engineering and harness engineering dimensions.

### Changes

#### Query Feedback Signals (P1)
- `src/models.rs`: Added `accepted_source_count: usize` and `was_productive: bool` to `SearchAttemptRecord` (`#[serde(default)]`)
- `src/lab/search_ops.rs`: Added `accepted_source_count` param to `record_search_attempt`, added `productive_queries()` method
- Enables tracking which queries produced results for future planner decisions

#### Reader Context Trimming (P0)
- `src/lib.rs` `chunk_source()`: Added relevance-based paragraph selection — scored by `token_overlap` against topic+query, top-12 kept (was unbounded 24)
- Reduces noise in reader context sent to AI

#### Evidence Gate Enhancement (P2)
- `src/lib.rs` `evidence_gate()`: Differentiated authority scores (academic +20, official +18, media +8, community +3), archived/cached penalty (-10), freshness bonus (+5)
- Better discrimination between high and low quality sources

#### Provider Smart Routing (P3)
- `src/providers/hybrid.rs`: Added `QueryProfile` + `analyze_query()` detecting code keywords and Chinese characters
- Code provider runs if `run_code_surface || profile.is_code`; Minimax skipped for pure code queries without Chinese

#### Planner Context Enhancement (P0)
- `src/lab/mod.rs`: Added `build_state_summary()` — builds research state summary (accepted sources, productive/failed queries, key claims)
- Updated `ai_decompose_queries()` — accepts `state_summary` param, tells AI about existing findings and evidence gaps
- Planner now makes informed decisions about what to search next

#### Quality Gate (P1)
- `src/lab/mod.rs` `run_round()`: After query loop, if 0 sources collected and not first round, triggers retry with fresh AI-generated queries
- Prevents silent failures where a round produces nothing

#### Adaptive Pipeline (P3)
- `src/lab/mod.rs` `plan_queries()`: `adaptive_max` reduces query count in later rounds (`max_sources - current_round`, min 3)
- First round gets full budget; later rounds focus on fewer, higher-quality queries

#### Writer Context Compression (P2)
- `src/lab/mod.rs`: Added `build_research_summary()` — compresses cross-round findings into structured summary (claim status groups, source authority distribution)
- Injected into `generate_model_report()` prompt as `research_summary` field
- Writer gets concise overview instead of raw accumulated data

### Result
- 33 tests pass, 0 warnings

## Phase 9: Fault Tolerance, PDF, Search Quality [Done]

### Retry with Exponential Backoff
- `src/utils.rs`: Added `retry(max_retries, op)` — generic retry with exponential backoff (1s base, doubled each attempt, 30s cap, ±25% jitter)
- `src/utils.rs`: Added `retry_http(max_retries, op)` — HTTP-specific retry for 429/5xx errors
- Wrapped `embed_texts()` and `rerank()` in `utils.rs` with `retry(2, ...)`
- Wrapped `exa::search()` and `exa::search_code_context()` in `providers/exa.rs` with `retry(2, ...)`
- Wrapped `zhipu::search()` MCP call in `providers/zhipu.rs` with `retry(2, ...)`
- 4 retry tests: succeed_on_first_try, succeed_after_failures, exhausts_all_attempts, zero_retries

### PDF Text Extraction
- Added `lopdf = "0.35"` to `Cargo.toml` (pure-Rust PDF parser, no system deps)
- `src/utils.rs`: Added `is_pdf_url(url)` — detects `.pdf` URLs
- `src/utils.rs`: Added `fetch_pdf_text(url, timeout_secs)` — downloads PDF + extracts text
- `src/utils.rs`: Added `extract_pdf_text_from_bytes(bytes)` — parses PDF with lopdf
- `src/lib.rs` `chunk_source()`: When text is empty and URL is a PDF, fetches and extracts PDF text
- 2 PDF tests: is_pdf_url_detects_pdf, extract_pdf_text_from_bytes_rejects_garbage

### Cross-Provider Dedup
- `src/lib.rs`: Added `normalize_url_for_dedup()` — strips UTM/click tracking params, trailing slashes
- `src/lib.rs` `rerank_hits()`: Deduplicates hits by normalized URL before BM25 scoring; keeps hit with most text

### Evidence Gate Novelty Bonus
- `src/lib.rs`: Added `extract_root_domain()` — extracts root domain from URL (strips www, port, path)
- `src/lib.rs` `evidence_gate()`: Added `accepted_domains: &[String]` param; +8 score bonus for novel domains
- `src/lab/mod.rs`: Updated 2 call sites to build `accepted_domains` from accepted sources
- 1 novelty test: evidence_gate_novel_domain_bonus

### Result
- 40 tests pass (7 new), 0 warnings
- All HTTP calls have retry protection (embedding, reranker, exa, zhipu)
- PDF URLs in search results are automatically parsed
- Cross-provider duplicate URLs are deduplicated before ranking
- Source diversity is encouraged through domain novelty bonus

## Phase 10: Kimi Search Provider [Done]

**Date**: 2026-06-02
**Status**: Complete

### Problem
Need to add Kimi (Moonshot AI) as a search provider to expand search sources.

### Solution
Kimi's `$web_search` is a builtin_function — Kimi executes the search internally on its backend. We call the Moonshot API with the `$web_search` tool, and Kimi returns an AI-synthesized answer with sources.

### API Details
- Endpoint: `https://api.moonshot.cn/v1/chat/completions` (OpenAI-compatible)
- Model: `kimi-k2.6` (recommended for web search due to larger context)
- Tool: `{"type": "builtin_function", "function": {"name": "$web_search"}}`
- Must disable thinking: `"thinking": {"type": "disabled"}`
- Flow: send chat completion → Kimi returns `finish_reason=tool_calls` → pass arguments back as-is → Kimi executes search → returns final answer with `finish_reason=stop`

### Changes
- `src/providers/kimi.rs` (94 lines): New provider — calls Moonshot API with `$web_search` builtin_function, handles tool_call loop
- `src/providers/mod.rs`: Added `pub(crate) mod kimi`
- `src/lib.rs`: Added `SearchProvider::Kimi` variant to enum + display impl
- `src/lab/search_ops.rs`: Added Kimi dispatch (calls `kimi::search` with api_key, model, timeout)
- `src/lab/mod.rs`: Added `kimi` to imports
- `src/providers/hybrid.rs`: Added `kimi_api_key` and `kimi_model` to `HybridInputs`, added Kimi thread spawn and join handler
- `src/config.rs`: Added `KimiConfig` struct (api_key, model), added to `SearchProvidersConfig`, added sync_field!/set/unset entries
- `~/.config/research/config.json`: Added `kimi` section with model default

### Result
- 40 tests pass, 0 warnings
- Kimi provider available as `--search-provider kimi` or in hybrid mode
- Config: `research config set kimi_api_key <key>` to enable
- E2E verified: 20 claims, 6 sources, 6 leads, full Chinese report generated in ~450s
- Kimi API quirks: temperature must be 0.6 (not 0.3), each search takes ~60s (search + synthesis)
