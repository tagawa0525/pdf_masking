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

    /// 指定ページ辞書からMediaBoxを取得する（Parent経由の継承も考慮）。
    fn get_media_box(&self, dict: &lopdf::Dictionary) -> crate::error::Result<lopdf::Object> {
        // まず現在の辞書からMediaBoxを探す
        if let Ok(obj) = dict.get(b"MediaBox") {
            return Ok(obj.clone());
        }

        // 見つからなければParentをたどって継承を確認する
        if let Ok(lopdf::Object::Reference(parent_id)) = dict.get(b"Parent") {
            let parent_dict = self.doc.get_dictionary(*parent_id)?;
            return self.get_media_box(parent_dict);
        }

        Err(crate::error::PdfMaskError::pdf_read("MediaBox not found"))
    }

    /// 指定ページ(1-indexed)のMediaBoxからページ寸法(width_pts, height_pts)を返す。
    pub fn page_dimensions(&self, page_num: u32) -> crate::error::Result<(f64, f64)> {
        let page_id = self.get_page_id(page_num)?;
        let page_dict = self.doc.get_dictionary(page_id)?;

        // MediaBoxを取得（継承も考慮）
        let media_box = self.get_media_box(page_dict)?;

        let media_box_array = media_box.as_array()?;
        if media_box_array.len() < 4 {
            return Err(crate::error::PdfMaskError::pdf_read("Invalid MediaBox"));
        }

        // MediaBoxの値は整数または実数の可能性がある
        let to_f64 = |obj: &lopdf::Object| -> crate::error::Result<f64> {
            match obj {
                lopdf::Object::Integer(i) => Ok(*i as f64),
                lopdf::Object::Real(f) => Ok(*f as f64),
                _ => Err(crate::error::PdfMaskError::pdf_read(
                    "Invalid MediaBox value",
                )),
            }
        };

        let x0 = to_f64(&media_box_array[0])?;
        let y0 = to_f64(&media_box_array[1])?;
        let x1 = to_f64(&media_box_array[2])?;
        let y1 = to_f64(&media_box_array[3])?;

        let width = (x1 - x0).abs();
        let height = (y1 - y0).abs();

        // Validate that the computed page dimensions are positive and reasonable.
        if width <= 0.0 || height <= 0.0 {
            return Err(crate::error::PdfMaskError::pdf_read(
                "Invalid MediaBox: non-positive page dimensions",
            ));
        }

        // Optionally enforce an upper bound based on typical PDF limits (14,400 pt ≈ 200 in).
        const PDF_MAX_DIMENSION_PT: f64 = 14_400.0;
        if width > PDF_MAX_DIMENSION_PT || height > PDF_MAX_DIMENSION_PT {
            return Err(crate::error::PdfMaskError::pdf_read(
                "Invalid MediaBox: page dimensions exceed PDF limits",
            ));
        }

        Ok((width, height))
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

    /// リソース辞書のXObjectエントリからSubtype=Imageのストリームを列挙し、
    /// 各画像に対してコールバックを呼び出す共通ヘルパー。
    fn for_each_image_xobject<'a, F>(
        &'a self,
        dict: &'a lopdf::Dictionary,
        mut f: F,
    ) -> crate::error::Result<()>
    where
        F: FnMut(String, &'a lopdf::Stream),
    {
        let xobject_entry = match dict.get(b"XObject") {
            Ok(entry) => entry,
            Err(_) => return Ok(()), // XObjectエントリがない場合は何もしない
        };

        let xobject_dict = match xobject_entry {
            lopdf::Object::Dictionary(d) => d,
            lopdf::Object::Reference(id) => {
                self.doc.get_object(*id).and_then(lopdf::Object::as_dict)?
            }
            _ => return Ok(()),
        };

        for (name_bytes, value) in xobject_dict.iter() {
            let stream = match value {
                lopdf::Object::Reference(id) => self
                    .doc
                    .get_object(*id)
                    .and_then(lopdf::Object::as_stream)?,
                lopdf::Object::Stream(s) => s,
                _ => continue,
            };

            if let Ok(subtype) = stream.dict.get(b"Subtype").and_then(lopdf::Object::as_name)
                && subtype == b"Image"
            {
                let name = String::from_utf8_lossy(name_bytes).into_owned();
                f(name, stream);
            }
        }

        Ok(())
    }

    /// リソース辞書からXObject/Image名を収集する。
    fn collect_image_names_from_dict(
        &self,
        dict: &lopdf::Dictionary,
    ) -> crate::error::Result<Vec<String>> {
        let mut names = Vec::new();
        self.for_each_image_xobject(dict, |name, _stream| {
            names.push(name);
        })?;
        Ok(names)
    }

    /// 指定ページ(1-indexed)のXObjectリソースから画像Streamオブジェクトを取得する。
    ///
    /// XObject名をキー、lopdf::Streamを値とするHashMapを返す。
    /// compose_text_maskedに渡してリダクション処理に使用する。
    pub fn page_image_streams(
        &self,
        page_num: u32,
    ) -> crate::error::Result<HashMap<String, lopdf::Stream>> {
        let page_id = self.get_page_id(page_num)?;
        let (resource_dict, resource_ids) = self.doc.get_page_resources(page_id)?;

        let mut streams = HashMap::new();

        if let Some(dict) = resource_dict {
            self.collect_image_streams_from_dict(dict, &mut streams)?;
        }
        for res_id in resource_ids {
            let dict = self.doc.get_dictionary(res_id)?;
            self.collect_image_streams_from_dict(dict, &mut streams)?;
        }

        Ok(streams)
    }

    /// リソース辞書からXObject/ImageのStreamオブジェクトを収集する。
    fn collect_image_streams_from_dict(
        &self,
        dict: &lopdf::Dictionary,
        streams: &mut HashMap<String, lopdf::Stream>,
    ) -> crate::error::Result<()> {
        self.for_each_image_xobject(dict, |name, stream| {
            streams.insert(name, stream.clone());
        })?;
        Ok(())
    }

    /// ページ番号(1-indexed)からObjectIdを取得する。
    fn get_page_id(&self, page_num: u32) -> crate::error::Result<lopdf::ObjectId> {
        let pages = self.doc.get_pages();
        pages.get(&page_num).copied().ok_or_else(|| {
            crate::error::PdfMaskError::pdf_read(format!("page {} not found", page_num))
        })
    }
}
