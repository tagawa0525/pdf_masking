# リファクタリング実装計画: コードレビュー結果の反映

## Context

Phase 1-13 の TDD 開発完了後のコードレビューで、3つの横断的構造
問題と複数の個別課題が発見された。レビュー結果
(`docs/plans/code-review-findings.md`) に基づき、5つの PR で
リファクタリングを段階的に実施する。

1つの PR を作成 → レビュー → 修正 → マージの順で完了させてから次に進む。

---

## Step 0: レビュー結果ドキュメントのコミット

`docs/plans/code-review-findings.md` を main にマージするための PR を作成。

---

## Phase 14: `#[allow(dead_code)]` 一括削除

**ブランチ:** `refactor/phase14-remove-dead-code-suppressions`

**概要:** 不要な 17箇所の `#[allow(dead_code)]` を削除。lib/bin
境界の問題で、`lib.rs` が全モジュールを `pub mod` で
エクスポートしているため、実際には binary crate (main.rs)
経由で使用されている項目に dead_code 警告が出ていた。

**対象ファイル (削除箇所):**

- `src/error.rs:3,43`
- `src/linearize.rs:11,48`
- `src/pipeline/orchestrator.rs:7`
- `src/pipeline/job_runner.rs:15,28,41`
- `src/pipeline/page_processor.rs:11,25`
- `src/pdf/optimizer.rs:21,85,120,130`
- `src/mrc/mod.rs:6`
- `src/cache/store.rs:24,52`
- `src/cache/hash.rs:12,38`

**残す箇所 (実際の dead code):**

- `src/pdf/image_xobject.rs:16` — Phase 7+ スタブ

**手順:**

1. 全17箇所の `#[allow(dead_code)]` を削除
2. `cargo check` で警告が出ないことを確認（出た場合は `lib.rs` に `pub use` 再エクスポートを追加）
3. `cargo test` で全テスト通過を確認

**コミット:** `refactor: remove 17 unnecessary #[allow(dead_code)] suppressions`

---

## Phase 15: 個別コード品質修正

**ブランチ:** `refactor/phase15-isolated-quality-fixes`

**概要:** 小規模・独立・低リスクな修正をまとめて実施。

### 15a: `validate_cache_key` の大文字 hex 許容修正

- **ファイル:** `src/cache/store.rs:42`
- **問題:** `is_ascii_hexdigit()` が `A-F` も許容するが、`compute_cache_key` は小文字のみ生成
- **修正:** `matches!(b, b'0'..=b'9' | b'a'..=b'f')` に変更
- **TDD:** RED (大文字キーを拒否するテスト追加) → GREEN (バリデーション修正)

### 15b: `cache/hash.rs` 手動 JSON → `serde_json` + `BTreeMap`

- **ファイル:** `src/cache/hash.rs:22-30`
- **問題:** `format!()` による手書き JSON。フィールド追加時に脆い
- **修正:** `BTreeMap<&str, serde_json::Value>` で構築し `serde_json::to_string()` で出力
- **既存テストでキー順序・値の正しさを検証**

### 15c: `MrcLayers` に `Debug` derive 追加

- **ファイル:** `src/mrc/mod.rs:6`
- **修正:** `#[derive(Debug)]` を追加

### 15d: `leptonica.rs` i32 境界チェック共通化 + 不要束縛削除

- **ファイル:** `src/ffi/leptonica.rs`
- **修正1:** `fn validate_i32_dimensions()` を抽出 (41, 91行)
- **修正2:** 178行の変数代入を `pixDestroy()` に簡略化

### 15e: `mrc/jpeg.rs` 可視性統一

- **ファイル:** `src/mrc/jpeg.rs`
- **修正:** `encode_rgb_to_jpeg` を `pub(super)` → `pub(crate)` に変更

### 15f: `image_xobject.rs` スタブ整理

- **ファイル:** `src/pdf/image_xobject.rs:14-19`
- **修正:** コメントで Phase 7+ TODO であることを明記。テストはそのまま維持

**コミット:**

1. `test(cache): add test for uppercase hex rejection in validate_cache_key`
2. `fix(cache): restrict validate_cache_key to lowercase hex only`
3. `refactor(cache): replace manual JSON with serde_json`
4. `refactor: add Debug, fix binding, unify visibility, clarify stub`

---

## Phase 16: `writer.rs` XObject メソッド統合

**ブランチ:** `refactor/phase16-writer-xobject-dedup`

**概要:** 3つのメソッドの構造重複を解消。

**ファイル:** `src/pdf/writer.rs`

**設計:** 共通のプライベートメソッド `add_image_xobject` を導入。差分パラメータ:

- `color_space`: `"DeviceRGB"` | `"DeviceGray"`
- `bits_per_component`: `8` | `1`
- `filter`: `"DCTDecode"` | `"JBIG2Decode"`
- `smask_id`: `Option<ObjectId>` (前景のみ)

3つの既存メソッドはこのヘルパーを呼ぶ薄いラッパーとして残す（API互換維持）。

**コミット:** `refactor(writer): extract shared add_image_xobject helper`

---

## Phase 17: エラーハンドリング改善 (`From` trait 実装)

**ブランチ:** `refactor/phase17-error-from-impls`

**概要:** `error.rs` に `From<T>` impl を追加し、14箇所の
`.map_err(|e| ...e.to_string())` を `?` に置換。

**追加する `From` impl:**

- `lopdf::Error` → `PdfReadError` (pdf/reader.rs, 6箇所)
- `serde_json::Error` → `CacheError` (cache/store.rs, 2箇所)
- `serde_yml::Error` → `ConfigError` (config/settings.rs, 1箇所)
- `PdfiumError` → `RenderError` (render/pdfium.rs, 4箇所)
- `image::ImageError` → `JpegEncodeError` (mrc/jpeg.rs, 1箇所)

**対象外 (現状維持):**

- `writer.rs` の7箇所: `|_|` でエラーを破棄しカスタムメッセージを使用
- `cache/store.rs` のIO系10箇所: 既存の `From<io::Error> → IoError` と競合
- `content_stream.rs` の2箇所: `ContentStreamError` は `lopdf::Error` の `From` 先と別 variant
- `config/job.rs` の5箇所: カスタムメッセージ付き
- `optimizer.rs` の2箇所: IO エラーを `PdfWriteError` にラップ

**ファイル:**

- `src/error.rs` — `From` impl 5つ追加
- `src/pdf/reader.rs` — 6箇所の `map_err` → `?`
- `src/cache/store.rs` — 2箇所
- `src/config/settings.rs` — 1箇所
- `src/render/pdfium.rs` — 4箇所
- `src/mrc/jpeg.rs` — 1箇所

**コミット:**

1. `refactor(error): add From impls for external error types`
2. `refactor: replace 14 map_err calls with ? operator`

---

## Phase 18: `String` → `PathBuf` パス型移行

**ブランチ:** `refactor/phase18-pathbuf-migration`

**概要:** 8箇所の `String` パスフィールドを `PathBuf` に移行。コードベース全体の型安全性を向上。

**移行順序 (リーフ → ルート):**

### Step 1: リーフ関数のシグネチャ変更

- `pdf/reader.rs`: `open(path: &str)` → `open(path: impl AsRef<Path>)`
- `render/pdfium.rs`: `render_page(pdf_path: &str, ...)` → `&Path`
- `linearize.rs`: `linearize()` パラメータ → `&Path`

### Step 2: データ構造の変更

- `config/settings.rs`: `cache_dir: String` → `PathBuf`
- `config/merged.rs`: `cache_dir` → `PathBuf`
- `pipeline/job_runner.rs`: `JobConfig` / `JobResult` → `PathBuf`
- `cache/hash.rs`: `pdf_path: &str` → `&Path`
- `pipeline/page_processor.rs`: `pdf_path` パラメータ → `&Path`

### Step 3: main.rs の更新

- `resolve_path()`: `String` → `PathBuf` 返却
- `eprintln!` で `path.display()` を使用

**注意:**

- `serde_yml` は `PathBuf` のデシリアライズに対応済み
- cache key のハッシュ入力: Linux では
  `Path::as_os_str().as_encoded_bytes()` ≡ `str::as_bytes()`
  なのでハッシュ互換性に問題なし
- テストの呼び出し側も合わせて修正

---

## 検証方法

各 PR マージ後:

```sh
cargo check          # コンパイルエラーなし
cargo test           # 全テスト通過
cargo clippy         # 警告なし（可能な限り）
```

全 PR マージ後:

```sh
cargo test           # 回帰なし
cargo clippy         # クリーン
```

## マージ順序

```txt
Phase 14 → Phase 15 → Phase 16 → Phase 17 → Phase 18
```

Phase 14-16 は相互に独立。Phase 17 は Phase 14 の後が望ましい
（error.rs の変更が衝突しないため）。Phase 18 は最後（最大規模、
全モジュールに波及）。
