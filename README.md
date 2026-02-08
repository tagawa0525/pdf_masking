# pdf_masking

A CLI tool that removes text information from specified PDF pages while
preserving visual appearance through MRC (Mixed Raster Content) compression.

Targeted pages are rendered as bitmaps and decomposed into three layers
(ITU-T T.44), making text non-searchable and non-selectable while maintaining
document quality.

## How It Works

Each page is processed through a 4-phase pipeline:

1. **Content analysis** - Analyze PDF structure and detect image XObjects
2. **Rendering** - Render pages to bitmaps via pdfium at specified DPI
3. **MRC composition** (parallel) - Decompose each bitmap into three layers:
   - **Mask**: 1-bit text/line art layer (JBIG2 encoded)
   - **Foreground**: Low-resolution text color (JPEG)
   - **Background**: Full-color image content (JPEG)
4. **PDF assembly** - Build optimized output PDF with MRC XObjects

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
    pages: [1, 3, "5-10", 15]
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
| `pages` | Yes | Pages to mask (1-based). Single: `5`, range: `"5-10"`, |
| | | mixed: `[1, 3, "5-10"]` |
| `dpi` | No | Rendering resolution (default: 300) |
| `bg_quality` | No | Background JPEG quality 1-100 (default: 50) |
| `fg_quality` | No | Foreground JPEG quality 1-100 (default: 30) |
| `preserve_images` | No | Keep original image XObjects (default: true) |
| `linearize` | No | Web-optimize output PDF (default: true) |

### Settings File

Place a `settings.yaml` in the same directory as the job file to set defaults:

```yaml
dpi: 300
fg_dpi: 100
bg_quality: 50
fg_quality: 30
parallel_workers: 0     # 0 = auto (CPU count)
cache_dir: .cache
preserve_images: true
linearize: true
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
