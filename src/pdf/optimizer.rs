// Phase 9: FlateDecode圧縮、孤立オブジェクト除去、フォント削除

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use lopdf::{Document, Object, ObjectId};

/// 指定ページの `/Resources` 辞書から `/Font` エントリを除去する。
///
/// MRC変換されたページでは元のフォントは不要になるため、
/// ファイルサイズ削減のために削除する。
/// inline Resources と indirect (参照) Resources の両方に対応する。
#[allow(dead_code)]
pub fn remove_fonts_from_pages(doc: &mut Document, page_ids: &[ObjectId]) {
    for &page_id in page_ids {
        // Get the page dictionary to find the Resources entry
        let resources_ref = {
            let Ok(page_dict) = doc.get_dictionary(page_id) else {
                continue;
            };
            let Ok(resources_obj) = page_dict.get(b"Resources") else {
                continue;
            };
            // Check if Resources is a reference (indirect) or inline
            resources_obj.as_reference().ok()
        };

        if let Some(res_id) = resources_ref {
            // Indirect Resources: modify the referenced dictionary
            if let Ok(res_dict) = doc.get_dictionary_mut(res_id) {
                res_dict.remove(b"Font");
            }
        } else if let Ok(page_dict) = doc.get_dictionary_mut(page_id)
            && let Ok(resources_obj) = page_dict.get_mut(b"Resources")
            && let Ok(res_dict) = resources_obj.as_dict_mut()
        {
            // Inline Resources: modify directly in the page dictionary
            res_dict.remove(b"Font");
        }
    }
}

/// ドキュメント内の未圧縮ストリームにFlateDecode圧縮を適用する。
///
/// 既にフィルターが設定されているストリームはスキップする（二重圧縮防止）。
#[allow(dead_code)]
pub fn compress_streams(doc: &mut Document) {
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();

    for id in ids {
        let needs_compression = {
            let Some(Object::Stream(stream)) = doc.objects.get(&id) else {
                continue;
            };
            // Skip streams that already have a filter
            stream.dict.get(b"Filter").is_err()
        };

        if needs_compression {
            let Some(Object::Stream(stream)) = doc.objects.get_mut(&id) else {
                continue;
            };

            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            if encoder.write_all(&stream.content).is_err() {
                continue;
            }
            let Ok(compressed) = encoder.finish() else {
                continue;
            };

            stream.dict.set("Filter", "FlateDecode");
            stream.set_content(compressed);
        }
    }
}

/// 孤立オブジェクト（どこからも参照されていないオブジェクト）を除去する。
#[allow(dead_code)]
pub fn delete_unused_objects(doc: &mut Document) {
    doc.prune_objects();
}

/// PDF最適化の全パスを順序通りに実行する。
///
/// 1. 指定ページからフォントを除去
/// 2. 未圧縮ストリームを圧縮
/// 3. 孤立オブジェクトを除去
#[allow(dead_code)]
pub fn optimize(doc: &mut Document, masked_page_ids: &[ObjectId]) {
    remove_fonts_from_pages(doc, masked_page_ids);
    compress_streams(doc);
    delete_unused_objects(doc);
}
