# 非埋め込みフォントのシステムフォント解決

Created: 2025-02-11 15:02

## Context

PR #50（CID文字コード修正）と PR #51（BWモード）を実装したが、
サンプル PDF で `text_to_outlines: true` を有効にしてもテキストが
「切れている」問題が残っている。

**根本原因**: `parse_page_fonts` は埋め込みフォント（FontFile2 あり）
のみ解析し、非埋め込みフォント（F1=TimesNewRomanPSMT, F6=ArialMT 等）
をスキップする。`convert_text_to_outlines` がこれらのフォントを参照
すると「font not found」エラーとなり、ページ全体が pdfium
ビットマップフォールバックに落ちる。

**解決策**: 非埋め込みフォントに対してシステムにインストール済みの
フォントファイルを解決・ロードする。`fontdb` クレート
（pure Rust, Linux/macOS/Windows 対応）を使用。

## 前提作業

- `text_to_outlines.rs` の未コミットの passthrough 変更を `git checkout` で破棄する

## 変更対象

### 1. `Cargo.toml` — fontdb 依存追加

```toml
fontdb = "0.22"
```

### 2. `src/pdf/font.rs` — システムフォント解決

**ParsedFont に `face_index` フィールドを追加**

```rust
pub struct ParsedFont {
    font_data: Vec<u8>,
    face_index: u32,  // .ttc コレクション対応
    encoding: FontEncoding,
    widths: HashMap<u16, f64>,
    default_width: f64,
    units_per_em: u16,
}
```

`char_code_to_glyph_id`(L46) と `glyph_outline`(L76) の
`Face::parse(&self.font_data, 0)` を
`Face::parse(&self.font_data, self.face_index)` に変更。

### システムフォントデータベース（LazyLock で初期化）

```rust
static SYSTEM_FONT_DB: LazyLock<fontdb::Database> =
    LazyLock::new(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    });
```

**`resolve_system_font(base_font_name: &str) -> Result<(Vec<u8>, u32)>`**

1. PostScript 名で完全一致検索（Windows ではこれで解決する）
2. 一致しない場合、PostScript 名からファミリ名・スタイルを推定して `fontdb::Query` で検索
   - `TimesNewRomanPSMT` → family "Times New Roman", weight Normal
   - `TimesNewRomanPS-BoldMT` → family "Times New Roman",
     weight Bold
   - `ArialMT` → family "Arial", weight Normal
   - Linux でこれらが無い場合、代替マッピング:
     "Times New Roman" → "Liberation Serif",
     "Arial" → "Liberation Sans"
3. 見つからなければ `Err` を返す
   （フォントスキップ → ページは pdfium フォールバック）

**PostScript 名のパース関数 `parse_ps_name_to_query`**

PostScript 名の一般的なパターンからファミリ名・ウェイト・スタイルを
抽出:

- `-Bold`, `-BoldItalic`, `-Italic`, `-Oblique` サフィックス解析
- `MT`, `PS` サフィックス除去
- CamelCase をスペース区切りに展開
  (例: `TimesNewRoman` → `Times New Roman`)

### フォント辞書ヘルパー

**`resolve_system_font_from_dict(font_dict)`**

フォント辞書から `BaseFont` 名を取得して `resolve_system_font` を
呼ぶヘルパー。戻り値は `Result<(Vec<u8>, u32)>`。

**`parse_truetype_font` の変更（L256-275）**

```rust
// 変更前
let font_data = extract_font_file2(doc, font_dict)?;
// 変更後
let (font_data, face_index) = extract_font_file2(doc, font_dict)
    .map(|data| (data, 0u32))
    .or_else(|_| resolve_system_font_from_dict(font_dict))?;
```

`Face::parse` と `ParsedFont` 構築に `face_index` を使用。

**`parse_type0_font` の変更（L328）**

同様に `extract_font_file2` のフォールバックとして
`resolve_system_font_from_dict(cid_font_dict)` を追加。

### 3. テスト

**`tests/font_parser_test.rs`**

- `test_system_font_resolved_for_non_embedded`: `parse_page_fonts` が
  F1 を返すことを確認。システムフォント無い場合はスキップ
- `test_system_font_glyph_outline_available`: システムフォント解決
  後の ParsedFont で `char_code_to_glyph_id` +
  `glyph_outline` が成功すること

**`tests/text_to_outlines_test.rs`**

- `test_sample_pdf_missing_embedded_font_returns_error` を
  `test_sample_pdf_converts_with_system_fonts` に変更:
  `convert_text_to_outlines` が `Ok` を返すことを確認

### 4. `text_to_outlines.rs` の passthrough ロジック削除

未コミットの `block_passthrough` / `passthrough_ops` 変更を破棄。
システムフォント解決により不要。`render_text_codes` の
「font not found」エラーは従来通りページ全体を pdfium
フォールバックに導く（テキスト選択不可を保証）。

## TDD コミット

1. **RED**: システムフォント解決のテストを追加
   （現在の実装では失敗する）
2. **GREEN**: `fontdb` 依存追加 + `font.rs` にシステムフォント解決
   実装 + `face_index` 追加

## 検証方法

1. `cargo test` — 全テスト通過
2. `cargo clippy` — 警告なし
3. `cargo run -- sample/jobs.yaml`
   （`text_to_outlines: true` 設定済み）で出力 PDF を確認:
   - 全テキストがベクターパスとして描画されること
   - テキスト選択・コピーが不可能であること
   - ブラウザで表示して文字が切れていないこと
