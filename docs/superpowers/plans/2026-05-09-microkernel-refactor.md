# Research CLI Microkernel Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use backend-subagent-driven-development (recommended) or backend-executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the `research` Rust CLI from monolithic `src/lib.rs` into a strict microkernel architecture while preserving behavior, CLI surface, and test results.

**Architecture:** Extract shared models/config/storage/viewer/providers first, then introduce narrow role traits and a thin `Lab` kernel in `src/lab.rs`. Keep `main.rs` unchanged and preserve `pub fn run(cli: Cli) -> Result<Value>`.

**Tech Stack:** Rust edition 2024, `anyhow`, `clap`, blocking `reqwest`, `serde`, `serde_json`, `chrono`, `ulid`, `tempfile`

---

## File structure

- Modify: `src/lib.rs` — shrink to CLI definitions, module declarations, re-exports, `run()`
- Create: `src/models.rs` — shared state/artifact records and internal orchestration types
- Create: `src/config.rs` — config structs, normalization, set/unset, resolved config loading
- Create: `src/storage.rs` — project paths, state/graph persistence, JSONL/event/artifact I/O
- Create: `src/viewer.rs` — viewer spawn, serve, snapshot, HTTP state responses
- Create: `src/providers/*.rs` — provider-specific search implementations
- Create: `src/planner.rs`, `src/searcher.rs`, `src/verifier.rs`, `src/reader.rs`, `src/linker.rs`, `src/writer.rs`, `src/reviewer.rs` — role traits and default deterministic implementations
- Create: `src/lab.rs` — thin kernel, lifecycle commands, `run_round`, collaborator wiring

### Task 1: Extract models
**Files:** `src/models.rs`, `src/lib.rs`
- [ ] Move `State`, artifact record types, `SearchHit`, `RankedSearchHit`, `AgentRole`, `AgentRunRecord`, `GateDecision` into `src/models.rs`.
- [ ] Update `src/lib.rs` to `mod models;` and import/re-export moved public data types.
- [ ] Run: `cargo test start_resume_and_export_project -- --nocapture`
- [ ] Commit: `git commit -m "refactor: extract shared models"`

### Task 2: Extract config
**Files:** `src/config.rs`, `src/lib.rs`
- [ ] Move `ResearchConfig` cluster, `ResolvedConfig`, normalize/load/save/set/unset logic into `src/config.rs`.
- [ ] Keep parsing/path helpers accessible from `src/lib.rs` or move only when all call sites are updated.
- [ ] Run: `cargo test config_commands_drive_cli_defaults -- --nocapture`
- [ ] Commit: `git commit -m "refactor: extract config module"`

### Task 3: Extract storage
**Files:** `src/storage.rs`, `src/lib.rs` or `src/lab.rs`
- [ ] Introduce concrete `Storage { root: PathBuf }`.
- [ ] Move project root/path helpers, state/graph load-save, JSONL I/O, events, mirrors, manifests, notes, reports into storage.
- [ ] Replace direct file I/O in `Lab` with `self.storage.*`.
- [ ] Run: `cargo test save_state_does_not_rebuild_graph_from_jsonl_artifacts -- --nocapture`
- [ ] Commit: `git commit -m "refactor: extract storage service"`

### Task 4: Extract viewer
**Files:** `src/viewer.rs`, `src/lib.rs` or `src/lab.rs`
- [ ] Move `ensure_viewer`, `serve`, `handle_http`, `viewer_state`, snapshot logic, HTML rendering helpers into `src/viewer.rs`.
- [ ] Preserve `env::current_exe()` spawn behavior and `#[cfg(test)]` no-spawn behavior.
- [ ] Run: `cargo test start_persists_graph_and_exposes_it_in_viewer_state -- --nocapture`
- [ ] Commit: `git commit -m "refactor: extract viewer module"`

### Task 5: Extract providers
**Files:** `src/providers/mod.rs`, `src/providers/*.rs`, caller modules
- [ ] Move deterministic, exa, code, zhipu, minimax, hybrid provider logic into provider modules.
- [ ] Keep provider modules returning raw recall hits; leave ranking/gating to verifier.
- [ ] Run: `cargo test code_surface_config_is_canonical_and_auto_detects_technical_topics -- --nocapture` and `cargo test mcp_text_extraction_reads_exa_code_context_shape -- --nocapture`
- [ ] Commit: `git commit -m "refactor: extract search providers"`

### Task 6: Introduce role traits
**Files:** `src/planner.rs`, `src/searcher.rs`, `src/verifier.rs`, `src/reader.rs`, `src/linker.rs`, `src/writer.rs`, `src/reviewer.rs`
- [ ] Define narrow traits and input/output structs for each role.
- [ ] Move deterministic logic from `Lab` methods into default role implementations.
- [ ] Keep roles pure: compute outputs, no direct file I/O, no access to full `Lab`.
- [ ] Run: `cargo test evidence_gate_rejects_wrong_identifier_candidates -- --nocapture` and `cargo test local_project_search_can_supply_accepted_evidence -- --nocapture`
- [ ] Commit: `git commit -m "refactor: extract pipeline roles"`

### Task 7: Create thin kernel in `src/lab.rs`
**Files:** `src/lab.rs`, `src/lib.rs`
- [ ] Move `Lab`, `StartInput`, `SearchProfile`, lifecycle commands, and `run_round` orchestration into `src/lab.rs`.
- [ ] Make `Lab` hold `Storage`, `Viewer`, and boxed role implementations.
- [ ] Preserve command behavior and JSON output shapes.
- [ ] Run: `cargo test start_resume_and_export_project -- --nocapture`
- [ ] Commit: `git commit -m "refactor: create microkernel lab"`

### Task 8: Slim `src/lib.rs` and relocate tests
**Files:** `src/lib.rs`, affected module files
- [ ] Reduce `src/lib.rs` to CLI definitions, module declarations, re-exports, and `run()`.
- [ ] Move tests near the modules they exercise without changing assertions.
- [ ] Run: `cargo build && cargo test`
- [ ] Commit: `git commit -m "refactor: slim lib and relocate tests"`

## Verification
- `cargo build`
- `cargo test`
- Confirm all 13 existing tests pass unchanged in behavior
- Confirm `main.rs` remains unchanged
- Confirm `pub fn run(cli: Cli) -> Result<Value>` remains unchanged
