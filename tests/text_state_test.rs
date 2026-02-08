// テキスト状態パーサのテスト (RED phase)

use pdf_masking::pdf::text_state::{FillColor, TextDrawCommand, parse_content_operations};

// ============================================================
// 1. 基本的なテキストブロック解析
// ============================================================

#[test]
fn test_parse_simple_bt_et_with_tj() {
    // BT /F1 12 Tf (Hello) Tj ET
    let content = b"BT /F1 12 Tf (Hello) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    // BT...ET外のオペレータはそのまま出力に含まれる
    // テキスト描画コマンドが1つ抽出される
    assert_eq!(result.text_commands.len(), 1);
    let cmd = &result.text_commands[0];
    assert_eq!(cmd.font_name, "F1");
    assert!((cmd.font_size - 12.0).abs() < 1e-6);
    assert!(!cmd.char_codes.is_empty());
}

#[test]
fn test_parse_tj_array() {
    // BT /F1 10 Tf [(A) -100 (B)] TJ ET
    let content = b"BT /F1 10 Tf [(A) -100 (B)] TJ ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    let cmd = &result.text_commands[0];
    assert_eq!(cmd.font_name, "F1");
    assert!((cmd.font_size - 10.0).abs() < 1e-6);
}

// ============================================================
// 2. テキストマトリクスの追跡
// ============================================================

#[test]
fn test_td_updates_text_matrix() {
    // BT /F1 12 Tf 100 200 Td (A) Tj ET
    let content = b"BT /F1 12 Tf 100 200 Td (A) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    let cmd = &result.text_commands[0];
    // Td sets text matrix translation
    assert!((cmd.text_matrix.e - 100.0).abs() < 1e-6);
    assert!((cmd.text_matrix.f - 200.0).abs() < 1e-6);
}

#[test]
fn test_tm_sets_text_matrix() {
    // BT /F1 10 Tf 1 0 0 1 50 700 Tm (X) Tj ET
    let content = b"BT /F1 10 Tf 1 0 0 1 50 700 Tm (X) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    let cmd = &result.text_commands[0];
    assert!((cmd.text_matrix.e - 50.0).abs() < 1e-6);
    assert!((cmd.text_matrix.f - 700.0).abs() < 1e-6);
}

// ============================================================
// 3. fill colorの追跡
// ============================================================

#[test]
fn test_fill_color_rgb() {
    // 0.5 0 0 rg で赤っぽい色を設定してからテキスト描画
    let content = b"BT /F1 12 Tf 0.5 0 0 rg (R) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    let cmd = &result.text_commands[0];
    match &cmd.fill_color {
        FillColor::Rgb(r, g, b) => {
            assert!((r - 0.5).abs() < 1e-6);
            assert!(g.abs() < 1e-6);
            assert!(b.abs() < 1e-6);
        }
        _ => panic!("expected RGB fill color"),
    }
}

#[test]
fn test_fill_color_gray() {
    let content = b"BT /F1 12 Tf 0.3 g (G) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    match &result.text_commands[0].fill_color {
        FillColor::Gray(g) => {
            assert!((g - 0.3).abs() < 1e-6);
        }
        _ => panic!("expected Gray fill color"),
    }
}

#[test]
fn test_fill_color_default_black() {
    // fill color未設定の場合、デフォルトは黒 (Gray(0))
    let content = b"BT /F1 12 Tf (B) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    match &result.text_commands[0].fill_color {
        FillColor::Gray(g) => {
            assert!(g.abs() < 1e-6, "default fill color should be black (0)");
        }
        _ => panic!("expected default Gray(0) fill color"),
    }
}

// ============================================================
// 4. CTMの追跡
// ============================================================

#[test]
fn test_ctm_tracks_cm_operator() {
    // cm でCTMを変更してからテキスト描画
    let content = b"2 0 0 2 10 20 cm BT /F1 12 Tf (S) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    let cmd = &result.text_commands[0];
    assert!((cmd.ctm.a - 2.0).abs() < 1e-6);
    assert!((cmd.ctm.d - 2.0).abs() < 1e-6);
    assert!((cmd.ctm.e - 10.0).abs() < 1e-6);
    assert!((cmd.ctm.f - 20.0).abs() < 1e-6);
}

#[test]
fn test_ctm_save_restore() {
    // q/Q でCTMスタックが正しく動作する
    let content = b"q 2 0 0 2 0 0 cm Q BT /F1 12 Tf (A) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    let cmd = &result.text_commands[0];
    // Qで復元されたので、CTMは単位行列に戻る
    assert!((cmd.ctm.a - 1.0).abs() < 1e-6);
    assert!((cmd.ctm.d - 1.0).abs() < 1e-6);
}

// ============================================================
// 5. テキスト状態パラメータ
// ============================================================

#[test]
fn test_char_spacing_tc() {
    let content = b"BT /F1 12 Tf 2.5 Tc (AB) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    assert!((result.text_commands[0].char_spacing - 2.5).abs() < 1e-6);
}

#[test]
fn test_word_spacing_tw() {
    let content = b"BT /F1 12 Tf 5.0 Tw (A B) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    assert!((result.text_commands[0].word_spacing - 5.0).abs() < 1e-6);
}

#[test]
fn test_horizontal_scaling_tz() {
    let content = b"BT /F1 12 Tf 150 Tz (W) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    assert!((result.text_commands[0].horizontal_scaling - 150.0).abs() < 1e-6);
}

#[test]
fn test_text_rise_ts() {
    let content = b"BT /F1 12 Tf 3.0 Ts (R) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    assert!((result.text_commands[0].text_rise - 3.0).abs() < 1e-6);
}

// ============================================================
// 6. 非テキストオペレータの保持
// ============================================================

#[test]
fn test_non_text_operations_preserved() {
    // BT...ET外のオペレータは non_text_operations に保持される
    let content = b"q 1 0 0 1 0 0 cm BT /F1 12 Tf (A) Tj ET Q";
    let result = parse_content_operations(content).expect("should parse");

    // non_text_operations には q, cm, Q が含まれる（BT...ET内のものは除く）
    assert!(!result.non_text_operations.is_empty());
}

// ============================================================
// 7. 複数のテキストブロック
// ============================================================

#[test]
fn test_multiple_text_blocks() {
    let content = b"BT /F1 12 Tf (A) Tj ET BT /F2 10 Tf (B) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 2);
    assert_eq!(result.text_commands[0].font_name, "F1");
    assert_eq!(result.text_commands[1].font_name, "F2");
}

// ============================================================
// 8. エッジケース
// ============================================================

#[test]
fn test_empty_content() {
    let content = b"";
    let result = parse_content_operations(content).expect("should handle empty");
    assert!(result.text_commands.is_empty());
    assert!(result.non_text_operations.is_empty());
}

#[test]
fn test_no_text_blocks() {
    let content = b"q 1 0 0 1 0 0 cm /Im1 Do Q";
    let result = parse_content_operations(content).expect("should handle no text");
    assert!(result.text_commands.is_empty());
    assert!(!result.non_text_operations.is_empty());
}

#[test]
fn test_bt_without_tf_uses_default() {
    // Tf無しのBT...ETブロック（フォント未設定）
    let content = b"BT (X) Tj ET";
    let result = parse_content_operations(content).expect("should parse");

    assert_eq!(result.text_commands.len(), 1);
    // フォント未設定の場合、空文字列
    assert!(result.text_commands[0].font_name.is_empty());
}
