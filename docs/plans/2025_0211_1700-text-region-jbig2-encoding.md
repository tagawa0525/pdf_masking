# テキスト領域エンコードをJPEGからJBIG2に変更

## Context

`compose_text_masked`パスでは、ページ全体をラスタライズした後にテキスト領域だけを切り出して個別XObjectとしてPDFに埋め込む。現在この切り出し領域はJPEGエンコードされているが、テキストは本質的に二値（黒/白）コンテンツであり、JBIG2のほうが適切。

**JBIG2に変更する利点:**

- **鮮明さ**: JPEGのブロックノイズ・ブラーがなく、テキストの輪郭がシャープに保たれる
- **データサイズ**: 1bit二値圧縮のため、8bit JPEG比で大幅に小さい
- **一貫性**: BWモード(`compose_bw`)は既にJBIG2を使用しており、TextMaskedモードもJBIG2に統一できる

**トレードオフ:** カラーテキストは白黒に変換される。ただしプロジェクトの目的はテキスト情報の除去であり、色の保持は不要。フォントが利用可能な場合は`text_to_outlines`パスが色を保持する。

## 変更対象ファイル

| ファイル                         | 変更内容                         |
| -------------------------------- | -------------------------------- |
| `src/ffi/leptonica.rs`           | `clip_rectangle`メソッド追加     |
| `src/mrc/compositor.rs`          | `crop_text_regions_jbig2`追加    |
| `src/mrc/mod.rs`                 | `jpeg_data` -> `jbig2_data`      |
| `src/pdf/writer.rs`              | JBIG2 XObject出力に変更          |
| `src/pipeline/page_processor.rs` | `quality`削除                    |
| `tests/`                         | 既存テスト更新 + 新規テスト追加  |

変更不要: `src/mrc/jbig2.rs`（`encode_mask`はそのまま再利用）、`src/mrc/segmenter.rs`、`src/mrc/jpeg.rs`（MRCパスで引き続き使用）

## 実装ステップ

TDDサイクル（RED→GREEN→REFACTOR）でコミットを分ける。

### Step 1: `Pix::clip_rectangle` FFI追加

1-bit Pixから矩形領域を切り出すために、
leptonicaの`pixClipRectangle`をラップする。
`boxCreate`と`pixClipRectangle`は
生成済みバインディングに存在する（`leptonica-sys 0.4.9`）。

**RED**: `tests/`に`clip_rectangle`のテストを追加（コンパイルエラー）

**GREEN**: `src/ffi/leptonica.rs`にimport追加
（`boxCreate`, `pixClipRectangle`）し、
`Pix::clip_rectangle(&self, x, y, w, h) -> Result<Pix>`
を実装。パターンは既存の`connected_component_bboxes`に倣う。

### Step 2: `crop_text_regions_jbig2`関数追加

**RED**: `tests/`にJBIG2クロップのテスト追加

**GREEN**: `src/mrc/compositor.rs`に新関数を追加:

```rust
pub fn crop_text_regions_jbig2(
    text_mask: &Pix,  // segment_text_maskの1-bit出力
    bboxes: &[PixelBBox],
) -> Result<Vec<(Vec<u8>, PixelBBox)>>
```

各bboxに対して`text_mask.clip_rectangle()` → `jbig2::encode_mask()`を実行。

### Step 3: `compose_text_masked`をJBIG2パスに切り替え

**RED**: 既存テストの`jpeg_data`参照を`jbig2_data`に変更（フィールド名変更でコンパイルエラー）

**GREEN**:

1. `src/mrc/mod.rs`: `TextRegionCrop.jpeg_data` → `jbig2_data`
2. `src/mrc/compositor.rs` `compose_text_masked`内:
   - `RgbaImage::from_raw` + `DynamicImage`構築 + `crop_text_regions`呼び出し(L241-249)を削除
   - `crop_text_regions_jbig2(&text_mask, &bboxes)`に置換
   - `TextRegionCrop { jbig2_data, ... }`を構築
3. `src/mrc/compositor.rs` `TextMaskedParams`から`quality`フィールド削除
4. `src/pipeline/page_processor.rs`: `TextMaskedParams`構築時の`quality`を削除

### Step 4: PDF writerをJBIG2出力に変更

**RED**: writerテストでJBIG2Decodeフィルタ・1bpc・Decode [1 0]を検証

**GREEN**: `src/pdf/writer.rs` `write_text_masked_page`(L334-346)を変更:

- `add_image_xobject(&region.jpeg_data, ..., 8, "DCTDecode", None)` →
  `add_mask_xobject(&region.jbig2_data, region.pixel_width, region.pixel_height)`
- `Decode [1 0]`を追加（BWパスL264-269と同じパターン）
- `color_space`変数を削除（JBIG2はDeviceGray固定）

### Step 5: 不要コードの削除

**REFACTOR**:

- `crop_text_regions`関数の削除（`compositor.rs` L86-140）
- 対応テスト4件の削除（`test_crop_text_regions_*`）
- docstring更新（JPEG→JBIG2）

## 検証

```bash
# 全テスト
cargo test

# 個別テスト（段階的に確認）
cargo test test_pix_clip_rectangle       # Step 1
cargo test test_crop_text_regions_jbig2  # Step 2
cargo test test_write_text_masked        # Step 4

# E2Eテスト（pdfium必要）
cargo test test_e2e_single_page_pdf
cargo test test_e2e_with_settings_yaml

# リント・フォーマット
cargo clippy
cargo fmt --check
```

出力PDFの目視確認: テキスト領域がシャープに表示され、テキスト選択不可であること。
