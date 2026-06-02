# PPT Master — Architecture Deep Dive

## 1. Pipeline Architecture

### Full Pipeline Flow

```
Phase A: Planning
─────────────────
User provides source document
    ↓
source_to_md/*.py → Markdown normalization
    ↓
project_manager.py init → creates project directory
    ↓
project_manager.py import-sources → copies/moves source files
    ↓
[Agent reads source_md/] → Strategist role activated
    ↓
Strategist analyzes content → produces outline
    ↓
Eight Confirmations (one user interaction, 8 decisions bundled)
    ↓
Strategist writes design_spec.md + spec_lock.md
    ↓
[Agent reads spec_lock.md] → Image_Generator role activated (if needed)
    ↓
Image_Generator processes image_prompts.json manifest
    ↓
[Agent reads spec_lock.md] → Executor role activated

Phase B: Generation
────────────────────
Executor generates pages SEQUENTIALLY:
    For each page:
        1. Re-read spec_lock.md (anti-drift)
        2. Generate SVG page in svg_output/
        3. Note visual rhythm for next page

    ↓
svg_quality_checker.py → validates ALL SVG pages (0 errors required)
    ↓
[Optional: verify-charts workflow for data charts]
    ↓
[Optional: visual-review workflow if user requests]

Phase C: Post-Processing
─────────────────────────
total_md_split.py → splits notes/total.md into per-page notes
    ↓
finalize_svg.py → svg_output/ → svg_final/ (icons embedded, images inlined)
    ↓
svg_to_pptx.py → svg_output/ → exports/presentation.pptx (native DrawingML)
    ↓
[Optional: --svg-snapshot → exports/presentation_svg.pptx]
```

### Project Directory Structure

```
projects/<project_name>/
├── source_md/                    # Converted source content (Markdown)
├── images/                       # Image assets
│   ├── image_prompts.json        # Image manifest (lifecycle tracking)
│   ├── *.png / *.jpg / *.webp    # Actual images
│   └── *.meta.json               # Per-image metadata (dimensions, EXIF, etc.)
├── design_spec.md                # Human-readable design specification
├── spec_lock.md                  # Machine-readable execution contract
├── svg_output/                   # Raw SVG pages from Executor
│   ├── page_001.svg
│   ├── page_002.svg
│   └── ...
├── svg_final/                    # Finalized SVG (icons embedded, images inlined)
├── notes/                        # Speaker notes
│   ├── total.md                  # All notes in one file
│   ├── page_001.md               # Per-page notes
│   └── ...
├── animations.json               # Optional animation overrides
├── exports/                      # Output PPTX files
└── backup/                       # Timestamped backups of svg_output/
```

## 2. Source Conversion Pipeline

### PDF → Markdown (`pdf_to_md.py`, 833 lines)
- **Engine**: PyMuPDF (fitz)
- **Heading Detection**: Font-size frequency analysis — most common body font size = body; anything 1.2×+ larger = heading
- **Header/Footer Removal**: Statistical analysis across pages — text blocks that appear on >60% of pages at same Y coordinate = header/footer
- **Image Filtering**: Min 100×100px, min 30K total pixels, aspect ratio < 12:1 (removes dividers, logos)
- **Table Extraction**: Uses PyMuPDF built-in table detection

### DOCX → Markdown (`doc_to_md.py`, 852 lines)
- **Engine**: mammoth for text, direct ZIP reading for image dimensions
- **Image Dimension**: Reads DrawingML XML inside DOCX ZIP to get EMU dimensions → px
- **Dedup**: SHA256 hash on image bytes, skips duplicates
- **Fallback**: openpyxl for embedded spreadsheets

### HTML → Markdown (`web_to_md.py`, partially shared with web fetcher)
- **Engine**: markdownify + BeautifulSoup
- **Lazy-load Detection**: `data-src`, `data-lazy-src`, `data-original` attributes
- **Content Extraction**: Scoring-based algorithm (paragraph density, text-to-code ratio)

### Excel → Markdown (`excel_to_md.py`)
- **Engine**: openpyxl
- **Merged Cells**: Expands into individual cells with consistent formatting
- **Auto-alignment**: Detects number/percentage/date formats

### Web → Markdown (`web_to_md.py`, 806 lines)
- **Engine**: curl_cffi with Chrome 120 JA3 TLS fingerprint
- **Purpose**: Bypasses WeChat and CDN blocks that reject Python's default requests
- **Content Scoring**: Multi-factor scoring (paragraph count, text density, metadata quality)
- **Image Handling**: Detects lazy-load attributes, resolves relative URLs

### PPTX → Markdown (`ppt_to_md.py`, 632 lines)
- **Engine**: python-pptx
- **Shape Processing**: Iterates shapes, extracts text, tables, groups
- **Note Extraction**: Per-slide speaker notes
- **Image Extraction**: Embedded images saved to disk

## 3. Canvas Format System

8 canvas formats, each defined by SVG viewBox dimensions:

| Format | viewBox (px) | Aspect | Primary Use |
|--------|-------------|--------|-------------|
| PPT 16:9 | 0 0 1280 720 | 16:9 | Default presentations |
| PPT 4:3 | 0 0 1024 768 | 4:3 | Traditional projectors |
| Xiaohongshu | 0 0 1242 1660 | 3:4 | Social media posts |
| Square | 0 0 1080 1080 | 1:1 | WeChat/Instagram |
| Story | 0 0 1080 1920 | 9:16 | TikTok/vertical video |
| WeChat Header | 0 0 900 383 | 2.35:1 | Article cover images |
| Landscape | 0 0 1920 1080 | 16:9 | Web banners |
| A4 Print | 0 0 1240 1754 | ~A4 | Print media |

**Key decision**: viewBox in pixels, not EMU. Pixel space makes layout reasoning unambiguous for AI and inspectable in browsers. EMU conversion happens only at PPTX export.

## 4. Template System

### Layout Templates (15+)
Location: `templates/layouts/<name>/`
Each template contains:
- `design_spec.md` — Style guide for this template
- SVG page examples — Visual reference pages
- Icon/color/font choices — Pre-configured

Templates: academic-defense, annual-report, blue-ocean-strategy, business-plan, data-driven, editorial, elegant-dark, executive-brief, gradient-wave, handwritten, minimalist, modern-tech, nature-green, pitch-deck, swot-analysis

### Brand Templates (2)
Location: `templates/brands/<name>/`
- Identity-only (logo, colors, fonts)
- No page roster — layout comes from layout templates or free design
- **Fusion rule**: Brand wins on identity, layout wins on page structure

### Chart Templates (57)
Location: `templates/charts/`
- SVG-based chart templates with labeled axes and data regions
- 71 total when counting sub-variants

### Template Design Philosophy
- **Opt-in, not default**: Default is free design (AI invents visual system from content)
- **No proactive matching**: AI never suggests templates
- **Layouts lock visual idiom**: Charts/icons don't — they're reusable primitives
- **Template = floor AND ceiling**: Locks deck into template's visual idioms

## 5. Quality Assurance Pipeline

### svg_quality_checker.py (1423 lines!)
The largest quality checker. Validates:

**Structural Checks:**
- Banned SVG elements: `<mask>`, `<style>`, `<foreignObject>`, `<symbol>`, `<textPath>`, `<animate>`, `<script>`, `<iframe>`
- Banned attributes: `class`, `@font-face`, `rgba()` (must use `fill-opacity`)
- XML well-formedness: No HTML named entities (`&mdash;` → must use `—`)
- Reserved XML chars: `&`, `<`, `>` must be entity-escaped

**Design Consistency Checks:**
- Color adherence to spec_lock.md palette
- Font family matches spec_lock.md typography
- Canvas dimensions match format
- Icon library matches spec_lock.md choice

**Severity Model:**
- **Errors** → Must fix (Executor re-authors the page)
- **Warnings** → Informational (don't block)
- **No auto-fix** → Intentional: auto-fix would silently lose design intent

### Chart Coordinate Verification (optional workflow)
- Bar heights proportional to data
- Pie sweep angles correct
- Axis tick positions match data range

## 6. Animation System

### Default Animation (built into export)
- **Anchor**: Top-level `<g>` groups in SVG
- **Chrome skip**: Groups named `background/header/footer/decoration/watermark/page_number` excluded
- **Effects**: 20+ entrance effects, semantic mapping from group ID
- **Timing**: `after-previous` chain for sequential reveal

### Custom Animation (optional sidecar)
- `animations.json` — per-slide, per-group overrides
- Keys: slide stem + top-level group ID
- Overrideable: order, effect, delay, duration

### Narration + Video Export
- TTS audio per slide
- Auto-advance = audio clip duration (for video export)
- Object animations must be click-free (after-previous/with-previous)

## 7. Project Lifecycle

### Initialization
```bash
python3 scripts/project_manager.py init <name> --format ppt169
```
Creates directory structure with format-specific config.

### Source Import
```bash
python3 scripts/project_manager.py import-sources <project> <files...> --move
```
**Asymmetric default**: Files outside repo → copied; files inside repo → moved.

### Validation
```bash
python3 scripts/project_manager.py validate <project>
```
Checks directory structure, required files, format consistency.

### Export
```bash
python3 scripts/svg_to_pptx.py <project>
# Optional: --merge-paragraphs for paragraph-level text frames
# Optional: --svg-snapshot for SVG-based preview PPTX
```

### Backup
Default flow always writes `backup/<timestamp>/svg_output/` — archival copy of raw SVG sources.
