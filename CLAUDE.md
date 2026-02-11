# CLAUDE.md

This file provides guidance to Claude Code when working with code in
this repository.

## プロジェクト概要

PDFの指定ページからテキスト情報を除去し、検索不能・選択不能にするRust CLIツール。視覚的な見た目はMRC（Mixed Raster Content）圧縮またはtext-to-outlines変換により維持する。

## 設計制約

- テキストを検索可能・選択可能なまま残す手法（パススルー、透明テキスト層の保持など）は、プロジェクトの目的と矛盾するため提案・採用しない
- 提案する手法が「テキスト情報の除去」という目的に合致するか、実装前に必ず確認する

## 開発環境

Nix flakeで全依存関係を管理。`direnv allow` 後は自動でシェルが構成される。

```bash
nix develop          # 開発シェルに入る（direnv使用時は不要）
cargo build          # デバッグビルド
cargo build --release
```

Windows の場合は `.\scripts\setup-windows.ps1` で初回セットアップ後、
毎セッション `. .\scripts\env-windows.ps1` を実行。

## ビルド・テスト・リントコマンド

```bash
cargo test                                # 全テスト実行
cargo test --test text_to_outlines_test   # 特定テストファイル
cargo test test_force_bw                  # テスト名で絞り込み
cargo clippy                              # リント
cargo fmt --check                         # フォーマットチェック
cargo fmt                                 # フォーマット適用
```

ビルド時に C++ シム（`csrc/jbig2enc_shim.cpp`）のコンパイルが走る。
`JBIG2ENC_INCLUDE_PATH`、`JBIG2ENC_LIB_PATH`、
`LEPTONICA_INCLUDE_PATH` 環境変数が必要（Nix シェルでは自動設定）。

## アーキテクチャ

### 処理パイプライン（4フェーズ）

```text
入力PDF + jobs.yaml
    ↓
[Phase A] コンテンツ解析
  — ページ内容・フォント・画像XObject検出、ColorModeフィルタリング
    ↓
[Phase A2] text_to_outlines（有効時）
  — フォントからグリフアウトラインを取得
  — BT...ETブロックをベクターパスに変換
  — レンダリング不要、フォント未埋め込み時はpdfiumフォールバック
    ↓
[Phase B] レンダリング（A2未使用時のみ）
  — pdfiumでビットマップ化
    ↓
[Phase C] MRC合成（rayon並列）
  — ビットマップを3層に分解
  — JBIG2+JPEGエンコード
    ↓
[Phase D] PDF組み立て
  — MRC XObjectを出力PDFに挿入
  — フォントサブセット化、オプションでqpdfリニアライズ
    ↓
出力PDF
```

### モジュール構成

- **`pipeline/`** — オーケストレーション。`orchestrator.rs`がジョブループ、`job_runner.rs`が4フェーズ実行、`page_processor.rs`がページ単位処理
- **`pdf/`** — PDF操作の中核。`content_stream.rs`（オペレータパース・行列計算）、`text_to_outlines.rs`（テキスト→パス変換）、`font.rs`（フォント抽出）、`glyph_to_path.rs`（グリフ→PDFパス）、`reader.rs`/`writer.rs`（入出力）
- **`mrc/`** — MRC合成。`compositor.rs`（RGB/BW/TextOutlines各モード）、`segmenter.rs`（前景・背景分離）、`jbig2.rs`/`jpeg.rs`（エンコーダ）
- **`ffi/`** — C/C++ライブラリのFFI。`*_sys.rs`（raw bindings）と安全ラッパーの2層構造
- **`config/`** — YAML設定。`job.rs`（ジョブ定義）、`settings.rs`（グローバル設定）、`merged.rs`（マージロジック）
- **`cache/`** — SHA-256ベースのページキャッシュ（JSON永続化）

### 主要な外部依存

| ライブラリ | 用途 |
| --- | --- |
| `lopdf` | PDF低レベル操作（辞書・ストリーム） |
| `pdfium-render` | PDFレンダリング（ビットマップ化） |
| `leptonica` | 画像前処理（C FFI経由） |
| `jbig2enc` | JBIG2エンコード（C++ シム） |
| `ttf-parser` | TrueTypeフォント解析 |
| `rayon` | Phase C の並列処理 |
| `qpdf` (CLI) | リニアライズ（Web 最適化） |

### ColorMode

ページごとに処理モードを指定可能: `Rgb`（3層MRC）、`Grayscale`（グレースケールMRC）、`Bw`（JBIG2のみ）、`Skip`（無処理コピー）。

### エラーハンドリング

`PdfMaskError` enum（`thiserror`）に集約。各モジュール固有のエラーは`From`実装で自動変換。`?`演算子で伝播。

## テスト構成

全テストは `tests/` ディレクトリの統合テスト。テストPDFは `sample/pdf_test.pdf` を使用。Rust editionは2024、MSRVは1.93。
