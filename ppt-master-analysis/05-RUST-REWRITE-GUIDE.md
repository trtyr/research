# PPT Master → Rust Reimplementation Guide

## Why Previous Rewrite Attempts Failed

Previous attempts likely focused on "port the code line by line" — missing the architectural decisions that make PPT Master's output quality high. The quality doesn't come from the Python code itself; it comes from the **pipeline design, prompt engineering, and anti-drift mechanisms**.

**The 7 things you MUST replicate (in priority order):**

1. **spec_lock.md re-read before every page** — The anti-drift mechanism
2. **Sequential page generation** — Not parallel, not batched
3. **Eight Confirmations** — Bundled blocking gate
4. **3D image lock** (rendering × palette × type) — Deck-wide consistency
5. **72 image layout pattern catalog** — Forces varied compositions
6. **page_rhythm** (anchor/dense/breathing) — Prevents uniformity
7. **Quality checker** — Validates SVG against spec_lock before export

---

## Component Mapping: Python → Rust

### Tier 1: Core Engine (Must Port Carefully)

| Component | Python LOC | Rust Approach | Crate Ecosystem |
|-----------|-----------|---------------|-----------------|
| SVG → DrawingML | ~5,500 | Port directly | `roxmltree` (SVG parse), `quick-xml` (XML write) |
| SVG Path Normalization | ~1,000 | Port path math | Custom (S/Q/T/A → cubic Bézier) |
| PPTX Assembly | ~845 | Port directly | `zip` crate (PPTX is a ZIP) |
| Transform Math | ~400 | Port with matrices | `euclid` or custom 2D affine |
| Image Processing | ~600 | Port or use existing | `image` crate |
| Text Flattening | ~705 | Port directly | Custom XML manipulation |
| Quality Checker | ~1,423 | Port directly | `roxmltree` for SVG validation |

### Tier 2: Pipeline Infrastructure (Redesign for Rust)

| Component | Python Approach | Rust Approach |
|-----------|----------------|---------------|
| Project Management | Filesystem scripts | Rust module with `std::fs` |
| Source Conversion | PyMuPDF, mammoth, etc. | `pdf-extract`, `docx-rs`, custom |
| Image Generation | ThreadPoolExecutor + dynamic import | `tokio` async runtime + trait objects |
| Web Fetching | curl_cffi (TLS fingerprint) | `reqwest` + TLS configuration |
| Config Management | .env files | `config` crate or TOML |
| CLI Interface | argparse | `clap` |

### Tier 3: AI Integration (Rust-Native Design)

| Component | Python Approach | Rust Approach |
|-----------|----------------|---------------|
| LLM Communication | Claude Code skill (prompt files) | API client with prompt templates |
| Role Switching | SKILL.md prompt loading | State machine with prompt composition |
| Manifest System | JSON files + atomic writes | `serde_json` + `tempfile` + `std::fs::rename` |
| Web Image Search | requests + API clients | `reqwest` + trait-based providers |

---

## Detailed Rust Crate Recommendations

### XML Processing
```toml
[dependencies]
roxmltree = "0.20"      # SVG parsing (read-only, fast)
quick-xml = "0.37"      # XML writing (PPTX generation)
```

### PPTX Generation
```toml
zip = "2.6"             # PPTX is a ZIP archive
flate2 = "1.0"          # Compression
```

### Image Processing
```toml
image = "0.25"          # Image manipulation, format conversion
base64 = "0.22"         # Base64 encoding for SVG embedding
```

### Async Runtime (for image generation + web search)
```toml
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "multipart"] }
```

### Document Parsing
```toml
pdf-extract = "0.7"     # PDF text extraction
docx-rs = "0.4"         # DOCX reading
calamine = "0.26"       # Excel reading
```

### CLI & Config
```toml
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
```

### Math & Geometry
```toml
euclid = "0.22"         # 2D geometry, transforms
```

---

## Architecture: Rust Crate Structure

```
ppt-master-rs/
├── Cargo.toml
├── crates/
│   ├── core/                    # Shared types, constants, config
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── constants.rs     # EMU_PER_PX, FONT_PX_TO_PT, etc.
│   │   │   ├── canvas.rs        # Canvas format definitions
│   │   │   ├── spec.rs          # spec_lock.md types (serde)
│   │   │   └── config.rs        # Configuration management
│   │   └── Cargo.toml
│   │
│   ├── svg/                     # SVG processing
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── parse.rs         # SVG parsing (roxmltree)
│   │   │   ├── path_norm.rs     # S/Q/T/A → cubic Bézier
│   │   │   ├── transform.rs     # Affine transform accumulation
│   │   │   ├── quality.rs       # Quality checker (port from Python)
│   │   │   ├── tspan.rs         # Text flattening
│   │   │   ├── icons.rs         # Icon expansion
│   │   │   └── finalize.rs      # SVG finalization pipeline
│   │   └── Cargo.toml
│   │
│   ├── pptx/                    # PPTX generation
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── drawingml.rs     # DrawingML element generation
│   │   │   ├── styles.rs        # Fill, stroke, gradient translation
│   │   │   ├── paths.rs         # SVG path → DrawingML custGeom
│   │   │   ├── text.rs          # SVG text → DrawingML text frames
│   │   │   ├── images.rs        # Image embedding + srcRect
│   │   │   ├── animation.rs     # Animation generation
│   │   │   ├── builder.rs       # PPTX file assembly
│   │   │   └── compat.rs        # Office compatibility (PNG fallback)
│   │   └── Cargo.toml
│   │
│   ├── source/                  # Source document conversion
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── pdf.rs           # PDF → Markdown
│   │   │   ├── docx.rs          # DOCX → Markdown
│   │   │   ├── excel.rs         # Excel → Markdown
│   │   │   ├── web.rs           # Web page → Markdown
│   │   │   └── html.rs          # HTML → Markdown
│   │   └── Cargo.toml
│   │
│   ├── image/                   # Image acquisition
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── manifest.rs      # image_prompts.json lifecycle
│   │   │   ├── registry.rs      # Backend registry (trait objects)
│   │   │   ├── generate.rs      # AI image generation
│   │   │   ├── search.rs        # Web image search
│   │   │   ├── metadata.rs      # Image analysis
│   │   │   └── backends/        # Per-backend implementations
│   │   │       ├── mod.rs
│   │   │       ├── openai.rs
│   │   │       ├── gemini.rs
│   │   │       └── ...
│   │   └── Cargo.toml
│   │
│   └── pipeline/                # Orchestration
│       ├── src/
│       │   ├── lib.rs
│       │   ├── project.rs       # Project lifecycle management
│       │   ├── strategist.rs    # Strategist role logic
│       │   ├── executor.rs      # Executor role logic
│       │   ├── roles.rs         # Role switching state machine
│       │   └── cli.rs           # CLI entry point (clap)
│       └── Cargo.toml
│
├── templates/                   # Ported from Python (mostly data)
│   ├── layouts/
│   ├── charts/
│   ├── icons/
│   └── brands/
│
├── references/                  # Prompt engineering files (Markdown)
│   ├── strategist.md
│   ├── executor-base.md
│   ├── executor-general.md
│   ├── shared-standards.md
│   ├── image-layout-patterns.md
│   └── ...
│
└── tests/
    ├── fixtures/                # Test SVG files
    ├── svg_to_pptx.rs          # Conversion tests
    └── integration/
```

---

## Critical Path: What to Build First

### Phase 1: SVG → PPTX Engine (Highest Risk, Highest Value)
This is the hardest component and the one with zero Rust ecosystem support. Build it first.

1. **Constants + coordinate math** (1 day)
   - EMU conversions, angle units, pixel math

2. **SVG parsing** (2-3 days)
   - Parse SVG with roxmltree
   - Extract element attributes, styles, transforms
   - Build in-memory SVG tree

3. **Path normalization** (2-3 days)
   - Port S/Q/T/A → cubic Bézier algorithms
   - Test with real SVG paths from templates

4. **Element dispatch** (5-7 days)
   - rect → sp
   - circle/ellipse → sp (ellipse preset)
   - path → custGeom
   - text → TextBox (with tspan flattening)
   - image → pic (with srcRect)
   - g → grpSp (recursive)

5. **Style translation** (3-4 days)
   - Solid fill, gradient fill, no fill
   - Stroke with dash patterns
   - Shadow/glow
   - Opacity

6. **PPTX assembly** (2-3 days)
   - ZIP structure
   - Content types, relationships
   - Slide XML generation
   - Media embedding

7. **Quality checker** (2-3 days)
   - Port banned element/attribute checks
   - spec_lock validation

**Phase 1 estimate: 17-25 days**

### Phase 2: Prompt Engineering System (Second Highest Value)
The prompts ARE the product quality. Port them faithfully.

1. **Role system state machine** (2 days)
   - Role switching protocol
   - Prompt file loading

2. **spec_lock system** (2 days)
   - Spec types (serde)
   - Re-read mechanism
   - update_spec propagation

3. **Template catalog** (3 days)
   - Chart templates
   - Icon libraries
   - Image layout patterns

**Phase 2 estimate: 7 days**

### Phase 3: Source Conversion + Project Management
1. PDF/DOCX/Excel → Markdown (5-7 days)
2. Project lifecycle (2-3 days)
3. Config management (1-2 days)

**Phase 3 estimate: 8-12 days**

### Phase 4: Image System
1. Backend registry + trait (2 days)
2. 3-4 key backends (3-4 days)
3. Web search providers (2 days)
4. Manifest lifecycle (2 days)

**Phase 4 estimate: 9-10 days**

---

## Key Technical Challenges

### 1. SVG Path Arc-to-Cubic (A → C)
The SVG spec F.6.5 algorithm for converting arc commands to cubic Bézier curves. This is pure math — port directly from Python.

```
Input: A rx ry x-rotation large-arc-flag sweep-flag x y
Output: One or more C (cubic Bézier) commands
Algorithm: SVG spec section F.6.5 (endpoint parameterization → center parameterization → cubic approximation)
```

### 2. Transform Matrix Accumulation
SVG transforms compose as 3×3 affine matrices. Implement clean matrix multiplication.

```rust
struct Affine2D {
    // | a c e |
    // | b d f |
    // | 0 0 1 |
    a: f64, b: f64, c: f64, d: f64, e: f64, f: f64,
}
```

### 3. Text Flattening
tspan elements with x/y/dy attributes → independent text elements. The positioning math is tricky.

### 4. Image Cropping (preserveAspectRatio → srcRect)
Calculate the source rectangle in percentage coordinates for PowerPoint's `<a:srcRect>`.

### 5. PPTX ZIP Structure
PPTX is a ZIP with specific [Content_Types].xml and relationship files. Must get these exactly right.

---

## What NOT to Port

1. **Flask live preview server** — Use Rust web framework (axum/actix) if needed, or skip entirely for CLI-first approach
2. **curl_cffi TLS fingerprint** — Only needed for WeChat bypass; may not be relevant for international version
3. **TTS providers** — Nice-to-have, build later
4. **PPTX → SVG reverse converter** — Only needed for template import
5. **Example projects** — Data, not code
6. **Gemini watermark remover** — Specific to one provider

---

## What to Port Faithfully

1. **All 16,588 lines of prompt/reference Markdown** — This IS the product quality
2. **All SVG constraints** (banned features list) — This prevents PPTX export failures
3. **All 72 image layout patterns** — This prevents visual monotony
4. **All 8 canvas format definitions** — This enables multi-format output
5. **spec_lock.md format** — This prevents context drift
6. **Quality checker rules** — This catches issues before export
7. **All template SVGs** (11,783 files) — These are data assets
8. **Icon library** (11,600+ icons) — Critical for visual quality

---

## Dependency Mapping: Python → Rust

| Python Package | Rust Alternative | Maturity |
|----------------|-----------------|----------|
| python-pptx | Custom (quick-xml + zip) | Must build |
| PyMuPDF | pdf-extract | Lower quality, may need work |
| mammoth | docx-rs | Available |
| openpyxl | calamine | Mature |
| curl_cffi | reqwest (with TLS config) | Different approach needed |
| Pillow | image | Mature |
| cairosvg | resvg | Mature |
| svglib | resvg or custom | resvg for rendering |
| flask | axum/actix (if needed) | Mature |
| edge-tts | Custom HTTP client | Research needed |

---

## Testing Strategy

### Unit Tests (Per-Crate)
1. **svg crate**: Path normalization, transform math, element parsing
2. **pptx crate**: Each element type → expected DrawingML XML
3. **image crate**: Manifest lifecycle, backend selection
4. **source crate**: Each converter → expected Markdown

### Integration Tests
1. **SVG → PPTX**: Take real SVG pages from templates, convert, validate in PowerPoint
2. **Full pipeline**: Source → Strategist → Executor → PPTX
3. **Quality regression**: Run quality checker on generated SVGs

### Test Fixtures
Port the 17 example projects (229 pages) as test fixtures. Each page is a test case for the SVG→PPTX engine.

---

## Estimated Total Effort

| Phase | Days | Risk |
|-------|------|------|
| Phase 1: SVG→PPTX Engine | 17-25 | High (core complexity) |
| Phase 2: Prompt System | 7 | Low (data porting) |
| Phase 3: Source + Project | 8-12 | Medium (PDF parsing quality) |
| Phase 4: Image System | 9-10 | Low (standard API clients) |
| Testing + Polish | 10-15 | Medium |
| **Total** | **51-69 days** | |

**The SVG→PPTX engine is the critical path. If it works correctly, everything else is straightforward.**

---

## Anti-Patterns to Avoid

1. **Don't skip spec_lock re-read** — This is what makes long decks consistent
2. **Don't parallelize page generation** — Visual continuity requires sequential context
3. **Don't use HTML/CSS as intermediate format** — The SVG→DrawingML mapping is the architectural choice
4. **Don't embed images as bitmaps** — Native DrawingML shapes are the value proposition
5. **Don't auto-fix quality check failures** — They must be re-authored with intent
6. **Don't let AI freely design layouts** — Catalog-based selection > free improvisation
7. **Don't batch user confirmations** — Eight Confirmations must be one interaction
