# `mrc` feature flag導入: Windows向けC/C++外部依存のオプショナル化

## Context

Windows環境でのビルドには vcpkg経由の leptonica インストール、jbig2enc の
CMakeソースビルド、pdfium プリビルトバイナリのダウンロード、MSVC C++
ツールチェーンが必要で、セットアップが煩雑。

text_to_outlinesパス（Phase A2）はこれらC/C++依存を一切使わず、純粋Rustクレート
（ttf-parser, fontdb）のみで動作する。このパスだけで十分なケース
（フォント埋め込みPDF）では、C/C++依存を完全に不要にできる。

`mrc` feature flagを導入し、MRC処理（Phase B+C）に必要なC/C++依存を
オプショナルにする。`default = ["mrc"]`で既存ユーザーに影響なし。
`--no-default-features`でtext_to_outlinesのみのビルドが可能になる。

## 変更概要

### 1. Cargo.toml — feature定義と依存のoptional化

```toml
[features]
default = ["mrc"]
mrc = ["dep:pdfium-render", "dep:leptonica-sys", "dep:libc", "dep:cc"]

[dependencies]
pdfium-render = { version = "0.8.37", optional = true }
leptonica-sys = { version = "0.4.9", optional = true }
libc = { version = "0.2.181", optional = true }
# image は image_xobject.rs で text_to_outlines パスから使用されるため常時有効
image = "0.25.9"

[build-dependencies]
cc = { version = "1.2.55", optional = true }
```

### 2. build.rs — jbig2encシムのコンパイルを条件化

- `#[cfg(feature = "mrc")]` で全体をラップ
- mrc無効時はbuild.rsのmain()が空になる

### 3. src/lib.rs — モジュール宣言のゲート

```rust
#[cfg(feature = "mrc")]
pub mod ffi;
pub mod mrc;  // mrc自体は常時有効（compositor, jpegが両パスで使われるため）
#[cfg(feature = "mrc")]
pub mod render;
```

### 4. src/mrc/mod.rs — サブモジュールとデータ型のゲート

```rust
pub mod compositor;
#[cfg(feature = "mrc")]
pub mod jbig2;
pub mod jpeg;  // image_xobject.rs → encode_image() から使用
#[cfg(feature = "mrc")]
pub mod segmenter;
```

データ型：

- `MrcLayers`, `BwLayers`: `#[cfg(feature = "mrc")]`
- `PageOutput::Mrc`, `PageOutput::BwMask` バリアント: `#[cfg(feature = "mrc")]`
- `SkipData`, `TextRegionCrop`, `ImageModification`, `TextMaskedData`,
  `PageOutput::Skip`, `PageOutput::TextMasked`: 常時有効

### 5. src/mrc/compositor.rs — MRC関数のゲート

import分離:

```rust
// 常時有効
use super::{ImageModification, TextMaskedData, TextRegionCrop, jpeg};
// MRC専用
#[cfg(feature = "mrc")]
use super::{BwLayers, MrcLayers, jbig2, segmenter};
#[cfg(feature = "mrc")]
use crate::mrc::segmenter::PixelBBox;
#[cfg(feature = "mrc")]
use crate::pdf::content_stream::pixel_to_page_coords;
#[cfg(feature = "mrc")]
use image::{DynamicImage, RgbaImage};
```

関数ゲート:

- **常時有効:** `MrcConfig`, `TextOutlinesParams`, `compose_text_outlines()`,
  `detect_and_redact_images()`
- **`#[cfg(feature = "mrc")]`:** `compose()`, `compose_bw()`,
  `TextMaskedParams`, `compose_text_masked()`, `crop_text_regions_jbig2()`

### 6. src/pdf/content_stream.rs — pixel_to_page_coordsのゲート

`pixel_to_page_coords()` は `segmenter::PixelBBox` を参照するため
`#[cfg(feature = "mrc")]` でゲート。他の関数は変更なし。

### 7. src/pdf/image_xobject.rs — optimize_image_encodingのゲート

```rust
#[cfg(feature = "mrc")]
use crate::mrc::jbig2;
use crate::mrc::jpeg;  // 常時有効（encode_imageから使用）
```

`optimize_image_encoding()` と関連テストを `#[cfg(feature = "mrc")]`
でゲート。`redact_image_regions()`, `bbox_overlaps()`,
`decode_image_stream()`, `fill_white()`, `encode_image()` は変更なし。

### 8. src/pdf/writer.rs — MRC/BWライターメソッドのゲート

```rust
#[cfg(feature = "mrc")]
use crate::mrc::{BwLayers, MrcLayers};
```

- `write_mrc_page()`, `write_bw_page()`: `#[cfg(feature = "mrc")]`
- `write_text_masked_page()`, `copy_page_from()`, `save_to_bytes()`, `new()`: 変更なし

### 9. src/error.rs — PdfiumError変換のゲート

```rust
#[cfg(feature = "mrc")]
impl From<pdfium_render::prelude::PdfiumError> for PdfMaskError {
    fn from(e: pdfium_render::prelude::PdfiumError) -> Self {
        Self::RenderError(e.to_string())
    }
}
```

エラーバリアント定義自体(`RenderError`, `SegmentationError`, `Jbig2EncodeError`)はString型のラッパーなので変更不要。

### 10. src/pipeline/job_runner.rs — MRCフォールバックの条件分岐

import:

```rust
#[cfg(feature = "mrc")]
use crate::mrc::compositor::MrcConfig;
#[cfg(feature = "mrc")]
use crate::pipeline::page_processor::ProcessPageParams;
#[cfg(feature = "mrc")]
use crate::render::pdfium::render_page;
```

型:

- `RenderResult`: `#[cfg(feature = "mrc")]`

`run_job()` 内でPhase B+Cを条件分岐:

```rust
#[cfg(feature = "mrc")]
let successful_pages = phase_bc_render_and_mrc(...)?;

#[cfg(not(feature = "mrc"))]
let successful_pages = {
    if !needs_rendering.is_empty() {
        let pages: Vec<u32> = needs_rendering.iter().map(|a| a.page_idx + 1).collect();
        return Err(PdfMaskError::render(format!(
            "text-to-outlines conversion failed for page(s) {:?} and \
             MRC fallback is unavailable (compiled without 'mrc' feature). \
             Ensure fonts are embedded in the source PDF, or rebuild with \
             `cargo build --features mrc`.",
            pages
        )));
    }
    // outlines_pages + skip pages
    ...
};
```

`phase_bc_render_and_mrc()`: `#[cfg(feature = "mrc")]`

`phase_d_write()` の match:

```rust
#[cfg(feature = "mrc")]
PageOutput::Mrc(layers) => { ... }
#[cfg(feature = "mrc")]
PageOutput::BwMask(bw) => { ... }
```

### 11. src/pipeline/page_processor.rs — MRCプロセッサのゲート

```rust
#[cfg(feature = "mrc")]
use crate::mrc::compositor::{
    MrcConfig, TextMaskedParams, compose, compose_bw, compose_text_masked,
};
#[cfg(feature = "mrc")]
use image::DynamicImage;
```

- `ProcessPageParams` 構造体とそのimpl: `#[cfg(feature = "mrc")]`
- `process_page()` 関数: `#[cfg(feature = "mrc")]`
- `ProcessPageOutlinesParams`, `ProcessedPage`, `process_page_outlines()`: 変更なし

### 12. src/cache/store.rs — MRC/BWキャッシュ処理のゲート

- `MrcLayers`, `BwLayers` のimport: `#[cfg(feature = "mrc")]`
- `MRC_CACHE_FILES`, `BW_CACHE_FILES` 定数: `#[cfg(feature = "mrc")]`
- `store()` / `retrieve()` / `contains()` 内のMrc/BwMask分岐: `#[cfg(feature = "mrc")]`
- `store_mrc_or_bw()` メソッド: `#[cfg(feature = "mrc")]`

### 13. テストファイル

ファイル全体ゲート (`#![cfg(feature = "mrc")]`):

- `tests/ffi_jbig2enc_test.rs`
- `tests/ffi_leptonica_test.rs`
- `tests/render_test.rs`
- `tests/e2e_test.rs`

テスト単位ゲート:

- `tests/mrc_pipeline_test.rs`: segmenter/jbig2/compose/compose_bw/compose_text_masked関連テストをゲート。jpeg/compose_text_outlines関連テストは常時有効
- `tests/pipeline_test.rs`: ProcessPageParams/MrcConfig使用テストをゲート
- `tests/pdf_writer_test.rs`: write_mrc_page/write_bw_page使用テストをゲート
- `tests/cache_test.rs`: MrcLayers/PageOutput::Mrc使用テストをゲート
- `tests/content_stream_test.rs`: pixel_to_page_coords/PixelBBox使用テストをゲート

変更不要:

- `tests/text_to_outlines_test.rs`
- `tests/font_parser_test.rs`
- `tests/glyph_to_path_test.rs`
- `tests/config_test.rs`
- `tests/color_mode_config_test.rs`
- `tests/cli_test.rs`
- `tests/optimizer_test.rs`
- `tests/linearize_test.rs`
- `tests/pdf_reader_test.rs`
- `tests/text_state_test.rs`
- `tests/project_setup_test.rs`

## 対象ファイル一覧

| ファイル | 変更内容 |
| --- | --- |
| `Cargo.toml` | features定義、optional依存 |
| `build.rs` | cfg(feature = "mrc") ラップ |
| `src/lib.rs` | ffi, render モジュールゲート |
| `src/error.rs` | `From<PdfiumError>` ゲート |
| `src/mrc/mod.rs` | jbig2/segmenter ゲート、MrcLayers/BwLayers/ |
| | PageOutput バリアント ゲート |
| `src/mrc/compositor.rs` | import分離、MRC関数ゲート |
| `src/pdf/content_stream.rs` | pixel_to_page_coords ゲート |
| `src/pdf/image_xobject.rs` | jbig2 import/optimize_image_encoding ゲート |
| `src/pdf/writer.rs` | write_mrc_page/write_bw_page ゲート |
| `src/cache/store.rs` | MRC/BWキャッシュ処理ゲート |
| `src/pipeline/job_runner.rs` | MRCフォールバック条件分岐、phase_bc ゲート |
| `src/pipeline/page_processor.rs` | ProcessPageParams/process_page ゲート |
| `tests/` (複数) | 上記テストファイル参照 |

## 検証手順

1. **MRCなしビルド**: `cargo check --no-default-features` — コンパイル成功確認
2. **MRCありビルド**: `cargo check` — 既存動作に影響なし確認
3. **MRCなしテスト**: `cargo test --no-default-features` — text_to_outlines系テスト通過
4. **MRCありテスト**: `cargo test` — 全テスト通過（既存と同一）
5. **Clippy**: `cargo clippy --no-default-features` + `cargo clippy` — 警告なし
6. **E2Eテスト**: フォント埋め込みPDFで `--no-default-features` ビルドのバイナリが正常動作
7. **フォールバックエラー**: フォント未埋め込みPDFで `--no-default-features` 時に明確なエラーメッセージ
