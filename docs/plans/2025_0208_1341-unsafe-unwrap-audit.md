# 不要な `unsafe` / `unwrap` の調査結果

## 調査背景

コードベース内に不要な `unsafe` ブロックや `unwrap()` / `expect()` が残っていないか確認する。

---

## `unsafe` の調査結果: 問題なし

全13箇所、すべて FFI（C ライブラリ連携）に必要なもの。不要な
`unsafe` は存在しない。

- `src/ffi/leptonica.rs` (10箇所): leptonica C ライブラリの呼び出し・
  メモリ管理
- `src/ffi/jbig2enc_sys.rs` (1箇所): jbig2enc C 関数の FFI 宣言
- `src/ffi/jbig2enc.rs` (2箇所): jbig2enc 呼び出しとバッファ変換・解放

すべての `unsafe` ブロックで以下が守られている:

- 入力バリデーション（寸法チェック、ビット深度チェック）が `unsafe` の前に実施
- null ポインタチェック済み
- エラー時のメモリ解放が適切（Fix #4, Fix #12 のコメントあり）
- RAII パターン（`Drop` trait）で自動クリーンアップ

## `unwrap()` / `expect()` の調査結果: 問題なし

### 本番コード: 2箇所のみ（いずれも妥当）

1. **`src/cache/hash.rs:34`** - `expect()`

   ```rust
   serde_json::to_string(&map).expect(
       "serializing primitive cache settings to JSON must not fail"
   )
   ```

   → BTreeMap にプリミティブ型（u32, u8, bool）のみ格納。JSON
   シリアライズが失敗することは論理的にありえない。**妥当**。

2. **`src/cache/hash.rs:87`** - `unwrap()`

   ```rust
   let key = pair.split(':').next().unwrap();
   ```

   → `#[cfg(test)]` ブロック内のテストコード。**妥当**。

### テストコード: 約157箇所（すべて妥当）

テストの setup/verification で `.unwrap()` / `.expect()` を使用。
既知の有効なデータに対する操作で、失敗時はテストインフラの問題
を示す。Rust のテスト慣習として適切。

---

## 結論

**不要な `unsafe` や `unwrap` は存在しない。** コードベースは適切なエラーハンドリングが実装されている:

- 本番コードは `Result<T, PdfMaskError>` を一貫して返却
- `thiserror` による 11 種類のドメイン別エラーバリアント
- 外部ライブラリエラーからの `From` trait 変換が整備済み
- `unwrap()` は論理的に失敗しえない箇所またはテストコードに限定
