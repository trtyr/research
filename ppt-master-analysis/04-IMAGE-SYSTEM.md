# PPT Master — Image Acquisition System

## 1. Architecture Overview

The image system handles two distinct image acquisition paths:
1. **AI Image Generation** — 13 backends for creating images from text prompts
2. **Web Image Search** — 4 providers for finding existing licensed images

Both paths share a common manifest-based lifecycle system.

```
spec_lock.md (image resource list)
    ↓
image_prompts.json (manifest)
    ↓
[AI Generation OR Web Search]
    ↓
images/ directory (actual files + metadata)
    ↓
SVG pages reference images
    ↓
Post-processing embeds/inlines images
```

---

## 2. AI Image Generation Backends (13)

### Registry Pattern
All backends follow a common interface, dynamically imported:

```python
# Dynamic import pattern
backend_module = importlib.import_module(f"image_backends.{backend_name}")
generator = backend_module.ImageGenerator(config)
```

### Backend List

| Backend | Provider | Key Config | Notes |
|---------|----------|------------|-------|
| `openai` | OpenAI DALL·E | `OPENAI_API_KEY` | Default for many use cases |
| `gemini` | Google Gemini | `GEMINI_API_KEY` | |
| `qwen` | Alibaba Qwen | `DASHSCOPE_API_KEY` | Chinese market |
| `zhipu` | Zhipu AI (CogView) | `ZHIPU_API_KEY` | Chinese market |
| `volcengine` | ByteDance Volcengine | `VOLCENGINE_API_KEY` | Chinese market |
| `stability` | Stability AI | `STABILITY_API_KEY` | |
| `bfl` | Black Forest Labs (FLUX) | `BFL_API_KEY` | |
| `ideogram` | Ideogram | `IDEOGRAM_API_KEY` | |
| `siliconflow` | SiliconFlow | `SILICONFLOW_API_KEY` | |
| `fal` | fal.ai | `FAL_API_KEY` | |
| `replicate` | Replicate | `REPLICATE_API_TOKEN` | |
| `openrouter` | OpenRouter | `OPENROUTER_API_KEY` | Aggregator |
| `modelscope` | ModelScope | `MODELSCOPE_API_KEY` | Chinese market |

### Backend Selection
- `IMAGE_BACKEND=<name>` selects the active backend
- Per-provider API keys (NOT a generic `IMAGE_API_KEY`)
- Reason: Multiple configured providers → "which is active?" must be config-readable

---

## 3. Web Image Search Providers (4)

| Provider | License Tier | Config | Notes |
|----------|-------------|--------|-------|
| Pexels | No attribution (CC0-like) | `PEXELS_API_KEY` | Default choice |
| Pixabay | No attribution | `PIXABAY_API_KEY` | |
| Openverse | Attribution required | No key needed | CC-licensed content |
| Wikimedia | Mixed licenses | No key needed | Wikipedia Commons |

### License Filtering

**Two tiers:**
1. **Default (permissive)**: CC BY / CC BY-SA allowed with inline attribution
2. **Strict** (`--strict-no-attribution`): Only no-attribution licenses

**Auto-rejected**: CC BY-NC* (non-commercial) and CC BY-ND* (no-derivatives)
- Reason: PPT output is typically shared commercially and modified

---

## 4. Manifest System (image_prompts.json)

### Lifecycle

```
Pending → Generated ✓
        → Failed ✗ → Retry with fallback
                   → Needs-Manual (exhausted retries)
```

### Manifest Entry Structure
```json
{
  "id": "hero_1",
  "prompt": "A professional business team collaborating in modern office...",
  "type": "hero",
  "status": "pending",
  "rendering": "vector-illustration",
  "palette": "warm-professional",
  "layout": "#5",
  "candidates": [],
  "selected": null,
  "attempts": 0,
  "last_error": null
}
```

### Atomic Writes
```python
# Crash recovery pattern
tmp = tempfile.NamedTemporaryFile(delete=False, suffix=".json")
json.dump(data, tmp)
tmp.close()
os.replace(tmp.name, manifest_path)  # Atomic on POSIX
```

---

## 5. Concurrency & Reliability

### Adaptive Concurrency
```python
# ThreadPoolExecutor with backoff
max_workers = initial_concurrency  # e.g., 4

def on_rate_limit():
    global max_workers
    max_workers = max(1, max_workers // 2)  # Halve, minimum 1
```

### Retry Strategy
- Per-image retry with exponential backoff
- Fallback to alternative backends on persistent failure
- Final fallback: mark as `Needs-Manual` (user provides image)

### Candidate Pool
For web image search:
1. Top-N alternatives stored per image slot
2. If selected image fails quality check → promote next candidate
3. Quality check: dimensions, aspect ratio, visual quality heuristic

---

## 6. Query Optimization

### Progressive Query Simplification
When image search returns no results:
```
Full query: "professional business team collaboration meeting"
→ 4 words: "business team collaboration"
→ 3 words: "team collaboration"
→ 2 words: "collaboration"
→ 1 word: "team"
```

### Prompt Assembly for AI Generation
Each per-image prompt is assembled from locked dimensions:
```
Final prompt = f"{rendering_style}, {palette_description}, {type_composition}: {original_prompt}"
```

This ensures deck-wide visual consistency across all generated images.

---

## 7. Image Processing Pipeline

### analyze_images.py (584 lines)
Extracts metadata from user-provided images:
- Dimensions (width × height)
- EXIF orientation
- Dominant color (histogram analysis)
- Subject detection (basic)

**Why metadata, not pixels**: LLM doesn't need pixel data for layout decisions. It needs:
- Aspect ratio → for placement
- Color tone → for palette compatibility
- Subject → for slide assignment

### rotate_images.py (588 lines)
- Auto-rotates images based on EXIF orientation
- Normalizes all images for consistent processing

### Image Embedding Strategies

| Stage | Format | Purpose |
|-------|--------|---------|
| `svg_output/` | External file reference | Fast iteration, easy replacement |
| `svg_final/` | Base64 inline | Self-contained preview SVGs |
| Native PPTX | File in media/ + `<a:srcRect>` | PowerPoint-native, minimal file size |

---

## 8. Image Layout Pattern Catalog (72 Patterns)

### Primary Structures (Container Layouts)
1. Full-bleed hero
2. Left-third with text overlay
3. Right-third with text overlay
4. Centered with border frame
5. Hero center with floating text
...

### Primary Structures (Image-as-Canvas + Native Overlay)
31-40. Image as background with overlaid cards, annotations, callouts, etc.

### Primary Structures (Multi-Image)
41-50. Grid layouts, mosaics, comparison side-by-side, timeline strips

### Modifier Layers
51-60. Non-rectangular clips (circle, hexagon, diamond)
61-68. Overlays & masks (duotone, gradient overlay, color wash)
69-72. Texture & effects (paper texture, grain, glass morphism)

### Composition Rule
```
Page layout = 1+ Primary Structure(s) + 0+ Modifier Layer(s)
```

The catalog is physically organized: all Primaries first, then all Modifiers. This helps the AI internalize the two-layer mental model from the table of contents alone.

---

## 9. SVG Image Embedding Reference

**Reference**: `references/svg-image-embedding.md` (184 lines)

### Technical Constraints
- SVG `<image>` href → local file path or Base64 data URI
- `preserveAspectRatio` must translate to `<a:srcRect>` in PPTX
- `<clipPath>` only allowed on `<image>` elements (not shapes)
- `fill-opacity` instead of `rgba()` for DrawingML compatibility

### Aspect Ratio Handling
```
preserveAspectRatio="xMidYMid meet"  → No crop needed
preserveAspectRatio="xMidYMid slice" → Crop: calculate srcRect
preserveAspectRatio="none"           → Stretch: no srcRect
```
