use std::collections::HashMap;
use std::path::Path;

use lopdf::Document;

pub struct PdfReader {
    doc: Document,
}

impl PdfReader {
    /// PDFファイルを開いてPdfReaderを作成する。
    pub fn open(path: impl AsRef<Path>) -> crate::error::Result<Self> {
        let doc = Document::load(path)?;
        Ok(Self { doc })
    }

    /// 内部のlopdf Documentへの参照を返す。
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// ページ数を返す。
    pub fn page_count(&self) -> u32 {
        self.doc.get_pages().len() as u32
    }

    /// 指定ページ(1-indexed)のコンテンツストリームをバイト列として返す。
    /// 複数のContentストリームがある場合は結合して返す。
    pub fn page_content_stream(&self, page_num: u32) -> crate::error::Result<Vec<u8>> {
        let page_id = self.get_page_id(page_num)?;
        Ok(self.doc.get_page_content(page_id)?)
    }

    /// 指定ページ(1-indexed)のXObjectリソースのうち、Subtype=ImageのXObject名一覧を返す。
    pub fn page_xobject_names(&self, page_num: u32) -> crate::error::Result<Vec<String>> {
        let page_id = self.get_page_id(page_num)?;
        let (resource_dict, resource_ids) = self.doc.get_page_resources(page_id)?;

        let mut names = Vec::new();

        // ページ辞書に直接埋め込まれたResources
        if let Some(dict) = resource_dict {
            names.extend(self.collect_image_names_from_dict(dict)?);
        }

        // 参照されているResources（親ページツリーから継承されたものも含む）
        for res_id in resource_ids {
            let dict = self.doc.get_dictionary(res_id)?;
            names.extend(self.collect_image_names_from_dict(dict)?);
        }

        names.sort();
        names.dedup();
        Ok(names)
    }

    /// リソース辞書からXObject/Image名を収集する。
    fn collect_image_names_from_dict(
        &self,
        dict: &lopdf::Dictionary,
    ) -> crate::error::Result<Vec<String>> {
        let mut names = Vec::new();

        let xobject_entry = match dict.get(b"XObject") {
            Ok(entry) => entry,
            Err(_) => return Ok(names), // XObjectエントリがない場合は空を返す
        };

        let xobject_dict = match xobject_entry {
            lopdf::Object::Dictionary(d) => d,
            lopdf::Object::Reference(id) => {
                self.doc.get_object(*id).and_then(lopdf::Object::as_dict)?
            }
            _ => return Ok(names),
        };

        for (name_bytes, value) in xobject_dict.iter() {
            // XObjectの実体を取得してSubtype=Imageかチェック
            let obj_dict = match value {
                lopdf::Object::Reference(id) => {
                    &self
                        .doc
                        .get_object(*id)
                        .and_then(lopdf::Object::as_stream)?
                        .dict
                }
                lopdf::Object::Stream(s) => &s.dict,
                _ => continue,
            };
            #[allow(clippy::collapsible_if)]
            if let Ok(subtype) = obj_dict.get(b"Subtype").and_then(lopdf::Object::as_name) {
                if subtype == b"Image" {
                    let name = String::from_utf8_lossy(name_bytes);
                    names.push(name.into_owned());
                }
            }
        }

        Ok(names)
    }

    /// 指定ページ(1-indexed)のXObjectリソースから画像Streamオブジェクトを取得する。
    ///
    /// XObject名をキー、lopdf::Streamを値とするHashMapを返す。
    /// compose_text_maskedに渡してリダクション処理に使用する。
    pub fn page_image_streams(
        &self,
        _page_num: u32,
    ) -> crate::error::Result<HashMap<String, lopdf::Stream>> {
        // Stub: return empty HashMap (PR 7 GREEN will implement full logic)
        Ok(HashMap::new())
    }

    /// ページ番号(1-indexed)からObjectIdを取得する。
    fn get_page_id(&self, page_num: u32) -> crate::error::Result<lopdf::ObjectId> {
        let pages = self.doc.get_pages();
        pages.get(&page_num).copied().ok_or_else(|| {
            crate::error::PdfMaskError::pdf_read(format!("page {} not found", page_num))
        })
    }
}
