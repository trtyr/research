# PPT Master v2.7.0 — Competitor Technical Analysis

## Executive Summary

PPT Master is an AI-driven presentation generation system built as a **Claude Code skill** (not a standalone app). It converts source documents into natively editable PPTX files through a sophisticated pipeline: **Source → Strategist (planning) → Image Generator → Executor (SVG) → Quality Check → Post-processing → PPTX**.

### Why It Works (The 7 Secrets)

Previous Rust rewrite attempts failed because they missed the **non-obvious quality differentiators**. These are not features you can see in the UI — they're pipeline architecture decisions that make the output quality dramatically better than naive approaches.

| # | Secret | What It Does | Why Naive Reimplementation Misses It |
|---|--------|-------------|--------------------------------------|
| 1 | **spec_lock.md** | Machine-readable execution contract re-read before EVERY page | Prevents context-compression drift in long decks. #1 quality differentiator. |
| 2 | **Eight Confirmations** | Bundled blocking gate (8 decisions in one user interaction) | After confirmation, pipeline runs unattended. Design choices are correlated, so resolving them together produces coherence. |
| 3 | **page_rhythm** | anchor/dense/breathing classification per page | Prevents uniform "AI-generated card grid" look. Each page has distinct visual density. |
| 4 | **Deck-wide image lock** | 3D lock: rendering × palette × type | All AI images share same visual style. Without this, every image gets its own style drift. |
| 5 | **72 image layout patterns** | Primary Structures + Modifier Layers taxonomy | Forces varied compositions. Fights AI's tendency to default to bare left-third or side-by-side. |
| 6 | **Sequential page generation** | Not parallel/batched | Visual continuity across pages. Sub-agents would start with stale partial snapshots. |
| 7 | **SVG quality checker** | Validates SVG against spec_lock + blacklist before export | Catches banned features that only surface at PPTX export time. |

### Project Scale

| Metric | Value |
|--------|-------|
| Python LOC | 38,040 |
| Prompt/Reference Markdown | 16,588 lines |
| SVG Templates | 11,783 files |
| Chart Templates | 57 |
| Icon Library | 11,600+ icons, 5 libraries |
| Example Projects | 17 (229 pages) |
| AI Image Backends | 13 |
| Web Image Providers | 4 |
| TTS Providers | 5 |
| Canvas Formats | 8 |

### Architecture at a Glance

```
User Input (PDF/DOCX/XLSX/URL/MD)
    ↓
[Source Conversion] → Markdown
    ↓
[Project Init] → project/ directory structure
    ↓
[Template] (optional) → copy template files
    ↓
[Strategist] → Eight Confirmations → design_spec.md + spec_lock.md
    ↓
[Image Generator] → AI images + web search images
    ↓
[Executor] → SVG pages (sequential, one-by-one)
    ↓
[Quality Check] → svg_quality_checker.py (mandatory, 0 errors)
    ↓
[Post-processing] → finalize SVG → svg_to_pptx → native PPTX
    ↓
Output: editable PPTX (DrawingML shapes, not images)
```

### Key Design Decision: SVG as Intermediate Format

The single most important architectural choice. SVG was chosen over:
- **Direct DrawingML**: Too verbose, insufficient AI training data
- **HTML/CSS**: Structural mismatch (document flow vs absolute canvas positioning)
- **WMF/EMF**: No AI training data
- **Embedded images**: Destroys editability

SVG wins because it shares DrawingML's worldview: both are **absolute-coordinate 2D vector graphics** formats. The conversion is translation between dialects, not format mismatch.

### Three-Role System

Single agent with role switching (NOT parallel sub-agents):
1. **Strategist** — Content analysis, Eight Confirmations, produces design_spec.md + spec_lock.md
2. **Image_Generator** — AI image generation with deck-wide rendering + palette lock
3. **Executor** — SVG page generation (3 variants: General, Consultant, Consultant-Top)

### Files in This Analysis

| File | Contents |
|------|----------|
| `00-EXECUTIVE-SUMMARY.md` | This file — overview and key findings |
| `01-ARCHITECTURE-DEEP-DIVE.md` | Complete pipeline architecture, data flow, project lifecycle |
| `02-SVG-TO-PPTX-ENGINE.md` | The most complex component — SVG→DrawingML conversion internals |
| `03-PROMPT-ENGINEERING.md` | Role system, prompt design, anti-drift mechanisms |
| `04-IMAGE-SYSTEM.md` | AI image generation, web search, manifest system, licensing |
| `05-RUST-REWRITE-GUIDE.md` | Practical guide for Rust reimplementation — what to port, what to redesign |

---

*Source: hugohe3/ppt-master v2.7.0 (MIT License) — cloned from GitHub*
*Analysis date: 2025-05-21*
