use lopdf::Document;

pub struct PdfReader {
    doc: Document,
}

impl PdfReader {
    /// PDFファイルを開いてPdfReaderを作成する。
    pub fn open(path: &str) -> crate::error::Result<Self> {
        let doc = Document::load(path)
            .map_err(|e| crate::error::PdfMaskError::pdf_read(e.to_string()))?;
        Ok(Self { doc })
    }

    /// ページ数を返す。
    pub fn page_count(&self) -> u32 {
        self.doc.get_pages().len() as u32
    }

    /// 指定ページ(1-indexed)のコンテンツストリームをバイト列として返す。
    /// 複数のContentストリームがある場合は結合して返す。
    pub fn page_content_stream(&self, page_num: u32) -> crate::error::Result<Vec<u8>> {
        let page_id = self.get_page_id(page_num)?;
        self.doc
            .get_page_content(page_id)
            .map_err(|e| crate::error::PdfMaskError::pdf_read(e.to_string()))
    }

    /// 指定ページ(1-indexed)のXObjectリソースのうち、Subtype=ImageのXObject名一覧を返す。
    pub fn page_xobject_names(&self, page_num: u32) -> crate::error::Result<Vec<String>> {
        let page_id = self.get_page_id(page_num)?;
        let (resource_dict, resource_ids) = self
            .doc
            .get_page_resources(page_id)
            .map_err(|e| crate::error::PdfMaskError::pdf_read(e.to_string()))?;

        let mut names = Vec::new();

        // リソース辞書からXObjectを収集するクロージャ
        let mut collect_from_dict = |dict: &lopdf::Dictionary| {
            if let Ok(xobject_entry) = dict.get(b"XObject") {
                let xobject_dict = match xobject_entry {
                    lopdf::Object::Dictionary(d) => Some(d),
                    lopdf::Object::Reference(id) => self
                        .doc
                        .get_object(*id)
                        .and_then(lopdf::Object::as_dict)
                        .ok(),
                    _ => None,
                };
                if let Some(xobject_dict) = xobject_dict {
                    for (name_bytes, value) in xobject_dict.iter() {
                        // XObjectの実体を取得してSubtype=Imageかチェック
                        let obj_dict = match value {
                            lopdf::Object::Reference(id) => self
                                .doc
                                .get_object(*id)
                                .and_then(lopdf::Object::as_stream)
                                .map(|s| &s.dict)
                                .ok(),
                            lopdf::Object::Stream(s) => Some(&s.dict),
                            _ => None,
                        };
                        if let Some(d) = obj_dict {
                            if let Ok(subtype) = d.get(b"Subtype").and_then(lopdf::Object::as_name)
                            {
                                if subtype == b"Image" {
                                    if let Ok(name) = std::str::from_utf8(name_bytes) {
                                        names.push(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };

        // ページ辞書に直接埋め込まれたResources
        if let Some(dict) = resource_dict {
            collect_from_dict(dict);
        }

        // 参照されているResources（親ページツリーから継承されたものも含む）
        for res_id in resource_ids {
            if let Ok(dict) = self.doc.get_dictionary(res_id) {
                collect_from_dict(dict);
            }
        }

        names.sort();
        names.dedup();
        Ok(names)
    }

    /// ページ番号(1-indexed)からObjectIdを取得する。
    fn get_page_id(&self, page_num: u32) -> crate::error::Result<lopdf::ObjectId> {
        let pages = self.doc.get_pages();
        pages.get(&page_num).copied().ok_or_else(|| {
            crate::error::PdfMaskError::pdf_read(format!("page {} not found", page_num))
        })
    }
}
