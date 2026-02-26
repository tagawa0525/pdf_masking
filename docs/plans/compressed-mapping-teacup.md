# leptonica/jbig2enc: C FFI → pure Rust crate移行

## Context

現在 leptonica と jbig2enc は C/C++ ライブラリとして FFI 経由で使用している。
ビルドに C++17 コンパイラ、環境変数 3 つ（`JBIG2ENC_INCLUDE_PATH` 等）、
Nix/vcpkg によるシステムライブラリ管理が必要で、セットアップが複雑。

ユーザーが pure Rust 版の `leptonica` (0.1.1) と `jbig2enc` (0.1.0) crate を
作成したため、これらに移行し C/C++ 依存を完全に除去する。

## 変更対象ファイル

### 削除
- `src/ffi/leptonica_sys.rs` — raw FFI re-export（不要）
- `src/ffi/jbig2enc_sys.rs` — raw FFI 宣言（不要）
- `src/ffi/leptonica.rs` — safe wrapper（pure Rust crate が代替）
- `src/ffi/jbig2enc.rs` — safe wrapper（pure Rust crate が代替）
- `src/ffi/mod.rs` — ffi モジュール定義
- `csrc/jbig2enc_shim.cpp` — C++ shim（不要）
- `scripts/jbig2enc-CMakeLists.txt` — Windows ビルド用（不要）

### 修正
| ファイル | 変更内容 |
|---|---|
| `Cargo.toml` | `leptonica-sys` → `leptonica`, `libc`/`cc` 削除, `jbig2enc` 追加 |
| `build.rs` | C++ shim ビルド全体を削除（空の `main()` のみ、または削除） |
| `src/lib.rs` | `pub mod ffi` 削除 |
| `src/mrc/segmenter.rs` | pure Rust leptonica API に書き換え |
| `src/mrc/jbig2.rs` | pure Rust jbig2enc API に書き換え |
| `src/mrc/compositor.rs` | `crate::ffi::leptonica::Pix` → `leptonica::Pix` |
| `src/pdf/image_xobject.rs` | `crate::ffi::leptonica::Pix` → pure Rust API |
| `flake.nix` | leptonica/jbig2enc C ライブラリ依存と環境変数を削除 |
| `scripts/setup-windows.ps1` | leptonica(vcpkg)/jbig2enc ビルド手順を削除 |
| `tests/ffi_leptonica_test.rs` | pure Rust API に書き換え（ファイル名も変更） |
| `tests/ffi_jbig2enc_test.rs` | pure Rust API に書き換え（ファイル名も変更） |

## API マッピング

### leptonica

| 現在の FFI | pure Rust crate |
|---|---|
| `Pix::create(w, h, depth)` | `leptonica::Pix::new(w, h, PixelDepth)` |
| `Pix::from_raw_rgba(w, h, data)` | `Pix::new(w, h, Bit32)` + `PixMut` でデータコピー |
| `pix.get_width()` | `pix.width()` |
| `pix.get_height()` | `pix.height()` |
| `pix.get_depth()` | `pix.depth()` → `PixelDepth` enum |
| `pix.otsu_adaptive_threshold(sx, sy)` | `color::otsu_adaptive_threshold(&pix, sx, sy, 0, 0, 0.0)` → `(threshold, binary)` |
| `pix.get_region_masks()` | `recog::pageseg::segment_regions(&pix, &opts)` → `SegmentationResult` |
| `pix.connected_component_bboxes(4)` | `region::find_connected_components(&pix, Connectivity4)` → `Vec<ConnectedComponent>` |
| `pix.convert_to_gray()` | `pix.convert_rgb_to_gray(0.0, 0.0, 0.0)` |
| `pix.clip_rectangle(x,y,w,h)` | `pix.clip_rectangle(x, y, w, h)` |
| `pix.set_pixel(x, y, val)` | `PixMut::set_pixel(x, y, val)` |
| `pix.set_all_pixels(1)` | `PixMut` 経由 |
| `pix.leptonica_clone()` | `pix.deep_clone()` |
| `Drop` (pixDestroy) | 自動（pure Rust、手動メモリ管理不要） |

### jbig2enc

| 現在の FFI | pure Rust crate |
|---|---|
| `jbig2enc::encode_generic(&mut pix)` | `jbig2enc::encoder::encode_generic(&pix, false, 0, 0, true)` |

パラメータ: `full_headers=false`（PDF fragment）, `xres/yres=0`（auto）, `duplicate_line_removal=true`（TPGD）

### 型の対応

| 現在 | pure Rust |
|---|---|
| `crate::ffi::leptonica::Pix` | `leptonica::Pix` |
| `crate::ffi::leptonica::RegionMasks` | `leptonica::recog::SegmentationResult` |
| `(u32, u32, u32, u32)` (bbox) | `ConnectedComponent { bounds: Box, .. }` |

## 実装手順

### 1. Cargo.toml 更新
```toml
[features]
mrc = ["dep:pdfium-render", "dep:leptonica", "dep:jbig2enc"]
# libc, cc を削除

[dependencies]
leptonica = { version = "0.1.1", optional = true }
jbig2enc = { version = "0.1.0", default-features = false, optional = true }
# leptonica-sys, libc 削除

[build-dependencies]
# cc 削除
```

### 2. build.rs 簡素化
C++ shim ビルドを全削除。`build.rs` 自体が不要なら削除。

### 3. src/ffi/ 削除 + src/lib.rs 更新
`pub mod ffi` を削除。

### 4. src/mrc/segmenter.rs 書き換え
```rust
use leptonica::{Pix, PixelDepth};
use leptonica::color::otsu_adaptive_threshold;
use leptonica::recog::pageseg::{segment_regions, PageSegOptions};

pub fn segment_text_mask(rgba_data: &[u8], width: u32, height: u32) -> Result<Pix> {
    let pix = pix_from_raw_rgba(width, height, rgba_data)?;
    let gray = pix.convert_rgb_to_gray(0.0, 0.0, 0.0)?;
    let tile_sx = width.clamp(16, 2000);
    let tile_sy = height.clamp(16, 2000);
    let (_threshold, binary) = otsu_adaptive_threshold(&gray, tile_sx, tile_sy, 0, 0, 0.0)?;
    let opts = PageSegOptions { detect_halftone: true, ..Default::default() };
    let result = segment_regions(&binary, &opts)?;
    // textline_mask は非 Option (空画像の場合もある)
    // 前景ピクセルがあるかチェック、なければ空マスク返却
    Ok(result.textline_mask)
}
```

### 5. src/mrc/jbig2.rs 書き換え
```rust
use jbig2enc::encoder::encode_generic;

pub fn encode_mask(mask: &Pix) -> Result<Vec<u8>> {
    let data = encode_generic(mask, false, 0, 0, true)
        .map_err(|e| PdfMaskError::jbig2_encode(e.to_string()))?;
    Ok(data)
}
```
シグネチャが `&mut Pix` → `&Pix` に変わるため、呼び出し元の `mut` も不要になる。

### 6. src/mrc/compositor.rs 更新
- `crate::ffi::leptonica::Pix` → `leptonica::Pix`
- `jbig2::encode_mask(&mut ...)` → `jbig2::encode_mask(&...)` （mut 不要）

### 7. src/pdf/image_xobject.rs 更新
- `crate::ffi::leptonica::Pix` → pure Rust API
- `from_raw_rgba` → ヘルパー関数
- `otsu_adaptive_threshold` → 関数呼び出し形式
- `jbig2::encode_mask(&mut binary)` → `jbig2::encode_mask(&binary)`

### 8. テスト更新
- `tests/ffi_leptonica_test.rs` → `tests/leptonica_test.rs`
- `tests/ffi_jbig2enc_test.rs` → `tests/jbig2enc_test.rs`
- import パスを `pdf_masking::ffi::leptonica` → `leptonica` に変更
- API 差分を反映（`get_width()` → `width()` 等）

### 9. ビルド環境更新
- `flake.nix`: `leptonica`, `jbig2enc` パッケージと関連環境変数を削除
- `scripts/setup-windows.ps1`: vcpkg leptonica、jbig2enc ソースビルド手順を削除
- `csrc/` ディレクトリ削除

## ヘルパー関数: pix_from_raw_rgba

`segmenter.rs` と `image_xobject.rs` で共通使用するため、
`src/mrc/segmenter.rs`（または適切な共通モジュール）に配置:

```rust
fn pix_from_raw_rgba(width: u32, height: u32, data: &[u8]) -> Result<Pix> {
    // バリデーション + Pix::new(w, h, Bit32) + PixMut でデータコピー
}
```

## エラーハンドリング

- `leptonica` crate のエラー → `PdfMaskError::SegmentationError` に変換（`map_err`）
- `jbig2enc::Jbig2Error` → `PdfMaskError::Jbig2EncodeError` に変換（`map_err`）
- `From` impl の追加は不要（手動 `map_err` で十分）

## 検証

```bash
cargo build                               # mrc feature 有効でビルド
cargo build --no-default-features         # mrc feature 無効でビルド
cargo test                                 # 全テスト
cargo test --test leptonica_test          # leptonica テスト
cargo test --test jbig2enc_test           # jbig2enc テスト
cargo clippy                               # lint
cargo fmt --check                          # format
```

統合テスト（`sample/pdf_test.pdf` を使用）で実際のPDF処理が正常動作することを確認。
