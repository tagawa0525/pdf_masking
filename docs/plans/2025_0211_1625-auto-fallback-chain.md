# `preserve_images` / `text_to_outlines` オプション削除 + 自動フォールバックチェーン導入

Created: 2025-02-11 16:25

## Context

現在、`preserve_images` と `text_to_outlines` の2つのオプションでユーザーが処理経路を手動選択している。しかし:

- `text_to_outlines` はフォールバック付きなので、常に試行して問題ない
- `preserve_images=false`（全面MRC）はユーザーが明示選択する必要がなく、`compose_text_masked` 失敗時の最終手段として機能させれば十分

2つのオプションを削除し、常に最良の方法から順に試行する自動フォールバックチェーンに変更する。

### 変更後のフォールバックチェーン

**Rgb/Grayscale:**

```text
A2: compose_text_outlines (ベクターパス変換、レンダリング不要)
  → 失敗 →
B+C: compose_text_masked (テキスト領域だけJPEG化、画像保持)
  → 失敗 →
B+C: compose (全面ラスタライズ、3層MRC)
```

**Bw:**

```text
A2: compose_text_outlines + force_bw
  → 失敗 →
B+C: compose_bw (全面JBIG2)
```

## コミット構成

### Commit 1 (RED): フォールバック動作のテストを追加

`tests/pipeline_test.rs` に `#[ignore]` 付きテストを追加:

- `test_process_page_text_masked_fallback_to_compose`: `compose_text_masked` が失敗する入力で `process_page` を呼び、`PageOutput::Mrc` が返ることを検証する
  - 不正なコンテンツストリーム（`strip_text_operators` がエラーを返す）+ 正常なビットマップを渡す
  - 現状の `process_page` にはこのフォールバックが無いため、テストは失敗する（`#[ignore]` で保護）

### Commit 2 (GREEN): オプション削除 + フォールバック実装 + テスト更新

Rust はフィールド削除時にコンパイルエラーが出るため、設定・パイプライン・テストを一括で変更する必要がある。

#### 2-1. 設定レイヤーからオプション削除

| ファイル | 変更内容 |
| --- | --- |
| `src/config/settings.rs` | `Settings` からフィールドと `Default` の値を削除 |
| `src/config/job.rs` | `Job` からフィールドを削除 |
| `src/config/merged.rs` | `MergedConfig` からフィールドとマージロジックを削除 |
| `src/main.rs` | `JobConfig` 構築から該当行を削除 |

#### 2-2. パイプラインの変更

**`src/pipeline/job_runner.rs`:**

- `JobConfig` から `preserve_images`, `text_to_outlines` フィールドを削除
- Phase A (L103-123): 条件ガードを除去し、Rgb/Grayscale/Bw では常に画像ストリーム抽出・フォント解析を実行

  ```rust
  // 変更前
  let image_streams = if config.preserve_images && matches!(mode, ...) { ... }
  let fonts = if config.text_to_outlines && config.preserve_images && ... { ... }

  // 変更後
  let image_streams = if matches!(mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Bw) { ... }
  let fonts = if matches!(mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Bw) {
      crate::pdf::font::parse_page_fonts(...).ok()
  } else { None };
  ```

- Phase A2 (L140-146): 適格条件を簡略化

  ```rust
  // 変更前
  let eligible = config.text_to_outlines && config.preserve_images
      && matches!(cs.mode, ...) && cs.fonts.is_some();

  // 変更後
  let eligible = matches!(cs.mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Bw)
      && cs.fonts.is_some();
  ```

- Phase C (L210-221): `process_page` 呼び出しから `false, None` を削除

**`src/pipeline/page_processor.rs`:**

- `process_page()` から `text_to_outlines: bool` と `fonts: Option<&HashMap<String, ParsedFont>>` パラメータを削除
- `compose_text_outlines` / `TextOutlinesParams` / `ParsedFont` のインポートを削除
- Rgb/Grayscale の分岐を書き換え:

  ```rust
  // 変更前
  if cache_settings.preserve_images {
      let outlines_result = if text_to_outlines { ... };
      if let Some(data) = outlines_result { ... } else { compose_text_masked(...)? }
  } else {
      compose(...)
  }

  // 変更後
  match compose_text_masked(&params) {
      Ok(data) => PageOutput::TextMasked(data),
      Err(_) => {
          let mrc_layers = compose(&rgba_data, width, height, mrc_config, mode)?;
          PageOutput::Mrc(mrc_layers)
      }
  }
  ```

**`src/cache/hash.rs`:**

- `CacheSettings` から `preserve_images` フィールドを削除
- `settings_to_canonical_json()` から `preserve_images` エントリを削除

#### 2-3. テスト更新

**`tests/config_test.rs`:**

- 削除するテスト（7件）: `test_settings_text_to_outlines_*` 系3件、`test_job_text_to_outlines_*` 系2件、`test_merge_text_to_outlines_*` 系2件 + `test_merge_text_to_outlines_default`
- 既存テストから `preserve_images` / `text_to_outlines` のアサーションと YAML 記述を除去

**`tests/cache_test.rs`:**

- 全 `CacheSettings` インスタンスから `preserve_images` フィールドを削除（約7箇所）
- `test_cache_key_differs_with_different_settings` でpreserve_imagesの差異テストを別フィールドに変更

**`tests/pipeline_test.rs`:**

- 全 `CacheSettings` インスタンスから `preserve_images` を削除
- 全 `process_page()` 呼び出しから末尾2引数 (`false`/`true`, `None`/`Some(...)`) を削除
- 削除するテスト:
  - `test_process_page_mrc_when_preserve_images_false` —
    MRC 強制は不可能。既存テストで十分なら削除
  - `test_process_page_text_to_outlines_with_fonts` — process_page 内での outlines 試行はもうない（Phase A2 に移動済み）。`process_page_outlines` の既存テストでカバー済み
  - `test_process_page_text_to_outlines_fallback_on_missing_font` — 同上
- Commit 1 の `#[ignore]` テストを有効化

**`tests/e2e_test.rs`:**

- `write_settings_yaml` ヘルパーから `preserve_images` パラメータを削除
- `test_e2e_with_settings_yaml`: settings.yaml が適用されることの検証を `dpi` や `bg_quality` など残存オプションで行うよう変更。BgImg/FgImg のアサーションを削除し、出力 PDF が有効であることの検証に留める
- `test_e2e_output_has_mrc_structure`: TextMasked 出力の検証に変更。BgImg/FgImg アサーションを削除し、出力 PDF が有効で1ページあることを検証。テスト名を `test_e2e_output_structure` にリネーム
- `test_e2e_grayscale_mode`: `preserve_images: false` を除去。TextMasked 出力になるため BgImg/FgImg/DeviceGray アサーションを削除し、出力 PDF が有効で1ページであることの検証に変更
- `test_e2e_mixed_mode`: `preserve_images: false` を除去。Page 1 (RGB) の BgImg アサーションを削除（TextMasked 出力になる）。Page 2 (BW) の BwImg アサーションは維持

### Commit 3: サンプルファイル・ドキュメント更新

| ファイル | 変更内容 |
| --- | --- |
| `sample/jobs.yaml` | `text_to_outlines: true` 行を削除 |
| `sample/test_bw.yaml` | `text_to_outlines: true` 行を削除 |
| `docs/processing-flow.md` | 新しいフォールバックチェーンを反映して書き換え |
| `CLAUDE.md` | Phase A2 の説明からオプション名への言及を除去 |

## 変更対象ファイル一覧

| ファイル | 変更の性質 |
| --- | --- |
| `src/config/settings.rs` | フィールド2つ + デフォルト値2つを削除 |
| `src/config/job.rs` | フィールド2つを削除 |
| `src/config/merged.rs` | フィールド2つ + マージロジック2行を削除 |
| `src/main.rs` | JobConfig構築から2行を削除 |
| `src/pipeline/job_runner.rs` | JobConfigフィールド削除、Phase A/A2の条件簡略化、process_page呼び出し更新 |
| `src/pipeline/page_processor.rs` | process_page パラメータ削除、text_masked→compose フォールバック追加 |
| `src/cache/hash.rs` | CacheSettingsフィールド削除、JSON生成更新 |
| `tests/config_test.rs` | テスト7件削除、既存テスト5件更新 |
| `tests/pipeline_test.rs` | テスト2-3件削除、残り全件のCacheSettings/process_page引数更新、フォールバックテスト追加 |
| `tests/cache_test.rs` | CacheSettings約7箇所更新 |
| `tests/e2e_test.rs` | ヘルパー更新、テスト4件の構造変更 |
| `sample/jobs.yaml` | 1行削除 |
| `sample/test_bw.yaml` | 1行削除 |
| `docs/processing-flow.md` | 全体書き換え |
| `CLAUDE.md` | パイプライン説明更新 |

## 後方互換性

`Settings` / `Job` の serde には `deny_unknown_fields` が設定されていないため、古い YAML に `preserve_images` や `text_to_outlines` が残っていても黙って無視される。パースエラーにはならない。

## 検証手順

```bash
cargo fmt --check      # フォーマット確認
cargo clippy            # リント
cargo test              # 全テスト (pdfium が必要な e2e テストは環境依存)
```

`nix develop` 環境内で `cargo test` が全件パスすることを確認する。
