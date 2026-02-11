# tracingログ拡充計画

## Context

現在 `tracing` は4ファイル（main.rs, job_runner.rs, page_processor.rs, font.rs）でのみ使用されている。残りの約15モジュールにはログ出力がなく、本番環境でのデバッグや処理状況の把握が困難。各モジュールに適切なレベルのログを追加し、パイプライン全体の可視性を高める。

## TDDの適用について

ログ追加は純粋な可観測性の変更であり、機能的動作を変更しない。
`debug!("foo")` が正しいレベルで出力されるかをテストしても
保守コストに見合わない。既存テストが全コードパスを網羅しているため、
ログ追加によるコンパイルエラーやロジック破壊は既存テストが検出する。
TDDサイクルは適用せず、各コミットで `cargo test` + `cargo clippy` +
`cargo fmt --check` を実行して確認する。

## ログレベル方針

- **error!**: 回復不能な失敗（既にエラー伝播で処理済み、追加不要）
- **warn!**: 予期しないが回復可能な状況（フォールバック、欠落データ）
- **info!**: デフォルトで表示される高レベル進捗（ジョブ開始/終了）
- **debug!**: 診断情報（寸法、件数、サイズ、エンコード選択）

構造化フィールドを使用:
`debug!(page = page_num, width = w, "page dimensions")`

## eprintln! 置き換え

### src/main.rs（3箇所）

tracing_subscriber初期化を `--help`/`--version` チェックの前に移動し、`eprintln!` を `info!` に置き換える。

- L17: `eprintln!("Usage: ...")` → `info!("Usage: ...")`
- L18: `eprintln!("  Process PDF...")` → `info!("  Process PDF...")`
- L27: `eprintln!("pdf_masking {}", ...)` → `info!("pdf_masking {}", ...)`

### tests/（20箇所）

各テスト関数の先頭で
`let _ = tracing_subscriber::fmt().with_test_writer().try_init();`
を呼び、`eprintln!("Skipping: ...")` / `eprintln!("SKIP: ...")` を
`warn!("...")` に置き換える。

対象ファイル:

- `tests/e2e_test.rs`: 9箇所（L239, L279, L324, L391, L435, L507, L639, L682, L730）
- `tests/font_parser_test.rs`: 6箇所（L198, L222, L242, L332, L356, L437）
- `tests/text_to_outlines_test.rs`: 1箇所（L73）
- `tests/linearize_test.rs`: 4箇所（L58, L80, L112, L135）

## コミット計画（9コミット）

### 1. main.rs の eprintln! を tracing に置き換え

tracing_subscriber初期化を `--help`/`--version` の前に移動し、`eprintln!` → `info!` に変更。

### 2. テストの eprintln! を tracing に置き換え

4つのテストファイルで `eprintln!` → `warn!` に変更、各テスト関数に tracing_subscriber 初期化を追加。

### 3. パイプラインオーケストレーション層

**対象**: `orchestrator.rs`, `job_runner.rs`(拡充), `page_processor.rs`(拡充)

- `orchestrator.rs`: `info!` でジョブ総数と完了サマリ（成功/失敗件数）
- `job_runner.rs`: `phase_a2_text_to_outlines` 内でページごとの成功/フォールバック情報を `debug!`
- `page_processor.rs`: キャッシュヒット/ミスを `debug!`（outlinesパス・MRCパス両方）

### 4. PDF読み込み・コンテンツ解析層

**対象**: `src/pdf/reader.rs`, `src/pdf/content_stream.rs`

- `reader.rs`: `open()` で `debug!` (ファイルパス),
  `page_xobject_names()` / `page_image_streams()` で件数
- `content_stream.rs`: `extract_xobject_placements()` /
  `strip_text_operators()` / `extract_white_fill_rects()` で結果件数を
  `debug!`

### 5. テキスト→アウトライン変換・フォント解決層

**対象**: `src/pdf/text_to_outlines.rs`, `src/pdf/font.rs`(拡充)

- `text_to_outlines.rs`: `convert_text_to_outlines()` 完了時に生成バイト数を `debug!`
- `font.rs`: `parse_page_fonts()` でフォント数を `debug!`, システムフォント解決時のフォールバックを `debug!`
- `glyph_to_path.rs`, `text_state.rs`: ホットパス（グリフ/オペレータ単位）のため追加しない

### 6. 画像XObject処理層

**対象**: `src/pdf/image_xobject.rs`

- `redact_image_regions()`: 重複領域数・ピクセル領域数を `debug!`
- `optimize_image_encoding()`: 候補数と選択結果を `debug!`
- 未対応フィルタで `warn!`

### 7. MRC合成・エンコード層

**対象**: `src/mrc/compositor.rs`, `src/mrc/segmenter.rs`

- `compositor.rs`: `compose()` / `compose_bw()` でエンコードサイズを
  `debug!`, `compose_text_masked()` / `compose_text_outlines()` で
  リージョン数を `debug!`
- `segmenter.rs`: `extract_text_bboxes()` でフィルタ前後・マージ後の
  件数を `debug!`
- `jbig2.rs`, `jpeg.rs`: 薄いラッパーのため追加しない
  （呼び出し元でサイズをログ）

### 8. PDF書き出し・最適化・レンダリング層

**対象**: `src/pdf/writer.rs`, `src/pdf/optimizer.rs`,
`src/render/pdfium.rs`

- `writer.rs`: 各 `write_*_page()` / `copy_page_from()` で `debug!`,
  `save_to_bytes()` でサイズ
- `optimizer.rs`: `optimize()` で対象ページ数,
  `remove_fonts_from_pages()` / `compress_streams()` で処理件数
- `pdfium.rs`: ライブラリ読み込みパスとレンダリング寸法を `debug!`

### 9. キャッシュ・リニアライズ層

**対象**: `src/cache/store.rs`, `src/cache/hash.rs`, `src/linearize.rs`

- `store.rs`: `store()` でキャッシュタイプとキー(先頭16文字), `retrieve()` でミス理由
- `hash.rs`: `compute_cache_key()` でキー(先頭16文字)とページ番号
- `linearize.rs`: 入出力パスを `debug!`, qpdf実行失敗を `warn!`

## 追加しないもの

- ログのテスト（保守コストに見合わない）
- trivialな関数（`Matrix::identity()`, `operand_to_f64()` 等）
- ホットループ内のログ（グリフ単位、ピクセル単位、オペレータ単位）
- キャッシュキーの全文表示（先頭16文字に切り詰め）

## 検証方法

```bash
cargo fmt --check && cargo clippy && cargo test
RUST_LOG=debug cargo run -- sample/jobs.yaml 2>debug.log
# debug.log でフェーズ遷移、キャッシュヒット/ミス、エンコードサイズ等が出力されることを確認
```
