# research CLI

Standalone Rust CLI for the persistent research workspace. Single crate, single `Lab` orchestrator.

## Build & Test

```bash
cargo build
cargo test                          # 44 tests, all in src/lib.rs
cargo test start_resume             # run one test by name prefix
cargo run -- start --topic "..."    # all commands go through cargo run --
```

## STRUCTURE

```
research/
├── src/
│   ├── main.rs           # thin CLI entry, JSON wrapper (39L)
│   ├── lib.rs            # CLI defs, dispatch, viewer HTML, tests (~2986L)
│   ├── lab/              # Lab orchestrator (5 files, 2521L total)
│   │   ├── mod.rs        # core lifecycle, AI, planner, verifier, writer, reviewer (1629L)
│   │   ├── graph.rs      # graph operations: load/save/build/nodes/edges (245L)
│   │   ├── storage_ops.rs # state/JSONL: load/save/list/read/write/append (141L)
│   │   ├── search_ops.rs # search orchestration: search/hybrid/record (170L)
│   │   └── render.rs     # report/Markdown rendering (336L)
│   ├── config.rs         # config load/normalize/set (638L)
│   ├── models.rs         # 14 data types: State, SourceRecord, etc.
│   ├── providers/        # search backend implementations (7 files)
│   ├── storage.rs        # JSONL persistence
│   ├── rendering.rs      # report section builders
│   ├── graph_utils.rs    # graph node/edge building
│   ├── local_search.rs   # local file search
│   ├── mcp.rs            # MCP protocol parsing
│   ├── utils.rs          # id(), now(), slug(), BM25, embedding, reranker, retry, PDF
│   └── viewer.rs         # viewer subprocess + HTTP server
├── docs/
└── ppt-master-analysis/
```

## ARCHITECTURE

`Lab` struct (4 fields, ~74 methods) is the microkernel — single `run_round` pipeline:

```
Planner → Searcher → Verifier → Reader → Linker → Writer → Reviewer
```

All roles are methods on `Lab`, not separate actors. Only Writer/Reviewer can be AI-assisted by default. Synthesis fallback: `generate_model_report` (if API) → `render_deterministic_report` (always works). Three search levels (quick/deep/research) are parameter profiles on the same code path.

Lab methods are split across `lab/mod.rs` + 4 sub-modules (`graph.rs`, `storage_ops.rs`, `search_ops.rs`, `render.rs`) but remain a single struct — no trait separation.

> Details: `src/AGENTS.md` for module-level conventions.

## CODE MAP

**Entry points:**
| Entry | File | Description |
|-------|------|-------------|
| `main()` | main.rs:6 | binary entry, JSON output wrapper |
| `run()` | lib.rs:80 | CLI dispatch → builds Lab → routes commands |
| `Lab::start()` | lab/mod.rs:189 | research lifecycle entry |
| `Lab::run_round()` | lab/mod.rs:752 | per-round 7-role pipeline |

**Hot symbols (degree):**
| Symbol | Degree | File |
|--------|--------|------|
| `Lab::new` | 84 | lab/mod.rs |
| `ResearchConfig` | 59 | config.rs |
| `run_round` | 49 | lab/mod.rs |
| `SearchHit` | 46 | models.rs |
| `SourceRecord` | 33 | models.rs |
| `State` | 29 | models.rs |

**Feature clusters:** `lab` (81 sym/882 edges), `lib` (163/732), `models` (173/513), `config` (125/420)

## HEALTH

Score: **94/100**. No cycles, no god modules flagged, no dead public symbols.

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Add CLI command | lib.rs `Command` enum + lab.rs handler method |
| Add search provider | providers/new.rs + mod.rs + lib.rs search fn |
| Change orchestration | lab/mod.rs → `run_round` (lines 752-1058) |
| Change config key | config.rs → `ResearchConfig` + `normalize()`/`set()`/`unset()` |
| Data model | models.rs → JSONL-compatible serde structs |
| Render output | rendering.rs → numbered_findings, evidence_base, etc. |

## CONVENTIONS

- **Visibility**: `pub(crate)` internal; only `Cli`, `Command`, enums, `run()` are `pub`
- **Errors**: `anyhow` → `bail!()` / `.context()`; `Result<Value>` at top; no custom Error types
- **IDs**: `format!("{prefix}_{}", Ulid::new().to_string().to_lowercase())`
- **Timestamps**: `Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)`
- **Persistence**: JSONL append-only (crash-safe); `.md` mirrors for readability
- **Tests**: construct `Cli` → call `run()`; `json: true`, `Deterministic` provider, `tempfile::tempdir()`
- **Config**: nested keys preferred; ~50 legacy flat `#[serde(skip_serializing)]` fields normalize on save

## ANTI-PATTERNS

- Provider implementations live in `providers/` modules; lib.rs only contains shared utilities (`dedupe_hits`, `short_excerpt`, etc.)
- `Lab` is split into 5 files (`lab/mod.rs` + 4 sub-modules) but still a single struct — no trait separation
- Config `set()`/`normalize()`/`unset()` use macros (`config_key_set!`/`config_key_unset!`/`sync_field!`) — add new keys to all three
- `#[cfg(test)]` disables viewer spawn — but `viewer_html`, `viewer_state`, and `handle_http` routing are tested directly (7 viewer tests)

## UNIQUE STYLES

- **Self-spawning viewer**: `env::current_exe()` forks subprocess to serve HTTP
- **Embedded SPA**: ~1300 lines HTML/CSS/JS in `viewer_html()` Rust string
- **Dual config compat**: `normalize()` syncs legacy flat keys ↔ nested structs

## Commands

All commands return JSON. Key subcommands:

```bash
research start --topic "..." --level quick|deep|research
research status [--topic-id ...]
research read --topic-id ... --target report [--offset 1 --limit 200]
research add-direction --topic-id ... --direction "..."
research resume --topic-id ...
research serve --topic-id ...       # local HTTP graph viewer
```

Use `research config set <dotted.key> <value>` to manage `~/.config/research/config.json`.

## Search providers

| provider | what it does |
|----------|-------------|
| `deterministic` | canned results, tests/offline only |
| `exa` | direct Exa API (`POST https://api.exa.ai/search`) |
| `code` | Exa MCP `get_code_context_exa`, reuses exa API key |
| `zhipu` | Zhipu MCP web search via Streamable HTTP |
| `minimax` | MiniMax Code Plan MCP, spawned via `uvx` |
| `kimi` | Moonshot AI builtin `$web_search` via chat completions API |
| `hybrid` | runs multiple providers, normalizes through shared reranker + evidence gate |

Hybrid mode treats all providers as recall-only. Results pass through a
provider-neutral reranker before hitting the evidence gate. If one provider
fails but another returns accepted evidence, the round continues.

## Reranking

The search pipeline uses a 3-tier reranking system:

1. **Cross-encoder reranker** (highest quality): `Qwen/Qwen3-Reranker-8B` via SiliconFlow API
   - Processes (query, document) pairs together for accurate relevance scoring
   - Blending: 30% BM25 + 70% reranker
2. **Bi-encoder embedding** (fallback): `Qwen/Qwen3-VL-Embedding-8B` via SiliconFlow API
   - Separate query/doc encoding, faster but less accurate
   - Blending: 60% BM25 + 40% embedding
3. **BM25 only** (baseline): Lexical matching with Robertson-Sparck Jones IDF

Config: `reranker.api_url`, `reranker.api_key`, `reranker.model` in `~/.config/research/config.json`

## Tests

- All 33 tests in `#[cfg(test)] mod tests` at the bottom of `lib.rs`.
- Tests construct `Cli` structs and call `run()` directly — they do NOT shell out.
- Always set `json: true` and `search_provider: Some(SearchProvider::Deterministic)` in test Cli structs.
- Use `tempfile::tempdir()` for isolated storage.
- The viewer auto-spawn is gated by `#[cfg(test)]` returning early — tests won't fork.

## Storage

```
~/.config/research/
├── config.json
└── projects/<topic_id>/
    ├── state.json
    ├── manifest.json
    ├── report.md
    ├── snapshot.html
    ├── sources.jsonl / accepted_sources.jsonl / rejected_sources.jsonl
    ├── candidate_sources.jsonl
    ├── chunks.jsonl
    ├── search_attempts.jsonl
    ├── agent_runs.jsonl
    ├── events.jsonl
    ├── leads.jsonl
    └── notes/<id>.md
```

## Viewer

- `serve` starts a local HTTP server on a free port. The viewer polls `/api/state`.
- During `start`/`resume`, the CLI spawns itself as a subprocess (`env::current_exe()`) to run the viewer.
- On completion, `snapshot.html` is written — a self-contained static page, no server needed.

## Key Dependencies

| crate | note |
|-------|------|
| `clap` | derive-based CLI parsing |
| `reqwest` | blocking HTTP (not async) |
| `anyhow` | error handling |
| `serde`/`serde_json` | JSON everywhere |
| `ulid` | sortable unique IDs |
| `chrono` | timestamps (with `serde` feature) |
| `tempfile` | dev-only, test isolation |

## Gotchas

- Legacy flat config keys (`search_provider`, `exa_api_key`) still parse but get rewritten to nested on save.
- Hybrid search runs multiple providers in parallel; results are normalized through a shared BM25+semantic reranker and evidence gate.
- The `deterministic` search provider returns canned results — only for tests/offline dev.
- Config is loaded from `~/.config/research/config.json` by default; override with `--config`.
- Project root defaults to `~/.config/research/projects/`; override with `--root`.
- `run_round` is the only code path that executes research work; every `start`/`resume` call funnels through it.
- The viewer subprocess spawn means the binary must exist at `env::current_exe()` — won't work from `cargo run` in a way that cleanly daemonizes.
