# PDF Masking CLI Tool — 実装計画

## Context

PDFファイルの指定ページからテキスト情報を完全に除去するCLIツール。テキストデータを削除しつつ、
見た目はMRC圧縮（ITU-T T.44準拠の3層構造）による画像化で保持する。対象PDFは数千ページ規模で、
9割がB&Wテキスト、容量の8割が画像ページ。

技術スタック: **Rust** + pdfium-render + lopdf + leptonica(C FFI) +
jbig2enc(C FFI) + qpdf

## モジュール構成

```text
src/
  main.rs                     CLI エントリポイント（引数解析、終了コード）
  error.rs                    PdfMaskError enum（thiserror）
  config/
    mod.rs
    settings.rs               settings.yaml 解析、デフォルト値
    job.rs                    jobs.yaml 解析、ページ範囲展開
    merged.rs                 設定 + ジョブオーバーライド → 実効設定
  pdf/
    mod.rs
    reader.rs                 lopdf でPDF読込、ページ列挙
    content_stream.rs         コンテンツストリーム解析（CTM追跡、画像XObject BBox算出）
    writer.rs                 MRC XObject構築、SMask参照、コンテンツストリーム組立
    optimizer.rs              オブジェクトストリーム、FlateDecode、孤立オブジェクト除去、フォント削除
    image_xobject.rs          画像XObjectのデコード/再エンコード、重なり検出・塗りつぶし
  render/
    mod.rs
    pdfium.rs                 pdfium-render ラッパー: ページ→ DynamicImage（メモリ上のみ）
  mrc/
    mod.rs                    MrcLayers 構造体定義
    segmenter.rs              leptonica segmentation: bitmap → mask/fg/bg 分離
    jbig2.rs                  jbig2enc ラッパー: 1-bit mask → JBIG2 bytes
    jpeg.rs                   image crate: fg/bg → JPEG bytes
    compositor.rs             パイプライン統合: bitmap + config → MrcLayers
  ffi/
    mod.rs
    leptonica_sys.rs          leptonica raw bindings（既存 leptonica-sys crate 利用）
    leptonica.rs              安全ラッパー（Pix型、RAII Drop）
    jbig2enc_sys.rs           jbig2enc raw bindings（custom bindgen + C shim）
    jbig2enc.rs               安全ラッパー
  cache/
    mod.rs
    hash.rs                   SHA-256（コンテンツストリーム + 設定）
    store.rs                  ファイルシステムキャッシュ: hash → MRC層バイト列
  pipeline/
    mod.rs
    page_processor.rs         ページ単位処理: キャッシュ確認 → 描画 → MRC → キャッシュ保存
    job_runner.rs             ジョブ単位: PDF読込 → 並列ページ処理 → 出力PDF組立
    orchestrator.rs           全ジョブ実行、リニアライズ
  linearize.rs                qpdf-rs ラッパー: リニアライズ
```

### 依存方向（上→下にのみ依存）

```text
main → pipeline/orchestrator → pipeline/job_runner → pipeline/page_processor
                                                          │
                        ┌─────────────────────────────────┼──────────────┐
                        │              │                  │              │
                    pdf/reader    render/pdfium     mrc/compositor  cache/store
                    pdf/content_stream                    │
                    pdf/writer         ┌──────────────────┼─────────┐
                    pdf/optimizer      │                  │         │
                    pdf/image_xobject  mrc/segmenter   mrc/jbig2  mrc/jpeg
                                       │                  │
                                    ffi/leptonica      ffi/jbig2enc
```

## エラー型

```rust
#[derive(Debug, thiserror::Error)]
pub enum PdfMaskError {
    ConfigError,          // YAML解析、ファイル不在、無効なページ範囲
    PdfReadError,         // lopdf 読込/解析
    PdfWriteError,        // lopdf オブジェクト構築
    ContentStreamError,   // 不正なオペレータ/オペランド
    RenderError,          // pdfium 初期化/描画
    SegmentationError,    // leptonica 呼出失敗
    Jbig2EncodeError,     // jbig2enc 失敗
    JpegEncodeError,      // JPEG エンコード失敗
    ImageXObjectError,    // 画像XObject デコード/再エンコード
    CacheError,           // キャッシュI/O
    LinearizeError,       // qpdf 失敗
    IoError(std::io::Error),
}
```

## 実装順序（TDDフェーズ）

各フェーズ = 1 feature branch。RED→GREEN→REFACTORサイクルをコミット履歴に残す。

### Phase 0: プロジェクト基盤

**Branch:** `feat/project-setup`

- Cargo.toml に全依存関係追加
- 全モジュールファイルを空スタブで作成（`pub mod` 宣言のみ）
- `PdfMaskError` enum 実装
- `cargo build` / `cargo test` 成功を確認

### Phase 1: 設定ファイル解析

**Branch:** `feat/config-parsing`
**対象:** `config/settings.rs`, `config/job.rs`, `config/merged.rs`

外部依存なし（serde/serde_yamlのみ）。完全に分離してテスト可能。

- ページ範囲パーサ: `"5-10"` → `Vec<u32>`
- Settings / Job 構造体の serde デシリアライズ
- 設定マージロジック（ジョブ > settings.yaml > デフォルト）
- settings.yaml 自動検出（ジョブファイルと同ディレクトリ）

### Phase 2: コンテンツストリーム解析

**Branch:** `feat/pdf-content-stream`
**対象:** `pdf/reader.rs`, `pdf/content_stream.rs`

最も知的に複雑なモジュール。PDFグラフィックス状態を正確に追跡する。

**追跡するオペレータ:**

- `q` / `Q`: グラフィックス状態スタック push/pop
- `cm`: CTM（Current Transformation Matrix）更新
- `Do /ImN`: XObject描画コマンド

**出力:** `Vec<ImagePlacement>` — 各画像XObjectの名前、BBox、CTM、オブジェクトID

**CTMからBBox算出:** 単位正方形 [0,0]-[1,1] をCTMで変換し、変換後の4頂点のmin/maxを取る。

**v1の簡略化:** Form XObjectの再帰解析は行わない。Form XObjectはMRC画像化の対象とする。

### Phase 3: FFI — leptonica

**Branch:** `feat/ffi-leptonica`
**対象:** `ffi/leptonica_sys.rs`, `ffi/leptonica.rs`

既存 `leptonica-sys` crate を利用し、安全ラッパーを自作。

- `Pix` 型: `*mut PIX` を所有、`Drop` で `pixDestroy` 呼出
- メソッド: `from_raw_rgba()`, `binarize()`, `get_region_masks()`
  （`pixGetRegionsBinary` ラッパー）
- 全unsafe コードはこのモジュール内に隔離

### Phase 4: FFI — jbig2enc

**Branch:** `feat/ffi-jbig2enc`
**対象:** `ffi/jbig2enc_sys.rs`, `ffi/jbig2enc.rs`

jbig2encはC++だが、薄いCシム（`extern "C"` ラッパー）を作成してbindgenのC++問題を回避。
`build.rs` + `cc` crate でビルド。

- `encode_generic(pix: &Pix, xres: i32, yres: i32) -> Result<Vec<u8>>`
- 戻り値の `uint8_t*` は `libc::free()` で解放（C allocator）

### Phase 5: MRCパイプライン

**Branch:** `feat/mrc-pipeline`
**対象:** `mrc/segmenter.rs`, `mrc/jbig2.rs`, `mrc/jpeg.rs`, `mrc/compositor.rs`
**依存:** Phase 3, 4

**キャッシュキー:** `SHA-256(page_content_stream || settings_canonical_json)`
設定のうちMRC出力に影響するもののみハッシュに含める
（dpi, fg_dpi, bg_quality, fg_quality, preserve_images）

```rust
struct MrcLayers {
    mask_jbig2: Vec<u8>,        // JBIG2 1-bit マスク
    foreground_jpeg: Vec<u8>,   // JPEG 低解像度前景
    background_jpeg: Vec<u8>,   // JPEG 背景（テキスト領域インペインティング済み）
    // + 各層の幅・高さ
}
```

**パイプライン:**

1. RGBA bitmap → leptonica Pix 変換
2. Otsu二値化 → `pixGetRegionsBinary` でテキストマスク取得
3. **マスク層:** テキストマスク（1-bit、フル解像度）→ jbig2enc
4. **背景層:** 原画像のテキスト領域をインペインティング（局所中央値で塗りつぶし）→ JPEG
5. **前景層:** テキスト外を白塗り → 1/4解像度にダウンスケール → JPEG

**テスト:** 合成画像で MrcLayers を生成し、`pixel = mask ? fg : bg` で再構成 → 原画像とPSNR比較（> 30dB）

### Phase 6: ページ描画

**Branch:** `feat/page-rendering`
**対象:** `render/pdfium.rs`

- `render_page(pdf_path, page_index, dpi) -> Result<DynamicImage>`
- pdfiumライブラリは `OnceCell` で一度だけ初期化
- bitmap はメモリ上のみ（ファイルI/Oなし）

### Phase 7: PDF構築（MRC → PDF）

**Branch:** `feat/pdf-writer`
**対象:** `pdf/writer.rs`, `pdf/image_xobject.rs`

**出力PDFのページ構造（SMaskベース3層合成）:**

```text
Contents: q <bg_ctm> cm /BgImg Do Q  q <fg_ctm> cm /FgImg Do Q  [保持する画像コマンド]
XObject:
  /BgImg: DCTDecode JPEG（背景）
  /FgImg: DCTDecode JPEG（前景）, /SMask → /MaskImg
  /MaskImg: JBIG2Decode 1-bit DeviceGray（マスク）
  [保持する /ImN エントリ]
```

**画像XObject改変（セキュリティ要件）:**

- マスキングオブジェクトと画像XObjectの重なり判定
- 重なりあり: デコード → 重なり領域を白塗り → 再エンコード
- 重なりなし: そのまま保持

### Phase 8: キャッシュ

**Branch:** `feat/caching`
**対象:** `cache/hash.rs`, `cache/store.rs`

**キャッシュキー:** `SHA-256(page_content_stream || settings_canonical_json)`
設定のうちMRC出力に影響するもののみハッシュに含める（dpi, fg_dpi, bg_quality, fg_quality, preserve_images）

**ストレージ:** `<cache_dir>/<hex_hash>/` に以下のファイルを格納:
`mask.jbig2`, `foreground.jpg`, `background.jpg`, `metadata.json`。
キャッシュヒット時は描画・セグメンテーション・エンコードを全スキップ。

### Phase 9: PDF最適化

**Branch:** `feat/pdf-optimization`
**対象:** `pdf/optimizer.rs`

1. マスク済みページの `/Font` リソース削除
2. `lopdf::Document::delete_unused_objects()` で孤立オブジェクト除去
3. 未圧縮ストリームに FlateDecode 適用
4. オブジェクトストリーム / クロスリファレンスストリーム有効化

### Phase 10: パイプライン統合・並列化

**Branch:** `feat/pipeline`
**対象:** `pipeline/page_processor.rs`, `pipeline/job_runner.rs`,
`pipeline/orchestrator.rs`

**並列化戦略（ページレベル）:**

```text
Phase A（逐次）: 全ページのコンテンツストリーム解析
Phase B（逐次、pdfium）: 全ページをビットマップに描画
Phase C（rayon並列）: 各ビットマップのMRC処理（セグメンテーション + エンコード）
Phase D（逐次）: 出力PDF組立
```

- rayonはPhase C（最も重い処理）のみに使用
- leptonica/jbig2enc のスレッド安全性が不明なため、FFI呼出は `Mutex` で保護
  （後にプロファイルで判断）
- `lopdf::Document` は `Send`/`Sync` でないため、メインスレッドでのみ操作

**エラーセマンティクス:**

- 1ページ失敗 → そのジョブ全体が失敗（部分出力なし）
- 1ジョブ失敗 → 他ジョブは続行。終了コードで失敗を通知

### Phase 11: リニアライズ

**Branch:** `feat/linearization`
**対象:** `linearize.rs`

qpdf-rs で中間PDF → リニアライズ済み最終PDF。オブジェクトストリーム生成・ストリーム圧縮も qpdf 側で実行。

### Phase 12: CLIエントリポイント

**Branch:** `feat/cli`
**対象:** `main.rs`

```bash
pdf_masking <jobs.yaml>...
pdf_masking --help
pdf_masking --version
```

フレームワーク不使用。`std::env::args()` で直接処理。

### Phase 13: E2Eインテグレーションテスト

**Branch:** `feat/e2e-tests`
**対象:** `tests/`

テストフィクスチャ（`tests/fixtures/`）:

- テキストのみページ
- テキスト + 画像XObject
- マスキングオブジェクト重なりページ
- 複数ページ混在

## リスクと対策

| リスク | 深刻度 | 対策 |
| --- | --- | --- |
| コンテンツストリーム解析の正確性 | 高 | `q`/`Q`/`cm`/`Do`のみ対応。実PDFで早期検証。 |
| | | 失敗時はpreserve_images=falseにフォールバック |
| leptonicaセグメンテーション品質 | 中 | 入力は常に既知DPI。B&Wテキスト（9割）では問題なし。 |
| | | Otsu二値化で安定性確保 |
| jbig2enc スレッド安全性 | 中 | Mutexで保護して開始。プロファイル後に判断 |
| jbig2enc C++ bindgen | 中 | 薄いCシム（`extern "C"`）で回避。`cc` crateでビルド |
| pdfium ライブラリ可用性 | 中 | static feature検討。CI用にChromiumリリースから |
| | | ダウンロード |
| ファイルサイズ肥大 | 中 | Phase 9の最適化 + qpdfリニアライズで対処。 |
| | | 各Phase後にサイズ計測 |
| FFI境界のメモリ管理 | 中 | RAII型でC資源を所有。Cアロケータの解放関数を |
| | | 正確に使用 |

## 検証方法

| コンポーネント | 単体テスト | 統合テスト | 受入基準 |
| --- | --- | --- | --- |
| 設定解析 | YAML文字列フィクスチャ | 実ファイル読込 | 全YAML機能動作 |
| コンテンツストリーム | 手製オペレータ列 | 実PDFフィクスチャ | BBox算出が正確 |
| FFI leptonica | 合成画像 | 実ページビットマップ | クラッシュなし、正確なマスク |
| FFI jbig2enc | 合成1-bpp画像 | 実テキストマスク | 有効なJBIG2出力 |
| MRCパイプライン | モックsegmenter/encoder | 実ビットマップ | 再構成PSNRが30dB超 |
| PDF構築 | 最小MRC PDF | フルパイプライン出力 | Acrobat/Chromeで開ける |
| キャッシュ | ハッシュ安定性、 | 2パス実行 | 2回目がキャッシュヒット |
| | store/load | | |
| 最適化 | フォント削除、圧縮 | サイズ比較 | Acrobat保存の1.5倍以内 |
| E2E | — | 実PDF全工程 | テキスト除去 + 視覚品質 + サイズ |
