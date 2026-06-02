[![Rust](https://img.shields.io/badge/rust-2024+-ed8225?style=flat-square&logo=rust&logoColor=white)](https://rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-22C55E?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-cross--platform-8B5CF6?style=flat-square)]()

# research

**AI-driven multi-source research CLI.** Persistent research workspace with multi-provider search, 3-tier reranking, AI query decomposition, and crash-safe JSONL persistence. Built in Rust with blocking reqwest, no async runtime.

[🔧 Quick Start](#quick-start) · [🏗️ Architecture](#architecture) · [📋 Commands](#commands) · [⚙️ Configuration](#configuration)

## Why research

|  | Exa | Perplexity | Tavily | **research** |
|---|---|---|---|---|
| **Multi-provider parallel** | ❌ single | ❌ single | ❌ single | **✅ hybrid (4 providers)** |
| **3-tier reranking** | ❌ | ❌ basic | ❌ | **✅ cross-encoder + bi-encoder + BM25** |
| **Incremental research** | ❌ | ❌ | ❌ | **✅ resume, add directions** |
| **Crash-safe persistence** | ❌ | ❌ | ❌ | **✅ JSONL append-only** |
| **PDF extraction** | ❌ | ❌ | ❌ | **✅ auto-detect + lopdf** |
| **Graph visualization** | ❌ | ❌ | ❌ | **✅ embedded HTML viewer** |
| **Self-hosted** | ❌ | ❌ | ❌ | **✅ local binary** |

> Search tools give you results. Research gives you a workspace — persistent, resumable, multi-source, with evidence tracking.

## Quick Start

```bash
# Build
cargo build --release

# Quick search (hybrid — all providers in parallel)
./target/release/research start --topic "Rust async runtime comparison 2025" --search-provider hybrid

# Deep research (multi-round with AI decomposition)
./target/release/research start --topic "state of LLM agents 2025" --search-provider hybrid --json

# Read the report
./target/release/research read --topic-id <topic_id> --target report

# Visual graph viewer
./target/release/research serve --topic-id <topic_id>

# Resume interrupted research
./target/release/research resume --topic-id <topic_id>

# Add follow-up direction
./target/release/research add-direction --topic-id <topic_id> --direction "compare with 2024 results"
```

## Architecture

```
Planner → Searcher → Verifier → Reader → Linker → Writer → Reviewer
```

Single `Lab` orchestrator runs a 7-role pipeline per round:

| Role | What it does |
|------|-------------|
| **Planner** | AI-decomposes topic into diverse queries |
| **Searcher** | Dispatches to providers (parallel in hybrid mode) |
| **Verifier** | Evidence gate: authority, novelty, freshness scoring |
| **Reader** | Chunks sources, relevance-based paragraph selection |
| **Linker** | Builds entity/claim/source graph |
| **Writer** | AI-synthesizes report with evidence citations |
| **Reviewer** | Identifies gaps, generates follow-up leads |

### Reranking pipeline

```
raw hits → cross-encoder (Qwen3-Reranker-8B) → bi-encoder (Qwen3-VL-Embedding-8B) → BM25
           30% BM25 + 70% reranker            60% BM25 + 40% embedding           100% lexical
```

Fallback chain: cross-encoder → bi-encoder → BM25 only.

### Search providers

| Provider | Source | Best for |
|----------|--------|----------|
| `hybrid` | All providers parallel | General search |
| `minimax` | MiniMax MCP | Chinese content, coding |
| `zhipu` | Zhipu MCP | Chinese web search |
| `kimi` | Moonshot AI builtin `$web_search` | Deep synthesis (~60s/query) |
| `exa` | Exa API | English web, code context |
| `code` | Exa MCP code context | Code-specific search |

## Commands

All commands return JSON with `--json` flag.

```bash
research start --topic "..." [--level quick|deep|research] [--search-provider hybrid]
research status [--topic-id ...]
research read --topic-id ... --target report|claims|sources|leads|notes|timeline
research add-direction --topic-id ... --direction "..."
research resume --topic-id ...
research search --topic-id ... --query "..." [--reason "..."]
research serve --topic-id ...         # local HTTP graph viewer
research config set <key> <value>     # manage ~/.config/research/config.json
research config get <key>
research config list
```

## Configuration

Config file: `~/.config/research/config.json`

```bash
# Search providers
research config set search.providers.minimax.api_key <key>
research config set search.providers.zhipu.api_key <key>
research config set search.providers.kimi.api_key <key>
research config set search.providers.exa.api_key <key>

# AI providers (for report generation)
research config set ai_providers.siliconflow.api_url https://api.siliconflow.cn
research config set ai_providers.siliconflow.api_key <key>
research config set ai_providers.siliconflow.models.fast Qwen/Qwen3-8B
research config set ai_providers.siliconflow.models.smart deepseek-ai/DeepSeek-V4-Pro

# Agent model assignment
research config set agents.planner.provider siliconflow
research config set agents.planner.model fast
research config set agents.writer.provider siliconflow
research config set agents.writer.model smart

# Embedding (for semantic reranking)
research config set embedding.api_url https://api.siliconflow.cn/v1/embeddings
research config set embedding.api_key <key>
research config set embedding.model Qwen/Qwen3-VL-Embedding-8B

# Reranker (for cross-encoder reranking)
research config set reranker.api_url https://api.siliconflow.cn/v1/rerank
research config set reranker.api_key <key>
research config set reranker.model Qwen/Qwen3-Reranker-8B

# Timeouts
research config set timeouts.mcp_timeout_secs 45
research config set timeouts.ai_timeout_secs 120
research config set timeouts.viewer_timeout_ms 500
```

## Storage

```
~/.config/research/
├── config.json
└── projects/<topic_id>/
    ├── state.json
    ├── report.md
    ├── snapshot.html          # self-contained static viewer page
    ├── sources.jsonl
    ├── accepted_sources.jsonl
    ├── chunks.jsonl
    ├── search_attempts.jsonl
    ├── agent_runs.jsonl
    ├── events.jsonl
    ├── leads.jsonl
    └── notes/<id>.md
```

## 🔧 Building

- **Rust** ≥ 1.85 (edition 2024)
- **No C library required** — pure Rust
- `cargo build --release` to compile

### Key dependencies

| crate | note |
|-------|------|
| `clap` | derive-based CLI parsing |
| `reqwest` | blocking HTTP with rustls |
| `anyhow` | error handling |
| `serde` / `serde_json` | JSON serialization |
| `lopdf` | PDF text extraction |
| `ulid` | sortable unique IDs |
| `chrono` | timestamps |
| `rand` | retry jitter |

## Testing

```bash
cargo test                          # 44 tests, all in src/lib.rs
cargo test start_resume             # run one test by name prefix
```

Tests construct `Cli` structs and call `run()` directly — no shell-outs. Always use `json: true` and `Deterministic` provider in tests.

---

⭐ Found this useful? Give it a star on [GitHub](https://github.com/anthropics/research).
