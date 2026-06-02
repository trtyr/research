# PPT Master — SVG → PPTX Conversion Engine

## The Most Complex Component

The SVG-to-PPTX conversion engine is the single most complex piece of engineering in PPT Master. It translates SVG (the AI-generated intermediate format) into native PowerPoint DrawingML shapes — making every element clickable, editable, and recolorable in PowerPoint.

### File Map

| File | Lines | Purpose |
|------|-------|---------|
| `svg_to_pptx/drawingml_elements.py` | 1913 | Element-to-element SVG→DrawingML translation |
| `svg_to_pptx/pptx_builder.py` | 845 | PPTX file assembly (relationships, content types, slides) |
| `drawingml_styles.py` | 656 | Fill, stroke, gradient, shadow, glow → DrawingML styles |
| `drawingml_paths.py` | 429 | SVG path commands → DrawingML custGeom |
| `drawingml_utils.py` | 462 | Coordinate conversion, EMU math, color parsing |
| `drawingml_converter.py` | 573 | Top-level orchestrator, element dispatch |
| `svg_to_pptx/pptx_cli.py` | 583 | CLI entry point |
| `svg_to_pptx.py` (root) | — | Pipeline integration |
| `use_expander.py` | — | `<use data-icon>` → inline SVG shapes |
| `tspan_flattener.py` | — | `<tspan>` → independent `<text>` elements |

**Total: ~5,500+ lines of pure conversion logic.**

---

## 1. Fundamental Constants

```
EMU_PER_PX = 9525              # 1 pixel = 9525 EMU (English Metric Units)
FONT_PX_TO_HUNDREDTHS_PT = 75  # Font size conversion
ANGLE_UNIT = 60000             # Degrees to DrawingML angle units (60000 per degree)
```

## 2. SVG → DrawingML Element Mapping

### Rectangle: `<rect>` → `<p:sp>`
- `rx` attribute present → `<a:prstGeom prst="roundRect">` (preset geometry)
- No `rx` → `<a:prstGeom prst="rect">`
- Width/height from attributes → EMU conversion
- Fill and stroke from presentation attributes

### Rounded Rectangle: `<rect rx="...">` → `<a:prstGeom prst="roundRect">`
- `rx` value mapped to `<a:avLst><a:gd name="adj" fmla="val {N}"/>`
- Adjustment value formula: percentage of effective radius

### Circle/Ellipse: `<circle>` / `<ellipse>` → `<p:sp>` with `<a:prstGeom prst="ellipse">`
- Special case: `stroke-dasharray` detection → donut arc (polar chart segments)
- Donut arc: `<a:prstGeom prst="arc">` with start/end angle adjustments

### Line: `<line>` or `<path>` with single M-L → `<p:sp>` (connector-like)
- Start point and end point mapped to `<a:cxnSp>` or custom geometry

### Path: `<path d="...">` → `<a:custGeom>` (Custom Geometry)
This is the most complex translation. SVG path commands must be normalized:

**Normalization Pipeline:**
```
SVG Path → Normalize → DrawingML custGeom
  S → C (smooth cubic → explicit cubic)
  Q → C (quadratic → cubic approximation)
  T → C (smooth quadratic → explicit cubic)
  A → C (arc → cubic Bézier, using SVG spec F.6.5 algorithm)
  Result: only M, L, C, Z commands remain
```

**DrawingML custGeom structure:**
```xml
<a:custGeom>
  <a:pathLst>
    <a:path w="..." h="...">
      <a:moveTo><a:pt x="..." y="..."/></a:moveTo>
      <a:lnTo><a:pt x="..." y="..."/></a:lnTo>
      <a:cubicBezTo>
        <a:pt x="..." y="..."/>  <!-- control point 1 -->
        <a:pt x="..." y="..."/>  <!-- control point 2 -->
        <a:pt x="..." y="..."/>  <!-- end point -->
      </a:cubicBezTo>
      <a:close/>
    </a:path>
  </a:pathLst>
</a:custGeom>
```

### Text: `<text>` → `<p:sp>` (TextBox)
- Container becomes a text frame shape
- **tspan flattening**: DrawingML text runs cannot reposition mid-paragraph
  - `<tspan x="..." dy="...">` → Each tspan becomes independent `<text>` element
  - This is the critical preprocessing step (tspan_flattener.py)
- Font family, size, weight, style → `<a:rPr>` properties
- Text content → `<a:t>` inside `<a:r>` inside `<a:p>`
- `text-anchor` → paragraph alignment (`<a:pPr algn="ctr/l/r">`)
- `dominant-baseline` → baseline offset

### Image: `<image>` → `<p:pic>` (Picture)
- SVG `href` → bitmap extraction (Base64 decode or file read)
- Bitmap copied to PPTX `ppt/media/imageN.ext`
- `<a:srcRect>` for `preserveAspectRatio` cropping
- `preserveAspectRatio="xMidYMid slice"` → `<a:srcRect t="..." b="..." l="..." r="..."/>`
- Size and position from SVG attributes → EMU

### Group: `<g>` → `<p:grpSp>` (Group Shape)
- Recursively processes children
- Group transform → `<p:grpSpPr><a:xfrm>`
- Children positions relative to group

## 3. Transform Handling (ConvertContext)

SVG transforms are accumulated through a `ConvertContext` object:

```python
class ConvertContext:
    # Accumulated state during tree traversal
    transform_stack: list[Transform]  # translate, scale, rotate, matrix
    fill: str | None                  # Current fill
    stroke: str | None                # Current stroke
    stroke_width: float               # Current stroke width
    font_family: str | None           # Current font
    font_size: float | None           # Current font size
    opacity: float                    # Current opacity
```

**Transform types supported:**
- `translate(tx, ty)` → Offset position
- `scale(sx, sy)` → Scale dimensions
- `rotate(angle, cx, cy)` → Rotate around center
- `matrix(a, b, c, d, e, f)` → Full 2D affine matrix

**Matrix composition:** Transforms are composed as 3×3 affine matrices and applied to child coordinates.

## 4. Style Translation

### Fill Translation
| SVG Fill | DrawingML |
|----------|-----------|
| `fill="#hex"` | `<a:solidFill><a:srgbClr val="hex"/></a:solidFill>` |
| `fill="url(#grad)"` linearGradient | `<a:gradFill>` with gradient stops |
| `fill="url(#grad)"` radialGradient | `<a:gradFill><a:path path="circle">` |
| `fill="none"` | `<a:noFill/>` |
| `fill-opacity="0.5"` | `<a:srgbClr val="hex"><a:alpha val="50000"/></a:srgbClr>` |

### Gradient Details
- **Linear**: Angle derived from gradient vector `(x1,y1)→(x2,y2)`
  - `<a:lin ang="{angle_degrees * 60000}" scaled="1"/>`
- **Radial**: Center point mapped to `<a:path><a:fillToRect l="..." t="..." r="..." b="..."/>`
- **Stops**: `<a:gs pos="{percentage * 1000}">` with color at each position

### Stroke Translation
| SVG Stroke | DrawingML |
|------------|-----------|
| `stroke="#hex"` | `<a:ln><a:solidFill><a:srgbClr val="hex"/>` |
| `stroke-width="2"` | `<a:ln w="{2 * 9525}">` |
| `stroke-dasharray="5,3"` | `<a:prstDash prst="dash"/>` |
| `stroke-linecap="round"` | `<a:ln cap="rnd">` |
| `stroke-linejoin="round"` | `<a:ln join="rnd">` (via `<a:round>)` |

### Shadow Translation
- `feGaussianBlur stdDeviation` → `blurRad` in EMU
  - Formula: `blurRad = stdDeviation * 2 * EMU_PER_PX * some_factor`
- `feDropShadow dx, dy` → offset in EMU
- Shadow opacity → `<a:alpha val="{opacity * 1000 * 0.75}"/>` (0.75× factor applied)
- Shadow color → `<a:srgbClr val="hex">`

### Glow Translation
- `feGaussianBlur stdDeviation` on glow source → `<a:glow rad="{...}">`
- Glow color + opacity → same pattern as shadow

## 5. Icon Expansion (use_expander.py)

SVG pages reference icons via `<use data-icon="lib/name"/>` shorthand. The expander:

1. Parses `data-icon` attribute: `lib` = icon library name, `name` = icon name
2. Looks up icon in template icon catalogs (5 libraries, 11600+ icons)
3. Reads the icon's SVG definition
4. Inlines the icon's shapes into the page SVG at the specified position
5. Applies size and color from the parent element

**Supported icon libraries:**
- Lucide (default)
- Phosphor
- Tabler Icons
- Bootstrap Icons
- Remix Icon

**Why expansion is needed**: DrawingML has no concept of `<use>` references. Without expansion, icons silently disappear in the PPTX output.

## 6. Text Flattening (tspan_flattener.py)

The most critical preprocessing step for text rendering.

**Problem**: DrawingML text runs cannot be repositioned mid-paragraph. SVG `<tspan x="..." dy="...">` positions each text segment independently, but PowerPoint's `<a:r>` elements flow inline.

**Solution**: Before conversion, every `<text>` element with repositioning `<tspan>` children is split into independent `<text>` elements:

```xml
<!-- Before flattening -->
<text x="100" y="50">
  <tspan x="100" dy="0">Line 1</tspan>
  <tspan x="100" dy="30">Line 2</tspan>
  <tspan x="100" dy="30">Line 3</tspan>
</text>

<!-- After flattening -->
<text x="100" y="50">Line 1</text>
<text x="100" y="80">Line 2</text>
<text x="100" y="110">Line 3</text>
```

This makes each line a separate shape in the PPTX — editable individually but positioned correctly.

## 7. Image Embedding Strategy

**Two divergent strategies for two products:**

| Product | Strategy | Why |
|---------|----------|-----|
| `svg_final/` (IDE preview) | Base64 inline | Self-contained SVGs, preview anywhere |
| Native PPTX | File copy to media/ | PowerPoint's native idiom, `<a:srcRect>` crop support |

### Cropping (`preserveAspectRatio` handling)
When `preserveAspectRatio="xMidYMid slice"` (cover mode):
1. Calculate source rectangle that covers the viewport
2. Express as `<a:srcRect t="..." b="..." l="..." r="..."/>` in percentage
3. PowerPoint applies the crop natively

## 8. PPTX Assembly (pptx_builder.py)

### File Structure
```
presentation.pptx (ZIP archive)
├── [Content_Types].xml
├── _rels/.rels
├── ppt/
│   ├── presentation.xml
│   ├── _rels/presentation.xml.rels
│   ├── slideLayouts/
│   ├── slideMasters/
│   ├── slides/
│   │   ├── slide1.xml
│   │   ├── slide2.xml
│   │   └── ...
│   ├── media/
│   │   ├── image1.png
│   │   └── ...
│   └── theme/
```

### Slide XML Structure
```xml
<p:sld>
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr>...</p:nvGrpSpPr>
      <p:grpSpPr>
        <a:xfrm>
          <a:off x="0" y="0"/>
          <a:ext cx="12192000" cy="6858000"/>  <!-- EMU -->
        </a:xfrm>
      </p:grpSpPr>
      <!-- Shapes, groups, pictures here -->
    </p:spTree>
  </p:cSld>
</p:sld>
```

### Office Compatibility Mode
- **On by default** (PowerPoint < 2019 can't render SVG)
- Generates per-slide PNG fallback alongside native shapes
- Newer Office: editable shapes shown
- Older Office: PNG fallback displayed
- Escape hatch: `--no-compat` for modern-only output

## 9. PPTX → SVG (Reverse: pptx_to_svg/)

Also exists for template import:
- `slide_to_svg.py` (914 lines) — Full slide → SVG
- `txbody_to_svg.py` (1003 lines) — Text body → SVG text
- `prstgeom_to_svg.py` (670 lines) — Preset geometries → SVG paths

This enables importing existing PPTX templates and converting them to SVG for editing.

---

## Conversion Complexity Summary

The SVG→PPTX engine is effectively a **miniature rendering engine** with:
- 19 element types dispatched
- Path normalization (5 SVG commands → cubic Bézier)
- Transform matrix accumulation
- Gradient interpolation
- Shadow/glow physics
- Text layout reflow
- Image cropping mathematics
- Icon catalog lookup + expansion
- XML serialization with proper namespaces

This is ~5,500 lines of intricate coordinate math and format translation. It's the component that makes the output "native PowerPoint" instead of "embedded images."
