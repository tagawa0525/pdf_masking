# pdf_masking

A CLI tool that removes text information from specified PDF pages while
preserving visual appearance. Two processing modes are available:

- **MRC mode** (default): Renders pages as bitmaps and decomposes them into
  three layers (ITU-T T.44 MRC compression)
- **Text-to-outlines mode**: Converts text to vector path outlines directly
  from font data, without rendering. Faster and smaller output. Works with
  embedded fonts or system fonts; falls back to pdfium rendering when fonts
  cannot be resolved.

In both modes, text becomes non-searchable and non-selectable while document
quality is maintained.

## How It Works

Each page is processed through a processing pipeline:

1. **Content analysis** - Analyze PDF structure, extract fonts and image
   XObjects, determine color mode per page
2. **Text-to-outlines** (when enabled) - Convert text (BT...ET blocks) to
   vector paths using glyph outlines from embedded fonts. Skips rendering.
   Non-embedded fonts are resolved via system font lookup (fontdb)
3. **Rendering** (MRC mode only) - Render pages to bitmaps via pdfium at
   specified DPI
4. **MRC composition** (parallel) - Decompose each bitmap into three layers:
   - **Mask**: 1-bit text/line art layer (JBIG2 encoded)
   - **Foreground**: Low-resolution text color (JPEG)
   - **Background**: Full-color image content (JPEG)
5. **PDF assembly** - Build optimized output PDF, subset fonts on masked
   pages, optionally linearize via qpdf

A SHA-256-based cache system skips unchanged pages on subsequent runs.

## Requirements

- System libraries: leptonica, jbig2enc, qpdf, pdfium
- **Linux/macOS**: [Nix](https://nixos.org/) (recommended)
- **Windows**: Visual Studio Build Tools, CMake, Git

## Setup

### Linux / macOS (Nix)

```bash
nix develop    # Enter dev shell with all dependencies
cargo build --release
```

### Windows Setup

A PowerShell setup script automates dependency installation:

```powershell
.\scripts\setup-windows.ps1       # One-time: download and build dependencies
. .\scripts\env-windows.ps1       # Each session: load environment variables
cargo build --release
```

The script performs the following:

1. Installs [leptonica](https://github.com/DanBloomberg/leptonica) via
   [vcpkg](https://github.com/microsoft/vcpkg) (static library,
   `x64-windows-static-md` triplet)
2. Clones and builds [jbig2enc](https://github.com/agl/jbig2enc) from source
   with CMake
3. Downloads prebuilt [pdfium](https://pdfium.nicehash.com/) binaries
4. Generates `scripts/env-windows.ps1` with all required environment variables

qpdf must be installed separately (`winget install qpdf.qpdf`) and is
required by default, since linearization is enabled unless you explicitly set
`linearize: false` in the job/settings config.

**Prerequisites**: Visual Studio 2019+ with C++ workload, CMake 3.20+, Git,
Rust (via rustup).

## Usage

```bash
pdf_masking <jobs.yaml> [<jobs.yaml>...]
```

### Job File

Define processing jobs in YAML:

```yaml
jobs:
  - input: path/to/input.pdf
    output: path/to/output.pdf
    color_mode: rgb
    bw_pages: [1, 3]
    skip_pages: [6]
    text_to_outlines: true
    dpi: 300
    bg_quality: 50
    fg_quality: 30
    preserve_images: true
    linearize: true
```

| Field | Required | Description |
| --- | --- | --- |
| `input` | Yes | Input PDF path |
| `output` | Yes | Output PDF path |
| `color_mode` | No | Default mode: `rgb`, `grayscale`, `bw`, `skip` |
| `bw_pages` | No | Pages to process as black-and-white |
| `grayscale_pages` | No | Pages to process as grayscale MRC |
| `rgb_pages` | No | Pages to process as full-color MRC |
| `skip_pages` | No | Pages to copy without processing |
| `text_to_outlines` | No | Convert to vector outlines (default: false) |
| `dpi` | No | Rendering resolution (default: 300) |
| `bg_quality` | No | Background JPEG quality 1-100 (default: 50) |
| `fg_quality` | No | Foreground JPEG quality 1-100 (default: 30) |
| `preserve_images` | No | Keep original image XObjects (default: true) |
| `linearize` | No | Web-optimize output PDF (default: true) |

Page lists accept single pages (`5`), ranges (`"5-10"`), and mixed
(`[1, 3, "5-10"]`). Pages not listed in any mode-specific list use the
`color_mode` default.

### Settings File

Place a `settings.yaml` in the same directory as the job file to set defaults:

```yaml
color_mode: rgb
dpi: 300
fg_dpi: 100
bg_quality: 50
fg_quality: 30
parallel_workers: 0     # 0 = auto (CPU count)
cache_dir: .cache
preserve_images: true
linearize: true
text_to_outlines: false
```

Job-level values override settings. Missing values use built-in defaults.

## Development

### Linux / macOS

```bash
nix develop
cargo test              # Run all tests
cargo clippy            # Lint
cargo fmt               # Format
```

### Windows Development

```powershell
. .\scripts\env-windows.ps1
cargo test
cargo clippy
cargo fmt
```

## License

This project is not yet licensed.
