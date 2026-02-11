# 処理フローとフォールバック構造

PDFページからテキスト情報を除去する方法が複数あり、最適な方法が失敗したとき次善の方法に自動的に切り替わる。この文書ではその判断ロジックを整理する。

## 全体像

処理は4つのフェーズで構成される。ページごとに「どのフェーズを通るか」が異なる。

```text
入力PDF
  |
  +-- [Skip] -------> そのままコピー (Phase D へ直行)
  |
  +-- [非Skip] -----> Phase A: コンテンツ解析
                         |
                         +-- フォント取得成功
                         |         |
                         |    Phase A2: テキスト→ベクターパス変換
                         |         |
                         |         +-- 成功 --------> Phase D (レンダリング不要)
                         |         |
                         |         +-- 失敗 -------+
                         |                         |
                         +-- フォント取得失敗 -----+
                                                   |
                                              Phase B: pdfium レンダリング
                                                   |
                                              Phase C: MRC 合成
                                                   |
                                              Phase D: PDF 組み立て
```

**ポイント**: Phase A2 が成功すればレンダリング(B)もMRC合成(C)も不要。失敗したときだけ B→C に進む。

---

## フォールバックチェーン

### Rgb/Grayscale

```text
Phase A2: compose_text_outlines (ベクターパス変換、レンダリング不要)
  → 失敗 →
Phase B+C: compose_text_masked (テキスト領域だけJPEG化、画像保持)
  → 失敗 →
Phase B+C: compose (全面ラスタライズ、3層MRC)
```

### Bw

```text
Phase A2: compose_text_outlines + force_bw
  → 失敗 →
Phase B+C: compose_bw (全面JBIG2)
```

---

## Phase A: コンテンツ解析

> `job_runner.rs`

すべての非Skipページに対して実行される。

```text
各ページについて:
  1. コンテンツストリームを抽出
  2. Rgb/Grayscale/Bw → 画像XObjectストリームを抽出
  3. Rgb/Grayscale/Bw → フォント情報を解析 (.ok() で失敗を許容)
```

フォント解析が失敗した場合、`fonts = None` となり Phase A2 の対象外になる。ジョブ全体は止まらない。

---

## Phase A2: テキスト→アウトライン変換

> `job_runner.rs`, `compositor.rs`

### 適格条件

以下の**すべて**を満たすページだけが Phase A2 の対象になる:

```text
ColorMode = Rgb | Grayscale | Bw
  AND fonts が取得できている (Some)
```

### 処理内容

```text
コンテンツストリーム + フォントマップ
  |
  v
convert_text_to_outlines()
  テキスト描画命令 (BT...ET) をフォントのグリフアウトラインに基づいて
  ベクターパス (m, l, c, h, f 等) に書き換える
  |
  v
画像リダクション (白矩形で隠された領域を検出・ゼロ埋め)
  |
  v
TextMaskedData {
  stripped_content_stream: テキストがパスに変換済みのストリーム,
  text_regions: [] (空 - JPEGクロップは不要),
  modified_images: リダクション済み画像
}
```

### フォールバック発生条件

Phase A2 で `Err` が返ると、そのページは Phase B に回される:

```rust
match result {
    Ok(page) => outlines_pages.push(page),  // 成功: レンダリング不要
    Err(_) => needs_rendering.push(cs),     // 失敗: Phase B へ
}
```

Err が返る具体的なケースは [フォント解決の詳細](#フォント解決の詳細) を参照。

---

## Phase B → C: レンダリング + MRC合成

Phase A2 の対象外、または Phase A2 が失敗したページがここに来る。

### Phase B: レンダリング

> `job_runner.rs`

```text
PDF + ページ番号 + DPI
  → pdfium でビットマップ (RGBA) を生成
```

失敗するとジョブ全体がエラーになる（フォールバックなし）。

### Phase C: MRC合成

> `page_processor.rs`

ColorMode に応じて処理が分岐する。

```text
                          Phase C の分岐
                              |
                    +---------+---------+
                    |                   |
              Rgb/Grayscale          Bw モード
                    |                   |
              [フォールバック]      compose_bw()
              下記参照            JBIG2 のみ
```

#### Rgb/Grayscale のフォールバック

```text
compose_text_masked()
  テキスト部分だけラスタライズ、画像保持
  |
  +-- 成功 → TextMaskedData
  |
  +-- 失敗 → compose() (全面MRC 3層)
```

**compose_text_masked** の処理:

```text
コンテンツストリーム + RGBA ビットマップ
  |
  1. BT...ET を除去したストリームを生成
  2. 白色矩形を検出
  3. 画像配置を取得、重なる白矩形で画像をリダクション
  4. テキストマスクからテキスト領域の矩形を抽出
  5. テキスト領域をビットマップからクロップし JPEG 化
  |
  v
TextMaskedData {
  stripped_content_stream: テキスト除去済み,
  text_regions: [JPEG画像 + 座標, ...],
  modified_images: リダクション済み画像
}
```

**compose (全面MRC)** は compose_text_masked 失敗時の最終手段:

```text
RGBA ビットマップ
  → テキストマスク生成 (1-bit)
  → JBIG2 エンコード (マスク層)
  → JPEG エンコード (前景・背景層)
  → MrcLayers (3層構造)
```

---

## Phase D: PDF組み立て

> `job_runner.rs`

処理結果の型に応じて出力方法が決まる:

| PageOutput の型 | 書き出し方法 | どの経路で生成されるか |
| --- | --- | --- |
| `Mrc(MrcLayers)` | `write_mrc_page` | Phase C compose() |
| `BwMask(BwLayers)` | `write_bw_page` | Phase C compose_bw() |
| `TextMasked(...)` | `write_text_masked_page` | Phase A2 または Phase C |
| `Skip(SkipData)` | `copy_page_from` | Skipモード (無加工コピー) |

---

## フォント解決の詳細

> `font.rs`

Phase A2 の成否はフォントが取得できるかに大きく依存する。フォント解決は以下の順序で試行される:

```text
フォント辞書のエントリ
  |
  v
1. FontFile2 埋め込みデータを抽出
  |
  +-- 成功 → そのデータを使用
  |
  +-- 失敗 (埋め込みなし)
        |
        v
2. PostScript名でシステムフォントを完全一致検索
     例: "ArialMT" → fontdb で PS名一致を探す
  |
  +-- 成功 → システムフォントを使用
  |
  +-- 失敗
        |
        v
3. PS名をファミリ+スタイルに分解して fontdb で検索
     例: "TimesNewRomanPS-BoldItalic"
       → family="Times New Roman", weight=Bold, italic=true
  |
  +-- 成功 → システムフォントを使用
  |
  +-- 失敗
        |
        v
4. Linux 代替フォントマッピング
     "Times New Roman" → "Liberation Serif"
     "Arial"           → "Liberation Sans"
     "Courier"         → "Liberation Mono"
  |
  +-- 成功 → 代替フォントを使用
  |
  +-- 失敗 → Err("system font not found")
```

### フォント解決失敗時の挙動

> `font.rs`

`parse_page_fonts` はページ内の各フォントを個別に解決する。特定のフォントが解決できなくても、そのフォントをスキップして残りの解析を続ける:

```text
ページ内のフォント: [F1, F2, F3]
  |
  F1: 解決成功 → fonts マップに追加
  F2: "system font not found" → スキップ (fonts マップに含まれない)
  F3: 解決成功 → fonts マップに追加
  |
  → fonts = {F1: ..., F3: ...}   ← F2 が欠けている
```

この状態で Phase A2 に進むと、コンテンツストリーム中に F2 を使うテキストが出現した時点で
`convert_text_to_outlines` が `Err` を返す。そのページ全体が Phase B にフォールバックする。

スキップされるエラーの種類:

| エラーメッセージに含まれる文字列 | 原因 |
| --- | --- |
| `"FontFile2"` | 埋め込みフォントデータがない |
| `"FontDescriptor"` | FontDescriptor 辞書がない |
| `"system font not found"` | システムフォントでも見つからなかった |
| `"unsupported font subtype"` | TrueType/Type0 以外 (Type1, CFF等) |

---

## フォールバックの全体まとめ

```text
ページの処理開始
  |
  |  [フォントなし]
  |  → Phase A2 をスキップ
  |
  v
Phase A2 を試行 ---- 失敗 ---+
  |                           |
  成功                        v
  |                      Phase B: レンダリング
  |                           |
  |                           v
  |                      Phase C: MRC合成
  |                           |
  |                           +-- Bw → compose_bw() JBIG2のみ
  |                           |
  |                           +-- Rgb/Grayscale:
  |                           |     |
  |                           |     compose_text_masked() -- 失敗 --+
  |                           |       |                               |
  |                           |       成功                            v
  |                           |       |                    compose()
  |                           |       |                    全面MRC 3層
  |                           |       |                               |
  v                           v       v                               v
Phase D: PDF 組み立て
```

### フォールバックは2段階

| 段階 | 主処理 | フォールバック先 | 発生箇所 |
| --- | --- | --- | --- |
| 第1段階 | Phase A2 (ベクターパス変換) | Phase B+C (ラスタライズ) | `job_runner.rs` |
| 第2段階 | compose_text_masked | compose (全面MRC) | `page_processor.rs` |

第1段階と第2段階は異なるアプローチ。Phase A2 はレンダリング前にベクターパス変換を試行し、第2段階はレンダリング後にテキスト部分だけ差し替える。

### フォールバックが起きないケース

| 条件 | 理由 | 通る経路 |
| --- | --- | --- |
| `ColorMode::Bw` + 埋め込みフォントあり | Bw でもパス変換可能 | Phase A2 成功 |
| `ColorMode::Skip` | 無加工コピー | Phase D 直行 |
| 全フォント解決成功 + 単純なコンテンツ | パス変換が成功 | Phase A2 成功 |
