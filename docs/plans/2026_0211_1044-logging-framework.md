# feat: logging-framework 導入計画

## Context

PR #60 の Copilot レビューで `eprintln!` の使用について指摘があった。
プロジェクト全体でログフレームワークを導入し、`RUST_LOG` 環境変数による
出力制御を可能にする。

## ライブラリ選定

**`tracing` + `tracing-subscriber`**

- `tracing` は Rust エコシステムの標準的な計装ライブラリ
- `tracing-subscriber` の `fmt` レイヤーで stderr 出力（現行動作と一致）
- `RUST_LOG` 環境変数による `EnvFilter` でログレベル制御
- 将来的に span ベースの構造化ログにも拡張可能

## 変換対象

### 変換する（tracing マクロに置換）

| ファイル | 行 | 現在 | 変換先 |
| --- | --- | --- | --- |
| `src/main.rs` | 34 | `eprintln!("ERROR: {e}")` | `error!("{e}")` |
| `src/main.rs` | 115-120 | `eprintln!("OK: ...")` | `info!(...)` |
| `src/main.rs` | 126-129 | `eprintln!("ERROR: ...")` | `error!(...)` |
| `src/main.rs` | 134-138 | `eprintln!("ERROR: ... {e}")` | `error!(...)` |
| `src/pdf/font.rs` | 33-37 | `eprintln!("Warning: ...")` | `warn!(...)` |
| `src/pipeline/page_processor.rs` | 222 | `eprintln!(...)` | `warn!(...)` |

### 変換しない（CLI インターフェース出力）

| ファイル | 行 | 理由 |
| --- | --- | --- |
| `src/main.rs` | 16-17 | `--help` 出力。常に表示されるべき |
| `src/main.rs` | 26 | `--version` 出力。常に表示されるべき |

## サブスクライバー初期化

`main()` 内で `--help`/`--version` ガードの後に配置:

```rust
use tracing_subscriber::EnvFilter;

tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env()
        .add_directive(tracing::Level::INFO.into()))
    .with_target(false)
    .with_level(true)
    .without_time()
    .with_writer(std::io::stderr)
    .init();
```

- `without_time()`: CLI ツールにタイムスタンプは不要
- `with_target(false)`: モジュールパスを非表示（CLI 出力を簡潔に）
- `with_level(true)`: `ERROR`, `WARN`, `INFO` プレフィックスを表示
- デフォルト `info` レベル、`RUST_LOG` で上書き可能

## 出力形式の変更

tracing-subscriber の fmt デフォルト:

```text
 ERROR {message}
  WARN {message}
  INFO {message}
```

既存テスト `test_main_nonexistent_job_file` は
`stderr.contains("ERROR")` でチェックしているため、
`ERROR` がプレフィックスに含まれていれば互換性あり。

## コミット計画（TDD）

### Commit 1 (RED): RUST_LOG フィルタリングのテスト追加

- `tests/cli_test.rs` に
  `test_rust_log_off_suppresses_error_output` を追加
- `RUST_LOG=off` でバイナリを実行し、
  stderr に "ERROR" が含まれないことをアサート
- 現状の `eprintln!` では失敗する（RED）

### Commit 2 (GREEN): tracing 導入、eprintln! 変換

- `Cargo.toml` に `tracing`, `tracing-subscriber`
  (features = ["env-filter"]) を追加
- `src/main.rs`: サブスクライバー初期化、
  4箇所の eprintln! を tracing マクロに変換
- `src/pdf/font.rs`: 1箇所を `warn!` に変換
- `src/pipeline/page_processor.rs`: 1箇所を `warn!` に変換
- Commit 1 のテストがパスする（GREEN）

### Commit 3 (REFACTOR): パイプラインに debug ログ追加（任意）

- `src/pipeline/job_runner.rs` のフェーズ開始時に `debug!` を追加
- デフォルトの `info` レベルでは表示されない
- `RUST_LOG=debug` で診断情報として確認可能

## 変更ファイル一覧

- `Cargo.toml`
- `src/main.rs`
- `src/pdf/font.rs`
- `src/pipeline/page_processor.rs`
- `tests/cli_test.rs`
- `src/pipeline/job_runner.rs`（Commit 3、任意）

## 検証方法

```bash
# 全テストが通ること
cargo test

# RUST_LOG=off でエラー出力が抑制されること
RUST_LOG=off cargo run -- nonexistent.yaml 2>&1 | grep -c ERROR
# → 0

# RUST_LOG=debug で詳細ログが表示されること（Commit 3 後）
RUST_LOG=debug cargo run -- sample/jobs.yaml 2>&1

# 既存の CLI テストが通ること
cargo test --test cli_test
```
