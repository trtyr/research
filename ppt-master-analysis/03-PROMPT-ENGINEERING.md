# PPT Master — Prompt Engineering & Role System

## 1. Three-Role System Architecture

PPT Master uses a **single agent with role switching**, NOT parallel sub-agents. This is a critical design choice with three connected reasons:

### Why Single Agent
1. **Context dependency**: Page design depends on full upstream context — Strategist's color choices, actually-acquired images, prior pages' visual rhythm
2. **Sub-agents get stale snapshots**: They'd start with partial context and produce visually drifting decks
3. **Batching accelerates drift**: Generating 5 pages per turn compresses context faster, degrading visual consistency

### Role Switching Protocol
```
[Agent reads references/<role>.md] → Role activated → Task performed → Role completed
```

The mandated read of role-specific reference files serves TWO purposes:
1. Forces fresh role instructions into context, overriding drift from previous mode
2. Creates visible marker in conversation transcript — audit trail for debugging

---

## 2. Role 1: Strategist

**Reference**: `references/strategist.md` (708 lines)

### Responsibilities
1. Analyze source content
2. Plan slide structure and outline
3. Define visual system (colors, fonts, icon library)
4. Run Eight Confirmations with user
5. Produce `design_spec.md` + `spec_lock.md`

### Eight Confirmations (The Blocking Gate)

Bundled into ONE user interaction:

| # | Decision | What's Confirmed |
|---|----------|-----------------|
| 1 | Canvas | Format (16:9, 4:3, square, etc.) |
| 2 | Page Count | Total pages + outline |
| 3 | Audience | Target audience (technical, executive, academic) |
| 4 | Style | Visual style (minimalist, editorial, gradient, etc.) |
| 5 | Color | Primary palette (HEX values) |
| 6 | Icon | Icon library choice (Lucide, Phosphor, etc.) |
| 7 | Typography | Font family + hierarchy |
| 8 | Image | AI image rendering + palette lock (3D decision) |

**Why bundled**: Design choices are correlated (color affects icon library affects typography). Resolving together produces coherent decisions; spreading confirmations would invite contradictions.

### spec_lock.md (The Anti-Drift Mechanism)

The #1 quality differentiator. Two files serve different masters:

| File | Master | Contents |
|------|--------|----------|
| `design_spec.md` | Human | Narrative: "why" of design (audience, style objective, color rationale, page outline) |
| `spec_lock.md` | Machine | Contract: "what" Executor must literally use (HEX colors, exact font string, icon library, image resources with status) |

**spec_lock.md is re-read before EVERY page.** This prevents LLM context-compression drift — without it, colors and fonts gradually mutate mid-deck on 20+ slide presentations.

**Example spec_lock structure:**
```yaml
canvas:
  format: ppt169
  width: 1280
  height: 720

colors:
  primary: "#2563EB"
  secondary: "#7C3AED"
  accent: "#F59E0B"
  background: "#FFFFFF"
  text: "#1F2937"
  muted: "#6B7280"

typography:
  font_family: "Inter"
  heading_size: 36
  body_size: 16
  caption_size: 12

icon_library: lucide

images:
  rendering: vector-illustration
  palette: warm-professional
  resources:
    - id: hero_1
      prompt: "..."
      status: pending  # → generated | failed | needs-manual
      layout: "#5"     # references image layout pattern catalog

page_outline:
  - page: 1
    type: cover
    rhythm: anchor
  - page: 2
    type: content
    rhythm: dense
    layout_note: "Key statistics with icons"
```

### Image Resource List (Section VIII of design_spec)

Each image entry in spec_lock declares:
- **Prompt**: For AI generation
- **Status lifecycle**: `pending → generated | failed | needs-manual`
- **Layout pattern**: `#<id> + #<id>` expression (Primary + Modifiers)
- **Type**: background / hero / framework / comparison / etc.

---

## 3. Role 2: Image_Generator

**Reference**: `references/image-generator.md` (445 lines)

### Responsibilities
1. Process image_prompts.json manifest
2. Generate AI images with deck-wide consistency
3. Search web images when AI generation not suitable
4. Manage candidate pools and fallbacks

### 3D Image Lock (Deck-Wide Consistency)

Three orthogonal dimensions locked at Strategist time:

| Dimension | Scope | Options | Example |
|-----------|-------|---------|---------|
| **Rendering** | Deck-wide | 16 types | vector-illustration, editorial, 3d-isometric, sketch-notes, watercolor... |
| **Palette** | Deck-wide | 10 types | How deck's HEX values are used: proportion + role + temperament |
| **Type** | Per-image | 12+ types | background, hero, framework, comparison, process, infographic... |

**Each per-image prompt = locked rendering + locked palette + per-image type**

Without this, every image gets its own style drift and the deck reads as a stack of unrelated illustrations.

### Rendering Types (16)
Defined in `references/ai-image-comparison/rendering/_manifest.md` (371 lines)
Examples: vector-illustration, editorial, 3d-isometric, sketch-notes, watercolor, flat-design, paper-cutout, low-poly, etc.

### Palette Types (10)
Defined in `references/ai-image-comparison/palette/_manifest.md` (262 lines)
Controls how the deck's HEX values map to image color usage.

### Type Templates (12+)
Defined in `references/ai-image-comparison/type/_manifest.md` (281 lines)
Controls internal composition per image purpose.

---

## 4. Role 3: Executor

**References**:
- `references/executor-base.md` (413 lines) — Shared rules
- `references/executor-general.md` (109 lines) — General variant
- `references/executor-consultant.md` (187 lines) — Consultant variant
- `references/executor-consultant-top.md` (199 lines) — Consultant-Top variant

### Three Variants

| Variant | When | Communication Mode |
|---------|------|--------------------|
| General | Default | Standard informative |
| Consultant | B2B/professional services | Data-driven, structured |
| Consultant-Top | Executive summaries | Minimal text, maximum impact |

### Per-Page Generation Protocol

```
For each page:
    1. Re-read spec_lock.md → exact HEX colors, font, icon library
    2. Consider page_rhythm of current page:
       - anchor: focal point, breathing room, visual impact
       - dense: information-rich, organized grid/flow
       - breathing: visual rest, transition, emotional beat
    3. Apply appropriate image layout pattern from catalog
    4. Generate SVG in svg_output/page_NNN.svg
    5. Note visual rhythm for continuity on next page
```

### page_rhythm System

The anti-"AI card grid" mechanism. Each page is classified:

| Rhythm | Visual Character | When Used |
|--------|-----------------|-----------|
| **anchor** | Focal point, breathing room, visual impact | Cover, section dividers, key takeaways |
| **dense** | Information-rich, organized layout | Data slides, multi-point content, comparisons |
| **breathing** | Visual rest, transition, whitespace-heavy | Between dense sections, emotional beats |

**Rhythm prevents uniformity**: Without it, AI defaults to same-density card layouts for every page.

### SVG Constraints (What Executor Must Follow)

**Banned** (no DrawingML equivalent):
- `<mask>` → Use gradient overlays, clipPath, filter shadow, or bake-in
- `<style>` / `class` → All styles must be inline attributes
- `@font-face` → Use web-safe or system fonts only
- `<foreignObject>` → No HTML embedding
- `<symbol>` + `<use>` (except icon `<use data-icon="...">`)
- `<textPath>` → No text on path
- `<animate*>` → No SVG animations (PPTX animations are separate)
- `<script>` / `<iframe>` → No interactivity
- `rgba()` → Use `fill-opacity` / `stroke-opacity` instead

**Conditional**:
- `marker-start` / `marker-end` → Only simple arrow markers
- `clip-path` → Only on `<image>` elements (not on shapes)

**XML Rules**:
- Typography: Raw Unicode (`—`, `→`, `©`, NBSP) — HTML entities are XML-illegal
- Reserved chars: `& < >` must be entity-escaped (e.g., `R&amp;D` not `R&D`)

### Image Layout Patterns (72 techniques)

Defined in `references/image-layout-patterns.md` (222 lines)

**Two-layer taxonomy:**

1. **Primary Structures** (container layouts / image-as-canvas / multi-image)
   - The page's structural bones
   - One or more per page
   - Examples: `#2 left-third`, `#5 hero-center`, `#38 image-as-canvas annotation`

2. **Modifier Layers** (non-rectangular clips / overlays / texture / effects)
   - Visual finish applied on top
   - Any number per page
   - Examples: `#53 circle-clip`, `#61 duotone-overlay`, `#68 paper-texture`

**Composition rule**: Multiple Primaries + Multiple Modifiers = valid. Single-Primary-no-Modifier needs justification.

**Why the catalog exists**: Without it, AI defaults every image page to bare `#2 left-third` or `#48 side-by-side` — visually flat, "AI-default" layouts.

---

## 5. Prompt Engineering Patterns

### Pattern: Catalog-Based Selection
Instead of letting AI freely design, constrain choices to predefined catalogs:
- 71 chart templates → AI picks, doesn't invent
- 11600+ icons → AI selects from library, doesn't draw
- 72 image layouts → AI composes from catalog, doesn't improvise
- 16 rendering types → AI locks deck-wide, doesn't per-image decide

**Benefit**: Constrains AI creativity while enabling quality. AI picks from professionally-designed options instead of generating mediocre novel ones.

### Pattern: Sequential Generation with Memory
Each page generation has access to:
1. spec_lock.md (read fresh every time)
2. Previous page's visual rhythm
3. Design spec narrative
4. Actually-acquired image resources

This creates visual continuity that batched/parallel generation cannot achieve.

### Pattern: Anti-Drift via Re-reading
spec_lock.md is the "source of truth" that prevents:
- Color mutation (HEX values stay exact)
- Font drift (font family string stays verbatim)
- Icon library switching
- Image style inconsistency

### Pattern: Bundled Decisions
Eight Confirmations bundles all user decisions into one interaction:
- Reduces decision fatigue
- Ensures decisions are mutually consistent
- After confirmation, pipeline runs autonomously

### Pattern: Severity-Graded Quality Gates
- Errors block (must re-author)
- Warnings inform (don't block)
- No auto-fix (preserves design intent)

### Prompt File Sizes (for reference)

| Reference File | Lines | Purpose |
|----------------|-------|---------|
| shared-standards.md | 736 | SVG constraints, banned features, substitute patterns |
| strategist.md | 708 | Strategist role definition |
| image-generator.md | 445 | Image generation role |
| executor-base.md | 413 | Base executor rules |
| template-designer.md | 386 | Template design guidelines |
| rendering manifest | 371 | 16 rendering type descriptions |
| type manifest | 281 | Image type templates |
| image-searcher.md | 266 | Web image search rules |
| palette manifest | 262 | 10 palette type descriptions |
| visual-review.md | 240 | Visual review rubric |
| image-layout-spec.md | 235 | Layout specification details |
| image-layout-patterns.md | 222 | 72 layout patterns catalog |

**Total prompt engineering: ~16,588 lines of Markdown** — this is the "secret sauce" that makes the output quality high. The prompts are meticulously designed to constrain AI behavior while preserving design flexibility.
