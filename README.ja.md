# pdf_masking

指定したPDFページからテキスト情報を除去し、
MRC（Mixed Raster Content）圧縮で見た目を維持するCLIツールです。

対象ページをビットマップにレンダリングし、ITU-T T.44準拠の3レイヤーに分解します。
テキストは検索・選択不可になりますが、文書の見た目は保たれます。

## 処理の仕組み

各ページは4フェーズのパイプラインで処理されます：

1. **コンテンツ解析** - PDF構造を解析し、画像XObjectを検出
2. **レンダリング** - pdfiumで指定DPIのビットマップに変換
3. **MRC合成**（並列処理） - ビットマップを3レイヤーに分解：
   - **マスク**: 1ビットのテキスト・線画レイヤー（JBIG2エンコード）
   - **前景**: 低解像度のテキスト色情報（JPEG）
   - **背景**: フルカラーの画像コンテンツ（JPEG）
4. **PDF組み立て** - MRC XObjectで最適化された出力PDFを生成

SHA-256ベースのキャッシュにより、変更のないページは再処理をスキップします。

## 必要環境

- システムライブラリ: leptonica, jbig2enc, qpdf, pdfium
- **Linux/macOS**: [Nix](https://nixos.org/)（推奨）
- **Windows**: Visual Studio Build Tools, CMake, Git

## セットアップ

### Linux / macOS (Nix)

```bash
nix develop    # 全依存関係を含む開発シェルに入る
cargo build --release
```

### Windows

PowerShellスクリプトで依存ライブラリのインストールを自動化しています：

```powershell
.\scripts\setup-windows.ps1       # 初回のみ: 依存ライブラリのダウンロードとビルド
. .\scripts\env-windows.ps1       # 毎セッション: 環境変数の読み込み
cargo build --release
```

スクリプトは以下を実行します：

1. [leptonica](https://github.com/DanBloomberg/leptonica) を
   [vcpkg](https://github.com/microsoft/vcpkg) 経由でインストール
   （静的ライブラリ、`x64-windows-static-md` トリプレット）
2. [jbig2enc](https://github.com/agl/jbig2enc) をソースからクローンし
   CMake でビルド
3. [pdfium](https://pdfium.nicehash.com/) のプリビルドバイナリをダウンロード
4. 必要な環境変数を設定する `scripts/env-windows.ps1` を生成

qpdf はデフォルト設定では必須です（`winget install qpdf.qpdf`）。ジョブ設定で線形化を無効にする場合は `linearize: false` を指定してください。

**前提条件**: Visual Studio 2019以降（C++ワークロード）、CMake 3.20以上、Git、Rust（rustup経由）

## 使い方

```bash
pdf_masking <jobs.yaml> [<jobs.yaml>...]
```

### ジョブファイル

YAMLで処理ジョブを定義します：

```yaml
jobs:
  - input: path/to/input.pdf
    output: path/to/output.pdf
    pages: [1, 3, "5-10", 15]
    dpi: 300
    bg_quality: 50
    fg_quality: 30
    preserve_images: true
    linearize: true
```

| フィールド | 必須 | 説明 |
| --- | --- | --- |
| `input` | はい | 入力PDFのパス |
| `output` | はい | 出力PDFのパス |
| `pages` | はい | マスク対象ページ（1始まり）。単一: `5`、範囲: `"5-10"`、 |
| | | 混合: `[1, 3, "5-10"]` |
| `dpi` | いいえ | レンダリング解像度（デフォルト: 300） |
| `bg_quality` | いいえ | 背景JPEG品質 1-100（デフォルト: 50） |
| `fg_quality` | いいえ | 前景JPEG品質 1-100（デフォルト: 30） |
| `preserve_images` | いいえ | 元の画像XObjectを保持する（デフォルト: true） |
| `linearize` | いいえ | 出力PDFをWeb最適化する（デフォルト: true） |

### 設定ファイル

ジョブファイルと同じディレクトリに `settings.yaml` を配置すると、デフォルト値を設定できます：

```yaml
dpi: 300
fg_dpi: 100
bg_quality: 50
fg_quality: 30
parallel_workers: 0     # 0 = 自動（CPU数）
cache_dir: .cache
preserve_images: true
linearize: true
```

ジョブレベルの値は設定ファイルの値を上書きします。未指定の項目には組み込みデフォルト値が使われます。

## 開発

### Linux / macOS

```bash
nix develop
cargo test              # 全テスト実行
cargo clippy            # リント
cargo fmt               # フォーマット
```

### Windows 開発

```powershell
. .\scripts\env-windows.ps1
cargo test
cargo clippy
cargo fmt
```

## ライセンス

本プロジェクトはまだライセンスが設定されていません。
