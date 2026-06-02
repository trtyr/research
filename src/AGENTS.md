# src/ — Source code conventions

All business logic. 20 files / 8,132 lines. Blocking reqwest, no tokio, edition 2024.

## STRUCTURE

```
src/
├── main.rs           # thin CLI entry, JSON wrapper (39L)
├── lib.rs            # CLI defs, dispatch, viewer HTML, tests (~2986L)
├── lab/              # Lab orchestrator (5 files, 2521L total)
│   ├── mod.rs        # core lifecycle, AI, planner, verifier, writer, reviewer (1629L)
│   ├── graph.rs      # graph operations: load/save/build/nodes/edges (245L)
│   ├── storage_ops.rs # state/JSONL: load/save/list/read/write/append (141L)
│   ├── search_ops.rs # search orchestration: search/hybrid/record (170L)
│   └── render.rs     # report/Markdown rendering (336L)
├── config.rs         # config load/normalize/set/unset (638L)
├── models.rs         # 14 data types: State, SourceRecord, etc. (300L)
├── storage.rs        # JSONL append-only persistence (163L)
├── utils.rs          # id(), now(), slug(), BM25, embedding, reranker, retry, PDF (~780L)
├── mcp.rs            # MCP protocol: SSE, JSON-RPC (137L)
├── viewer.rs         # HTTP server + viewer snapshot (125L)
├── local_search.rs   # file-walking project search (117L)
├── graph_utils.rs    # graph node/edge upsert (103L)
├── rendering.rs      # report section builders (98L)
├── types.rs          # StartInput, SearchProfile (29L)
└── providers/        # search backend implementations (7 files)
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add CLI command | lib.rs `Command` enum + lab.rs handler | match in `run()` routes to Lab method |
| Add search provider | providers/new.rs + mod.rs + lib.rs search fn | see providers/ exa.rs as template |
| Change orchestration | lab/mod.rs → `run_round` (lines 752-1058) | 7-role pipeline, per-source processing |
| Change config key | config.rs → `ResearchConfig` | add nested struct field + sync flat key in normalize/set/unset |
| Render output | rendering.rs | numbered_findings, evidence_base, perspectives, limitations |
| Persist new data | lab/storage_ops.rs (state/JSONL) + storage.rs (low-level) + models.rs (struct) | `write_jsonl`/`append_jsonl` + `read_jsonl` |
| Modify graph | lab/graph.rs (Lab methods) + graph_utils.rs (helpers) | `load_graph`, `save_graph`, `upsert_node`, `upsert_edge` |

## CONVENTIONS

- **Visibility**: all internal functions `pub(crate)`; only `Cli`, `Command`, enums, `run()` are `pub`
- **Error handling**: `anyhow` exclusively — `bail!()` for deterministic, `.context()` for I/O, `Result<Value>` at top level. No custom Error types
- **IDs**: `format!("{prefix}_{}", Ulid::new().to_string().to_lowercase())`
- **Timestamps**: `chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)`
- **Persistence**: JSONL append-only (crash-safe); `.md` mirrors for human readability. Use `append_jsonl` for streaming, `write_jsonl` for full rewrites
- **Test style**: construct `Cli` → call `run()` (black-box via public API). Always `json: true`, `Deterministic` provider, `tempfile::tempdir()` isolation
- **Side-effect gating**: `#[cfg(not(test))]` disables viewer spawn/TCP binding in tests; `#[cfg(test)]` returns early
- **Config**: nested keys preferred (`search.providers.exa.api_key`); legacy flat keys parse but normalize on save. ~50+ `#[serde(skip_serializing)]` flat compatibility fields
- **Agent config**: `ai_providers.<name>` defines channels (api_url, api_key, models); `agents.<role>` references provider + model alias; fallback to legacy `ai` config

## ANTI-PATTERNS (this project)

- **Lab split across 5 files**: `lab/mod.rs` + 4 sub-modules (`graph.rs`, `storage_ops.rs`, `search_ops.rs`, `render.rs`) — still one struct, no trait separation
- **Config boilerplate**: `set()`/`normalize()`/`unset()` use macros (`config_key_set!`/`config_key_unset!`/`sync_field!`) — add new keys to all three

## GOTCHAS

- `#[cfg(test)]` disables viewer spawn/TCP binding — but `viewer_html`, `viewer_state`, and `handle_http` routing are tested via direct calls (7 tests)
- `env::current_exe()` for viewer spawn means binary must exist; won't work from `cargo run`
- Provider dispatch in lab/mod.rs:1428 uses manual `match` on `SearchProvider` — no shared trait abstraction
- `dedupe_hits` (lib.rs) is `pub(crate)` and consumed by providers/hybrid.rs via `use crate::dedupe_hits`
- Zhipu MCP returns double-escaped JSON — `collect_zhipu_hits` in mcp.rs handles this by parsing twice
- Kimi uses builtin `$web_search` — Kimi executes search internally, we just pass arguments back
- Kimi requires `temperature: 0.6` and takes ~60s per query (internal search + synthesis). With 4 queries, total ~450s
- Kimi's tool type is `builtin_function` (not regular `function`); must disable thinking: `"thinking": {"type": "disabled"}`
- Viewer subprocess zombie risk: each `research start` spawns a viewer via `env::current_exe() + serve`; never cleaned up. Run `pkill -f "research.*serve"` if ports exhausted
- Reranking fallback chain: cross-encoder → bi-encoder → BM25 only. If reranker API fails, falls back gracefully
- Retry: `retry(max_retries, op)` in utils.rs — exponential backoff with jitter; used by embedding, reranker, exa, zhipu HTTP calls
- PDF extraction: `is_pdf_url()` + `fetch_pdf_text()` in utils.rs; integrated into `chunk_source()` in lib.rs — auto-fetches PDF text when URL ends in `.pdf`
- Cross-provider dedup: `normalize_url_for_dedup()` strips tracking params; `rerank_hits()` deduplicates by URL before scoring
- Evidence gate novelty: `extract_root_domain()` + +8 bonus for novel domains; `accepted_domains` param tracks already-accepted domain list
