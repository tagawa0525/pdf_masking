# PDF Masking CLI Tool - 新セッション用プロンプト

以下の要件でCLIツールの実装計画を立ててください。

---

## 目的

PDFファイルの指定ページからテキスト情報を完全に除去するCLIツール。テキストデータを削除しつつ、見た目はMRC圧縮による画像化で保持する。

## 確定済み要件

### MRC圧縮（Mixed Raster Content）

ITU-T T.44準拠の3層構造でページを画像化する。ページ内のテキスト/線画と写真/グラデーションを
**自動セグメンテーション**し、各領域を最適なアルゴリズムで圧縮する。

```text
マスク層（1bit高解像度）  → JBIG2  テキスト/線画のエッジ保持
前景色層（低解像度カラー） → JPEG   テキストの色情報
背景層（カラー）          → JPEG   写真、グラデーション、地色
合成: pixel = mask[x,y] ? foreground[x,y] : background[x,y]
```

品質目標: DocuWorksの画像化と同等のファイルサイズ/品質バランス。
カラーモードの手動指定は不要（MRCが自動判定）。

### 対象PDFの特徴

- ページの9割はB&W文字ページ
- 容量の8割は画像ページ（スキャン画像を含む）
- テキストの上にマスキング用画像オブジェクトが配置されているページがある
- 数千ページ規模

### preserve_images モード（デフォルト: true）

画像XObjectをできるだけ元の品質で保持し、テキスト部分のみMRC化するモード。これは既存ライブラリの機能ではなく本ツール独自のアプリケーションロジック。

**処理手順:**

1. lopdfでコンテンツストリームを解析し、画像XObjectの描画コマンド（`Do /ImX` + CTM）とバウンディングボックスを記録
2. pdfium-renderでページ全体を描画 → フルページビットマップ取得
3. ビットマップ上の画像XObject領域を白で塗りつぶし
4. 白塗り済みビットマップに対してMRCセグメンテーション → 3層生成
5. PDF構造構築: MRC背景層 → MRC前景層(SMask付き) → 画像XObjectコマンド復元

**画像XObjectの改変（セキュリティ要件）:**
マスキング用オブジェクトが画像XObjectの上に重なっている場合、そのオブジェクト下の画像領域を塗りつぶす必要がある（マスキングオブジェクトを移動・削除しても隠された内容が見えないようにする）。

- マスキングオブジェクトのバウンディングボックスと画像XObjectの重なりを判定
- 重なりがある画像XObjectは: デコード → 重なり領域を塗りつぶし → 再エンコード
- 重なりがない画像XObjectはそのまま保持

**ファイルサイズ:** 画像XObject領域はMRC層から除外するため二重格納なし。

### CLI設計

```bash
pdf_masking <jobs.yaml>...
```

- 位置引数のみ（1つ以上のジョブファイルパス）
- `--version` / `--help` のみ
- CLIフレームワークは不要（引数がシンプルなため）

### ファイル構成（設定とジョブを分離、YAML形式）

TOMLではページ範囲指定に`""`が必要だが、YAMLなら不要。

**設定ファイル**（オプション）: `settings.yaml`

- ジョブファイルと同じディレクトリに存在すれば自動読込。
  なければデフォルト値を使用
- 設定項目: dpi, fg_dpi, bg_quality, fg_quality, parallel_workers,
  cache_dir, preserve_images, linearize

**ジョブファイル**（必須）:

```yaml
jobs:
  - input: path/to/input.pdf
    output: path/to/output.pdf
    pages: [1, 3, 5-10, 15, 20-25]  # 引用符不要

  - input: path/to/another.pdf
    output: path/to/another_masked.pdf
    pages: [1-100]
    dpi: 200                # ジョブ単位で設定オーバーライド可能
    preserve_images: false
```

### PDF最適化

PoCで「Acrobat名前をつけて保存で1/3以下になる」問題が発生済み。対策:

- オブジェクトストリーム（PDF 1.5+）
- 全ストリームのFlateDecode圧縮
- 孤立オブジェクト除去
- マスク済みページのフォントリソース削除
- クロスリファレンスストリーム
- qpdf C APIでリニアライズ（Web最適化）

### 増分処理（キャッシュ）

「前回と少しだけ変わったPDF」を効率的に再処理する。

- ページ単位でコンテンツストリームのSHA-256ハッシュを比較
- 変更なしページはキャッシュからMRC層を取得（描画・セグメンテーション・エンコード全スキップ）
- 設定変更時はキャッシュ無効化

### 使用ライブラリ候補

| ライブラリ | 用途 | ライセンス |
| --- | --- | --- |
| pdfium-render | ページ→画像描画 | Apache-2.0 |
| MuPDF (C) | ページ→画像描画（代替候補） | AGPL-3.0 |
| lopdf | PDF構造操作（解析・置換、XObject/SMask） | MIT |
| leptonica (C) | MRCセグメンテーション（テキスト/画像分離） | BSD-2 |
| jbig2enc (C) | マスク層→JBIG2エンコード | Apache-2.0 |
| qpdf (C API) | リニアライズ | Apache-2.0 |

※ CライブラリのFFI統合が複数必要。言語選択時の考慮点。AGPLは問題なし。
※ MuPDFを使う場合、pdfium-renderは不要（MuPDFがPDF描画を担当）。MuPDFはZigとの親和性が高い。

### PDF上のMRC表現

SMaskベースの3層合成:

- 背景XObject: DCTDecode（JPEG）
- 前景XObject: DCTDecode（JPEG）、`/SMask`でマスクXObjectを参照
- マスクXObject: JBIG2Decode、1bit DeviceGray

### エラー処理

- thiserrorのみ使用（anyhow不使用）
- テスト以外でunwrap禁止。全て?演算子で伝播

### 開発プロセス

- TDD: RED（テスト先行コミット）→ GREEN（実装コミット）→ REFACTOR
- Git: mainに直接コミットしない。feature branch → PR → マージ
- 1コミット1論理変更

## 未確定事項（計画時に検討）

- **実装言語**: Rust（好み、ただしC FFIが複数必要）vs Zig（C統合が容易、MuPDF/Leptonicaと親和性高い）vs その他
- **PDF描画ライブラリ**: pdfium-render（Rust向け）vs MuPDF（AGPL、C、Zig親和性高い）。MuPDFはPDF操作機能も持つためlopdf不要の可能性あり
- **スキャン画像のセグメンテーション品質**: preserve_images=false時にスキャン画像を含むページ全体をMRCセグメンテーションする場合、品質が安定するか要検討。preserve_images=true時はデジタルテキストのみが対象なので問題なし
- **Leptonicaのセグメンテーション関数の具体的な選定**: pixGenHalftoneMask等の候補あり

## 計画に含めてほしいこと

- モジュール構成
- 実装順序（TDDフェーズ分け）
- 検証方法
- 計画書ファイル名は `YYYY_MMDD_NNNN-{概要}.md` のようにソータブルかつ内容がわかる形式
