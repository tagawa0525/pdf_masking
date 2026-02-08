// Phase 9: FlateDecode圧縮、孤立オブジェクト除去、フォント削除

use std::collections::HashSet;
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use lopdf::{Document, Object, ObjectId};

use crate::error::PdfMaskError;

/// 指定ページの `/Resources` 辞書から `/Font` エントリを除去する。
///
/// MRC変換されたページでは元のフォントは不要になるため、
/// ファイルサイズ削減のために削除する。
/// inline Resources と indirect (参照) Resources の両方に対応する。
///
/// 共有された indirect Resources を安全に扱う:
/// 他のページと共有されている場合はクローンして新しいオブジェクトとして追加し、
/// 対象ページだけが変更後の Resources を参照するようにする。
pub fn remove_fonts_from_pages(doc: &mut Document, page_ids: &[ObjectId]) {
    let masked_set: HashSet<ObjectId> = page_ids.iter().copied().collect();

    // Collect all page IDs and their indirect Resources references
    let all_page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();

    let mut all_res_refs: Vec<(ObjectId, Option<ObjectId>)> = Vec::new();
    for &pid in &all_page_ids {
        let res_ref = doc
            .get_dictionary(pid)
            .ok()
            .and_then(|d| d.get(b"Resources").ok())
            .and_then(|obj| obj.as_reference().ok());
        all_res_refs.push((pid, res_ref));
    }

    for &page_id in page_ids {
        let resources_ref = {
            let Ok(page_dict) = doc.get_dictionary(page_id) else {
                continue;
            };
            let Ok(resources_obj) = page_dict.get(b"Resources") else {
                continue;
            };
            resources_obj.as_reference().ok()
        };

        if let Some(res_id) = resources_ref {
            // Check if any non-masked page shares this same Resources reference
            let is_shared = all_res_refs
                .iter()
                .any(|&(pid, ref_opt)| !masked_set.contains(&pid) && ref_opt == Some(res_id));

            if is_shared {
                // Clone the Resources dictionary and add as a new object
                let cloned_dict = doc.get_dictionary(res_id).ok().cloned();
                if let Some(mut new_dict) = cloned_dict {
                    new_dict.remove(b"Font");
                    let new_res_id = doc.add_object(Object::Dictionary(new_dict));
                    // Update the page to point to the new Resources object
                    if let Ok(page_dict) = doc.get_dictionary_mut(page_id) {
                        page_dict.set("Resources", Object::Reference(new_res_id));
                    }
                }
            } else {
                // Not shared: modify in place
                if let Ok(res_dict) = doc.get_dictionary_mut(res_id) {
                    res_dict.remove(b"Font");
                }
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
pub fn compress_streams(doc: &mut Document) -> crate::error::Result<()> {
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
            encoder.write_all(&stream.content).map_err(|e| {
                PdfMaskError::pdf_write(format!("stream compression write failed: {e}"))
            })?;
            let compressed = encoder.finish().map_err(|e| {
                PdfMaskError::pdf_write(format!("stream compression finish failed: {e}"))
            })?;

            stream.dict.set("Filter", "FlateDecode");
            stream.set_content(compressed);
        }
    }

    Ok(())
}

/// 孤立オブジェクト（どこからも参照されていないオブジェクト）を除去する。
pub fn delete_unused_objects(doc: &mut Document) {
    doc.prune_objects();
}

/// PDF最適化の全パスを順序通りに実行する。
///
/// 1. 指定ページからフォントを除去
/// 2. 未圧縮ストリームを圧縮
/// 3. 孤立オブジェクトを除去
pub fn optimize(doc: &mut Document, masked_page_ids: &[ObjectId]) -> crate::error::Result<()> {
    remove_fonts_from_pages(doc, masked_page_ids);
    compress_streams(doc)?;
    delete_unused_objects(doc);
    Ok(())
}
