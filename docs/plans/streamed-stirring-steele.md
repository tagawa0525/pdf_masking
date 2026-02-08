# テキスト選択的ラスタライズ + 画像内リダクション

## Context

現在のパイプラインは、ページ全体を300 DPIでビットマップにレンダリングし、MRC 3層（背景JPEG + 前景JPEG + JBIG2マスク）にエンコードしている。これにより：

- **ベクターテキスト**（数KB）がラスタ画像（数百KB）に変換される
- **画像XObject**（既にPDF内で圧縮済み）が再ラスタライズ・再JPEG圧縮される
- **同じ画像が2枚のJPEG**（前景+背景）として格納される

結果：173KB → 1.6MB（約10倍のサイズ増加）

加えて、画像XObject内にピクセルとして含まれるテキストは、白色矩形で視覚的に隠されているだけで、データとしては画像内に残っている。白矩形を除去すると復元可能。

## 方針

2つの問題を解決する：

1. **サイズ削減：** PDFテキスト（BT...ET）のみ画像化し、画像XObjectやベクター描画はそのまま保持
2. **画像内リダクション：** 白色矩形で覆われた画像XObject領域のピクセルデータを白で上書きし、データレベルで永続化

**処理の流れ（変更後）：**
```
元PDF
 ├─ コンテンツストリーム解析
 │   ├─ BT...ETブロックを除去 → テキスト除去済みストリーム
 │   ├─ 画像Do/ベクター描画はそのまま保持
 │   └─ 白色fill矩形の位置を検出
 ├─ 画像XObjectリダクション
 │   ├─ 白色矩形と画像XObjectの重なり判定（既存 bbox_overlaps）
 │   ├─ 重なる画像: デコード → 該当領域を白で塗りつぶし → 再エンコード
 │   └─ 重ならない画像: そのまま保持
 ├─ ページをpdfiumでレンダリング（テキストの見た目取得用）
 ├─ segmenterでテキストマスク → 矩形領域抽出
 ├─ テキスト領域だけクロップ → JPEG化（SMask付きでテキスト部分のみ表示）
 └─ 元ページをdeep copy + 改変
     ├─ コンテンツストリーム = テキスト除去済み + テキスト画像Doオペレータ
     ├─ Resources/XObject = リダクション済み画像 + テキスト領域XObject
     └─ フォント削除（既存optimizer）
```

**期待サイズ：** 元の173KB + テキスト領域JPEG（20-50KB程度） ≈ 200-250KB

## 実装計画（8 PR、順次実行）

### PR 1: `feat/strip-text-operators` — コンテンツストリームのテキスト除去

**ファイル:** `src/pdf/content_stream.rs`

```rust
pub fn strip_text_operators(content_bytes: &[u8]) -> Result<Vec<u8>>
```

- `Content::decode()` でオペレーション列を取得
- BT/ET のネスト深度を追跡し、深度>0 の全オペレーションを除去
- `Content::encode()` で再エンコード

**テスト:** BT...ETブロックが除去される、非テキスト（q/cm/Do）は保持される、空ストリーム、テキストなし

---

### PR 2: `feat/text-region-bboxes` — テキスト領域の矩形抽出

**ファイル:** `src/ffi/leptonica.rs`, `src/mrc/segmenter.rs`

```rust
// leptonica.rs - pixConnCompBB のFFIラップ
impl Pix {
    pub fn connected_component_bboxes(&self, connectivity: i32) -> Result<Vec<PixelBBox>>
}

// segmenter.rs
pub struct PixelBBox { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }
pub fn extract_text_bboxes(text_mask: &Pix, merge_distance: u32) -> Result<Vec<PixelBBox>>
```

- leptonica の `pixConnCompBB` → `BOXA` → `Vec<PixelBBox>` 変換
- 近接矩形のマージ（XObject数削減）
- 最小面積閾値（4x4px未満を除外）

---

### PR 3: `feat/crop-text-regions` — テキスト領域のクロップ・JPEG化

**ファイル:** `src/mrc/compositor.rs`, `src/pdf/content_stream.rs`

```rust
// compositor.rs - ビットマップからテキスト領域をクロップしJPEG化
pub fn crop_text_regions(
    bitmap: &DynamicImage, bboxes: &[PixelBBox],
    quality: u8, color_mode: ColorMode,
) -> Result<Vec<(Vec<u8>, PixelBBox)>>

// content_stream.rs - ピクセル座標→PDFポイント座標変換（Y軸反転含む）
pub fn pixel_to_page_coords(
    pixel_bbox: &PixelBBox,
    page_width_pts: f64, page_height_pts: f64,
    bitmap_width_px: u32, bitmap_height_px: u32,
) -> BBox
```

**依存:** PR 2

---

### PR 4: `feat/detect-white-fills` — 白色fill矩形の検出

**ファイル:** `src/pdf/content_stream.rs`

```rust
/// コンテンツストリームから白色fill矩形の位置を抽出する。
/// 追跡するオペレータ:
///   色設定: rg/g/k/sc/scn + cs (fill color)
///   パス構築: re (rectangle)
///   fill: f/F/f*
/// CTMスタック(q/Q/cm)も追跡して正確なBBoxを計算。
pub fn extract_white_fill_rects(content_bytes: &[u8]) -> Result<Vec<BBox>>
```

- 現在の `extract_xobject_placements` と同様のCTMスタック追跡ロジックを共有
- fill colorが白（RGB: 1,1,1 / Gray: 1 / CMYK: 0,0,0,0）かを判定
- `re` + `f` の組み合わせで矩形fill操作を検出
- CTM適用済みのページ座標BBoxを返す

既存の再利用:
- `Matrix`, `BBox`, `ctm_to_bbox`, `operand_to_f64` — 全て `content_stream.rs` に既存

---

### PR 5: `feat/image-xobject-processing` — 画像XObjectのリダクション + B&W最適圧縮

**ファイル:** `src/pdf/image_xobject.rs`（既存プレースホルダを実装に置換）

**5a: リダクション**
```rust
/// 画像XObjectをデコードし、指定領域を白で塗りつぶして再エンコードする。
pub fn redact_image_regions(
    image_stream: &lopdf::Stream,
    redact_bboxes: &[BBox],     // 白塗り対象領域（画像ローカル座標）
    image_placement: &BBox,      // 画像のページ上での配置
) -> Result<Option<Vec<u8>>>    // None=変更不要, Some=再エンコード済みJPEGデータ
```

処理:
1. `image_stream.dict` から Filter と ColorSpace を取得
2. Filter に応じたデコード（DCTDecode→JPEG, FlateDecode→PNG, etc.）
3. ページ座標の `redact_bboxes` を画像ピクセル座標に変換
4. 該当ピクセル領域を白（255,255,255）で上書き
5. **元と同じ形式で再エンコード**（JPEG→JPEG, PNG→PNG, JBIG2→JBIG2, CCITT→CCITT）
6. 重なりがなければ `None` を返す（画像データの変更不要）

対応する Filter とエンコード方式:
| 元の Filter | デコード | 再エンコード | 備考 |
|---|---|---|---|
| DCTDecode | `image` crate JPEG | `jpeg::encode_*_to_jpeg()` | 最頻出 |
| FlateDecode | `flate2` 解凍 + raw pixel解釈 | `flate2` 圧縮（ロスレス） | Predictorに注意 |
| JBIG2Decode | leptonica Pix | `jbig2::encode_mask()` | B&W専用 |
| CCITTFaxDecode | leptonica Pix | leptonica CCITT G4 encoder | B&W専用 |
| JPXDecode | `jpeg2k` crate (OpenJPEG) | `jpeg2k` crate (OpenJPEG) | 依存追加が必要 |
| LZWDecode | LZW解凍 | FlateDecode で再圧縮 | LZWはレガシー、Flateが上位互換 |
| RunLengthDecode | RLE解凍 | FlateDecode で再圧縮 | まれ |
| (なし/非圧縮) | raw pixel | FlateDecode で圧縮 | まれ |

JPXDecode対応に必要な追加:
- `Cargo.toml`: `jpeg2k` crate 追加
- `flake.nix`: `pkgs.openjpeg` を buildInputs + LD_LIBRARY_PATH に追加

フィルタ連鎖（例: FlateDecode + ASCII85Decode）は順次デコード後、最も効率的な単一フィルタで再エンコード。

**5b: 画像最適圧縮（最小サイズ選択）**
```rust
/// 画像XObjectを複数形式でエンコードし、最小サイズの結果を返す。
/// リダクション後の画像、またはリダクション不要だが最適化対象の画像に適用。
pub fn optimize_image_encoding(
    decoded: &DynamicImage,
    original_size: usize,       // 元のストリームサイズ（比較用）
    quality: u8,
) -> Result<Option<OptimizedImage>>  // None=元のサイズより小さくならない

pub struct OptimizedImage {
    pub data: Vec<u8>,
    pub filter: &'static str,       // "DCTDecode" | "JBIG2Decode" | "FlateDecode"
    pub color_space: &'static str,  // "DeviceRGB" | "DeviceGray"
    pub bits_per_component: u8,     // 8 or 1
}
```

処理:
1. デコード済み画像から以下の候補を生成:
   - **候補A: B&W JBIG2** — Otsu閾値で二値化 → JBIG2エンコード（1bit DeviceGray）
   - **候補B: グレースケールJPEG** — RGB→Gray変換 → JPEG（8bit DeviceGray）
   - **候補C: 元形式で再エンコード** — リダクション適用後の同一形式エンコード
2. 各候補のバイトサイズを比較
3. 元のストリームサイズ以下で最小の候補を採用
4. どの候補も元より小さくならなければ `None`（元のまま保持、リダクション以外の変更なし）

**BitsPerComponent=1の画像:** 既にB&Wなので候補Aのみ試行（JBIG2再圧縮のみ）

既存の再利用:
- `bbox_overlaps()` — `image_xobject.rs` に既存
- `jpeg::encode_rgb_to_jpeg()` — `src/mrc/jpeg.rs` に既存
- `jbig2::encode_mask()` — `src/mrc/jbig2.rs` に既存
- `segment_text_mask()` / Otsu閾値 — `src/mrc/segmenter.rs` に既存
- `extract_xobject_placements()` — `content_stream.rs` に既存

**依存:** PR 4

---

### PR 6: `feat/text-masked-data-type` — 新データ型と合成関数

**ファイル:** `src/mrc/mod.rs`, `src/mrc/compositor.rs`

```rust
// mod.rs - 新データ型
pub struct TextRegionCrop {
    pub jpeg_data: Vec<u8>,
    pub bbox_points: BBox,
    pub width: u32,
    pub height: u32,
}

/// 画像XObjectの変更内容
pub struct ImageModification {
    pub data: Vec<u8>,
    pub filter: String,            // "DCTDecode" | "JBIG2Decode" | "FlateDecode"
    pub color_space: String,       // "DeviceRGB" | "DeviceGray"
    pub bits_per_component: u8,    // 8 or 1
}

pub struct TextMaskedData {
    pub stripped_content_stream: Vec<u8>,
    pub text_regions: Vec<TextRegionCrop>,
    pub modified_images: HashMap<String, ImageModification>,  // XObject名 → 変更内容
    pub page_index: u32,
    pub color_mode: ColorMode,
}

pub enum PageOutput {
    Mrc(MrcLayers),
    BwMask(BwLayers),
    Skip(SkipData),
    TextMasked(TextMaskedData),  // 新規追加
}
```

`compose_text_masked()` が PR 1-5 の関数を統合。

**依存:** PR 1, 2, 3, 4, 5

---

### PR 7: `feat/write-text-masked-page` — PDF Writer拡張 + パイプライン統合

**ファイル:** `src/pdf/writer.rs`, `src/pipeline/page_processor.rs`, `src/pipeline/job_runner.rs`

**Writer:**
```rust
pub fn write_text_masked_page(
    &mut self, source: &Document, page_num: u32,
    data: &TextMaskedData, color_mode: ColorMode,
) -> Result<ObjectId>
```

1. `deep_copy_object` で元ページを再帰deep copy
2. コンテンツストリーム置換: テキスト除去済み + テキスト画像Doオペレータ
3. Resources/XObjectにテキスト領域XObjectを追加
4. `redacted_images` のあるXObjectはdeep copy後にストリームデータを差し替え

**Pipeline:**
- `process_page`: `preserve_images=true` かつ RGB/Grayscale → `compose_text_masked` 分岐
- `run_job` Phase D: `PageOutput::TextMasked` → `write_text_masked_page`
- ページ寸法（pts）はビットマップサイズから逆算: `bitmap_px * 72.0 / dpi`
- 既存 `preserve_images` 設定（デフォルトtrue）で分岐制御

**依存:** PR 6

---

### PR 8: `feat/cache-text-masked` — キャッシュ対応

**ファイル:** `src/cache/store.rs`

```
<hash>/
  metadata.json          # type="text_masked", regions, redacted_images
  stripped_content.bin
  region_0.jpg ... region_N.jpg
  redacted_Im1.jpg ...   # リダクション済み画像（変更されたもののみ）
```

**依存:** PR 6

---

## 注意点

- **テキスト領域のオーバーレイ:** テキスト画像XObjectにはSMask（テキストマスク）を付与し、テキストピクセルのみ表示。背景の画像/ベクターと二重描画にならない
- **インライン画像（BI...EI）:** lopdfパーサが単一Operationとして処理するため、BT/ET深度追跡で問題なし
- **BT...ET外のテキスト状態オペレータ:** 残しても無害（描画オペレータなしでは効果なし）
- **Bwモード:** 既存JBIG2パイプライン維持（変更なし）
- **preserve_images=false:** 既存MRCパイプラインにフォールバック
- **画像フォーマット対応:** リダクション時は元の形式で再エンコード（JPEG→JPEG, Flate→Flate, JPEG2000→JPEG2000等）。B&W最適圧縮（5b）のみ意図的にJBIG2へ変換。LZW/RLE/非圧縮はFlateDecodeに統一
- **白色判定:** `rg`=`1 1 1`, `g`=`1`, `k`=`0 0 0 0` を白と判定。カラースペース変換は対象外（ほとんどのケースで十分）

## 検証方法

1. `cargo test` で既存テスト全パスを確認
2. `sample/jobs.yaml` で `pdf_test.pdf` → `pdf_test_masked.pdf` を生成
3. 出力サイズが元の173KBに近いこと（200-250KB以下目標）を確認
4. PDFビューアでテキストが見た目通り表示されることを確認
5. テキスト選択・コピーが不可能であることを確認
6. 元PDF内の画像が不必要に再圧縮されていないことを確認
7. 白矩形で覆われた画像領域の下のデータが実際に白で上書きされていることを確認（画像XObjectをデコードして検証）
