# ページ単位カラーモード設定の追加

## Context

現在の `jobs.yaml` は `pages` フィールドで処理対象ページを指定し、全ページが同一設定（RGB MRC）で処理される。ユーザーは実際のPDF（9割がB&W文字、一部がカラースキャン画像）に対して、ページごとに白黒/グレースケール/RGB/スキップを指定したい。

本変更では `pages` フィールドを廃止し、デフォルトカラーモード + モード別ページリストによる例外指定方式に移行する。

> **将来の検討事項**: カラーモードの自動判定（auto）は、スキャン画像の存在により判定精度の保証が困難なため、今回のスコープ外とする。将来的に実現可能性を検討する余地はある。

## 新しいYAML形式

```yaml
# settings.yaml
color_mode: bw          # グローバルデフォルト（省略時: rgb）

# jobs.yaml
jobs:
  - input: input.pdf
    output: output.pdf
    color_mode: bw              # ジョブデフォルト（省略時: settings.yamlの値）
    rgb_pages: [5-8, 100-120]   # 例外: これらのページはRGB
    grayscale_pages: [50-55]    # 例外: グレースケール
    skip_pages: [200]           # 例外: 元ページをそのままコピー
    # bw_pages も指定可能（デフォルトがrgbの場合など）
    # 上記リストに含まれない全ページ → デフォルト(bw)で処理
```

**カラーモードの定義:**

| モード       | 処理内容                | PDF出力                 |
| ------------ | ----------------------- | ----------------------- |
| `rgb`        | 3層MRC                  | RGB JPEG + JBIG2マスク  |
| `grayscale`  | グレースケール3層MRC    | Gray JPEG + JBIG2マスク |
| `bw`         | マスク層のみ            | JBIG2マスクのみ         |
| `skip`       | MRC処理なし             | 元ページコピー          |

## 変更対象ファイルと概要

### 1. 設定層

**`src/config/job.rs`**

- `ColorMode` enum 追加: `Rgb`, `Grayscale`, `Bw`, `Skip`（serde対応）
- `Job` 構造体:
  - `pages` フィールド削除
  - `color_mode: Option<ColorMode>` 追加
  - `bw_pages`, `grayscale_pages`, `rgb_pages`, `skip_pages` を
    `Option<Vec<u32>>` で追加
  - 各 `*_pages` は既存の `deserialize_pages` ロジックを再利用（`deserialize_optional_pages` ラッパー追加）
- `Job::resolve_page_modes()` メソッド追加:
  - モード別リストからページ→モードの `HashMap<u32, ColorMode>` を構築
  - 同一ページが複数リストに含まれる場合はエラー
  - 戻り値は1-basedページ番号のマップ（含まれないページはデフォルト）
- `parse_page_range()` と `deserialize_pages` 関連コードはそのまま維持

**`src/config/settings.rs`**

- `color_mode: ColorMode` フィールド追加（デフォルト: `Rgb`）

**`src/config/merged.rs`**

- `color_mode: ColorMode` フィールド追加
- `MergedConfig::new()`: `job.color_mode.unwrap_or(settings.color_mode)`

### 2. パイプライン層

**`src/pipeline/job_runner.rs`**

- `JobConfig` 構造体:
  - `pages: Vec<u32>` → `page_modes: Vec<(u32, ColorMode)>`（0-based、ページ順ソート済み）
  - 全ページ分のエントリを含む（デフォルトモード適用済み）
- `run_job()` の変更:
  - Phase B: `Skip` モードのページはレンダリングをスキップ（bitmap不要）
  - Phase C: `process_page()` に `ColorMode` を渡す。`MrcConfig` は引き続き共有
  - Phase D: `PageOutput` enum でマッチし、モードに応じた書き出しメソッドを呼び分け
    - `Mrc(layers)` → `writer.write_mrc_page(layers)`
    - `BwMask(bw)` → `writer.write_bw_page(bw)`
    - `Skip(skip)` → `writer.copy_page_from(source_doc, page_num)`
  - MRC/BWページのみ `masked_page_ids` に追加（optimizer がフォント削除する対象）
  - `PdfReader` をPhase Dまで保持（skip時のページコピーに必要）

**`src/pipeline/page_processor.rs`**

- `ProcessedPage.mrc_layers` → `ProcessedPage.output: PageOutput`
- `process_page()` に `color_mode: ColorMode` 引数追加
  - `Skip`: MRC処理なし、`PageOutput::Skip` を返す（キャッシュ不要）
  - `Bw`: segmenter + JBIG2のみ、`PageOutput::BwMask` を返す
  - `Rgb`/`Grayscale`: 既存パス + `color_mode` を `compose()` に渡す

**`src/main.rs`**

- `Job::resolve_page_modes()` を呼び出し、PDF全ページ数と照合してページ→モードのリストを構築
  - PDFのページ数を取得する必要がある（`PdfReader::open()` + `page_count()`）
  - override リストにないページにはデフォルト `color_mode` を適用
  - 1-based → 0-based 変換
- `JobConfig` に `page_modes: Vec<(u32, ColorMode)>` を設定

### 3. MRC層

**`src/mrc/mod.rs`**

- `PageOutput` enum 追加:

  ```rust
  Mrc(MrcLayers)    - RGB/Grayscale 3層MRC
  BwMask(BwLayers)  - JBIG2マスクのみ
  Skip(SkipData)    - 元ページコピー
  ```

- `MrcLayers` に `color_mode: ColorMode` フィールド追加（PDF書き出し時のColorSpace判定用）
- `BwLayers` 新規: `mask_jbig2: Vec<u8>`, `width: u32`, `height: u32`
- `SkipData` 新規: `page_index: u32`

**`src/mrc/compositor.rs`**

- `compose()` に `color_mode: ColorMode` 引数追加
  - `Rgb`: 現行動作（RGBA→RGB→JPEG）
  - `Grayscale`: RGBA→Gray→grayscale JPEG
- `compose_bw()` 新規関数: segmenter + JBIG2のみ、`BwLayers` を返す

**`src/mrc/jpeg.rs`**

- `encode_gray_to_jpeg(gray: &GrayImage, quality: u8)` 追加
  - `image` crateの `GrayImage` + `JpegEncoder` でグレースケールJPEGを出力

### 4. PDF出力層

**`src/pdf/writer.rs`**

- `write_mrc_page()`: `layers.color_mode` に基づき `DeviceRGB` / `DeviceGray` を切り替え
  - `add_image_xobject()` は既に `color_space: &str` を受け取るため、呼び出し側の変更のみ
- `write_bw_page(bw: &BwLayers)` 新規: JBIG2マスクのみをフルページ画像として描画
- `copy_page_from(source: &Document, page_num: u32)` 新規: lopdfでページオブジェクトを深コピー
  - `deep_copy_object()`: 再帰的にオブジェクトとその参照先をコピー、IDマッピングで共有参照を処理
  - コピーしたページの `Parent` を出力PDFの `Pages` ノードに差し替え
- `ensure_pages_id()` / `append_page_to_kids()` をヘルパーとして抽出（3つの書き出しメソッドで共有）

### 5. キャッシュ層

**`src/cache/hash.rs`**

- `CacheSettings` に `color_mode: ColorMode` 追加
- キャッシュキー計算にカラーモードを含める → 異なるモードの旧キャッシュは自動的にミスになる

**`src/cache/store.rs`**

- `CacheMetadata` に `color_mode` フィールド追加
- `store()`: BWモードの場合は `mask.jbig2` のみ保存（fg/bg なし）
- `retrieve()`: メタデータの `color_mode` に基づき適切なファイルを読み込み
- `Skip` ページはキャッシュ対象外

## 実装フェーズ（TDDサイクル）

### Phase 1: ColorMode enum と YAML パース

対象: `src/config/job.rs`, `src/config/settings.rs`, `src/config/merged.rs`

1. RED: `ColorMode` デシリアライズ、`*_pages` パース、
   `resolve_page_modes()`、競合検出のテスト
2. GREEN: enum定義、`Job` 新フィールド、
   `deserialize_optional_pages`、実装
3. REFACTOR: 必要に応じ

### Phase 2: グレースケールJPEGエンコード

対象: `src/mrc/jpeg.rs`

1. RED: `encode_gray_to_jpeg` のテスト
2. GREEN: 実装

### Phase 3: PageOutput enum と MRC処理の拡張

対象: `src/mrc/mod.rs`, `src/mrc/compositor.rs`, `src/pipeline/page_processor.rs`

- 3a: `PageOutput` enum / `BwLayers` / `SkipData` 導入、既存コードを
  `PageOutput::Mrc()` でラップ
- 3b: `compose()` にグレースケールパス追加、`compose_bw()` 新規
- 3c: `process_page()` に `color_mode` パラメータ追加、モード別分岐

### Phase 4: PDF writer の拡張

対象: `src/pdf/writer.rs`

1. RED: grayscale MRC (DeviceGray)、BW-only ページ、マルチモードドキュメントのテスト
2. GREEN: `write_mrc_page` のColorSpace分岐、`write_bw_page` 実装、ヘルパー抽出

### Phase 5: Skip モード（ページコピー）

対象: `src/pdf/writer.rs`

1. RED: `copy_page_from()` のテスト（コンテンツストリーム保持、MediaBox保持）
2. GREEN: `deep_copy_object()` と `copy_page_from()` 実装

> **リスク**: lopdfオブジェクトの深コピーは複雑。循環参照、暗号化ストリーム、外部リソース等のエッジケースあり。十分なテストが必要。

### Phase 6: パイプライン統合

対象: `src/pipeline/job_runner.rs`, `src/main.rs`

1. RED: 各モードでの `run_job()` テスト、`JobConfig` の `page_modes` テスト
2. GREEN: `main.rs` でのページモード解決、`run_job()` の4フェーズをモード対応に改修
3. 既存の `pages` フィールド使用箇所を `page_modes` に移行

### Phase 7: キャッシュ対応

対象: `src/cache/hash.rs`, `src/cache/store.rs`

1. RED: カラーモード別キャッシュキー差異、BWキャッシュ保存/復元のテスト
2. GREEN: `CacheSettings` / `CacheStore` 改修

### Phase 8: E2Eテスト

対象: `tests/e2e_test.rs`

- デフォルトRGB（後方互換）
- グレースケールモード
- BWモード
- ページ単位オーバーライド（複数モード混在）
- Skipモード（元ページ保持確認）

## 検証方法

1. **ユニットテスト**: 各フェーズのRED/GREENコミットで網羅
   - `cargo test` で全テスト通過を確認
2. **E2Eテスト**: 実際のPDFファイルを使い、出力PDFのXObject ColorSpace、ページ数、コンテンツストリームを検証
3. **手動確認**: 出力PDFをPDFビューアで開き、各モードの見た目を確認
   - BWページ: 白黒のみ、色なし
   - Grayscaleページ: グレースケール表示
   - RGBページ: フルカラー（現行と同等）
   - Skipページ: 元ページと同一（テキスト選択可能）
4. **ファイルサイズ比較**: BW < Grayscale < RGB の順でファイルサイズが小さくなることを確認
5. **既存テスト**: `pages` フィールド廃止に伴い既存テストを更新。後方互換性は破壊的変更として受け入れ
