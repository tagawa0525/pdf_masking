# コードレビュー結果

## 概要

pdf_masking (~2,700行) の全モジュールをレビュー。
3つの横断的な構造問題が発見され、計42の `.map_err` 呼び出しと
20の不要な `#[allow(dead_code)]` マーカーが確認された。

## 横断的構造問題

### 問題1: `From<T>` trait 不足 → `.map_err` 42箇所

`error.rs` は `std::io::Error` のみ `#[from]` を実装。
他のエラー型は `From` がないため、42箇所で
`map_err(|e| PdfMaskError::xxx(e.to_string()))` が散在。

**対応方針:**

- `pdf/reader.rs` (6) → `From<lopdf::Error>`
- `cache/store.rs` (2-12) → `From<serde_json::Error>` + IO ヘルパー
- 他は variant 意味論の制約で現状維持

### 問題2: `String` パス → `PathBuf` 未使用

パスが全て `String`/`&str` で伝播。
`main.rs` → `JobConfig` → 全関数が `&str` を受け取る。

**対象:**

- `config/job.rs`, `config/settings.rs`
- `main.rs:resolve_path()`
- `render/pdfium.rs`, `linearize.rs` のシグネチャ

### 問題3: `#[allow(dead_code)]` 20箇所 — lib/bin 境界問題

`lib.rs` が全モジュールを `pub` でエクスポート。
ライブラリ単体では内部関数が unused なため20箇所に `#[allow]`。

**実際の dead code (3箇所):**

- `pdf/image_xobject.rs:17` — Phase 7+ スタブ
- `pdf/optimizer.rs:86,121` — `optimize()` 内部

**不要な `#[allow]` (17箇所):**

main.rs 経由で全て使用。削除後も警告なし。

## 個別課題 (優先度別)

### 高: エラーハンドリング簡略化

- cache/store.rs の12箇所 → `From` impl で `?` に
- pdf/reader.rs の6箇所 → 同様

### 中: コード品質

- cache/hash.rs: 手動JSON → `serde_json` + `BTreeMap`
- cache/store.rs:42: 大文字hex許容の不一致
- writer.rs: XObject メソッド3つの重複
- leptonica.rs: i32 境界チェック共通化

### 低: クリーンアップ

- 不要な17箇所の `#[allow(dead_code)]` 削除
- `MrcLayers` に `Debug` derive
- `mrc/jpeg.rs` の可視性統一
- `redact_region_placeholder()` スタブ整理

## 数値サマリ

- `.map_err()` 総数: 42
- `#[allow(dead_code)]`: 20 (うち不要17)
- `String` パスフィールド: 8
- XObject メソッド重複: 3
- テストヘルパー重複: 4ファイル
