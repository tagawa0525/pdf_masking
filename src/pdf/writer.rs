// Phase 7: MRC XObject構築、SMask参照、コンテンツストリーム組立

use lopdf::{Document, Object, Stream, dictionary};

use crate::mrc::MrcLayers;

/// PDF Name仕様 (PDF Reference 7.3.5) に従い、名前をエスケープする。
///
/// 空白・デリミタ・非印字文字(ASCII 33〜126の範囲外)を`#XX`形式に変換する。
/// `#`自体もエスケープする。
fn escape_pdf_name(name: &str) -> String {
    let mut escaped = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            // PDF delimiters: ( ) < > [ ] { } / %
            // Plus: space (0x20), #(0x23), and non-printable (outside 0x21..=0x7E)
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#' | b' ' => {
                escaped.push_str(&format!("#{:02X}", b));
            }
            0x21..=0x7E => {
                escaped.push(b as char);
            }
            _ => {
                escaped.push_str(&format!("#{:02X}", b));
            }
        }
    }
    escaped
}

/// MrcLayersからPDF XObjectを作成し、ページに追加する。
///
/// 複数ページをサポートする。最初の`write_mrc_page`呼び出しでPages/Catalog構造を作成し、
/// 以降の呼び出しでは既存のKids配列にページを追加する。
pub struct MrcPageWriter {
    doc: Document,
    /// 共有Pagesノード。最初のwrite_mrc_page呼び出しで作成される。
    pages_id: Option<lopdf::ObjectId>,
}

impl Default for MrcPageWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl MrcPageWriter {
    pub fn new() -> Self {
        Self {
            doc: Document::with_version("1.5"),
            pages_id: None,
        }
    }

    /// 内部のlopdf::Documentへの可変参照を返す。
    /// PDF最適化などの後処理に使用する。
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// 背景JPEG XObjectを追加する。
    ///
    /// 戻り値はXObjectのオブジェクトID。
    pub(crate) fn add_background_xobject(
        &mut self,
        jpeg_data: &[u8],
        width: u32,
        height: u32,
    ) -> lopdf::ObjectId {
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        };
        let stream = Stream::new(dict, jpeg_data.to_vec());
        self.doc.add_object(Object::Stream(stream))
    }

    /// マスクJBIG2 XObjectを追加する。
    ///
    /// 戻り値はXObjectのオブジェクトID。
    pub(crate) fn add_mask_xobject(
        &mut self,
        jbig2_data: &[u8],
        width: u32,
        height: u32,
    ) -> lopdf::ObjectId {
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 1,
            "Filter" => "JBIG2Decode",
        };
        let stream = Stream::new(dict, jbig2_data.to_vec());
        self.doc.add_object(Object::Stream(stream))
    }

    /// 前景JPEG XObjectを追加する（SMaskとしてmask_idを参照）。
    ///
    /// 戻り値はXObjectのオブジェクトID。
    pub(crate) fn add_foreground_xobject(
        &mut self,
        jpeg_data: &[u8],
        width: u32,
        height: u32,
        mask_id: lopdf::ObjectId,
    ) -> lopdf::ObjectId {
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
            "SMask" => Object::Reference(mask_id),
        };
        let stream = Stream::new(dict, jpeg_data.to_vec());
        self.doc.add_object(Object::Stream(stream))
    }

    /// MRC用のコンテンツストリームバイト列を生成する。
    ///
    /// 背景と前景の描画コマンドを生成する:
    /// `q <width> 0 0 <height> 0 0 cm /BgName Do Q q <width> 0 0 <height> 0 0 cm /FgName Do Q`
    pub fn build_mrc_content_stream(
        bg_name: &str,
        fg_name: &str,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let bg = escape_pdf_name(bg_name);
        let fg = escape_pdf_name(fg_name);
        format!("q {width} 0 0 {height} 0 0 cm /{bg} Do Q q {width} 0 0 {height} 0 0 cm /{fg} Do Q")
            .into_bytes()
    }

    /// MrcLayersからPDFページを構築する。
    ///
    /// 最初の呼び出しでPages/Catalog構造を作成し、以降の呼び出しでは
    /// 既存のKids配列にページを追加する。追加されたページのObjectIdを返す。
    pub fn write_mrc_page(&mut self, layers: &MrcLayers) -> crate::error::Result<lopdf::ObjectId> {
        let width = layers.width;
        let height = layers.height;

        // XObjectを追加
        let bg_id = self.add_background_xobject(&layers.background_jpeg, width, height);
        let mask_id = self.add_mask_xobject(&layers.mask_jbig2, width, height);
        let fg_id = self.add_foreground_xobject(&layers.foreground_jpeg, width, height, mask_id);

        // Pages構造を初期化（初回のみ）またはIDを取得
        let pages_id = match self.pages_id {
            Some(id) => id,
            None => {
                let id = self.doc.new_object_id();
                self.pages_id = Some(id);
                id
            }
        };

        // XObjectリソース辞書を構築
        let mut xobject_dict = lopdf::Dictionary::new();
        xobject_dict.set("BgImg", Object::Reference(bg_id));
        xobject_dict.set("FgImg", Object::Reference(fg_id));
        // MaskImgはSMask経由で参照されるため、XObjectリソースには不要

        let resources_id = self.doc.add_object(dictionary! {
            "XObject" => Object::Dictionary(xobject_dict),
        });

        // コンテンツストリームを生成
        let content_bytes = Self::build_mrc_content_stream("BgImg", "FgImg", width, height);
        let content_stream = Stream::new(dictionary! {}, content_bytes);
        let content_id = self.doc.add_object(Object::Stream(content_stream));

        // ページを作成
        let page_id = self.doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(width as i64),
                Object::Integer(height as i64),
            ],
            "Resources" => resources_id,
            "Contents" => content_id,
        });

        // PagesノードにKidsを追加/更新
        if let Some(Object::Dictionary(pages_dict)) = self.doc.objects.get_mut(&pages_id) {
            // 既存のPagesノードにページを追加
            if let Ok(kids) = pages_dict.get_mut(b"Kids") {
                if let Ok(kids_array) = kids.as_array_mut() {
                    kids_array.push(page_id.into());
                }
            }
            if let Ok(count_obj) = pages_dict.get_mut(b"Count") {
                if let Ok(count) = count_obj.as_i64() {
                    *count_obj = Object::Integer(count + 1);
                }
            }
        } else {
            // 初回: Pagesノードを新規作成
            let pages = dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            };
            self.doc.objects.insert(pages_id, Object::Dictionary(pages));

            // Catalogを作成（初回のみ）
            let catalog_id = self.doc.add_object(dictionary! {
                "Type" => "Catalog",
                "Pages" => pages_id,
            });
            self.doc.trailer.set("Root", catalog_id);
        }

        Ok(page_id)
    }

    /// PDFドキュメントをバイト列として出力する。
    ///
    /// Catalog及びPagesが存在しない場合はエラーを返す。
    pub fn save_to_bytes(&mut self) -> crate::error::Result<Vec<u8>> {
        // Catalog/Pagesの存在を検証する
        let root_ref = self.doc.trailer.get(b"Root").map_err(|_| {
            crate::error::PdfMaskError::pdf_write("missing Catalog (Root) in trailer")
        })?;
        let catalog_id = root_ref
            .as_reference()
            .map_err(|_| crate::error::PdfMaskError::pdf_write("Root is not a reference"))?;
        let catalog = self
            .doc
            .get_dictionary(catalog_id)
            .map_err(|_| crate::error::PdfMaskError::pdf_write("Catalog object not found"))?;
        let pages_ref = catalog
            .get(b"Pages")
            .map_err(|_| crate::error::PdfMaskError::pdf_write("missing Pages in Catalog"))?;
        let pages_id = pages_ref
            .as_reference()
            .map_err(|_| crate::error::PdfMaskError::pdf_write("Pages is not a reference"))?;
        self.doc
            .get_dictionary(pages_id)
            .map_err(|_| crate::error::PdfMaskError::pdf_write("Pages object not found"))?;

        let mut buf = Vec::new();
        self.doc
            .save_to(&mut buf)
            .map_err(|e| crate::error::PdfMaskError::pdf_write(e.to_string()))?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Document;

    #[test]
    fn test_escape_pdf_name_simple() {
        assert_eq!(escape_pdf_name("BgImg"), "BgImg");
    }

    #[test]
    fn test_escape_pdf_name_with_space() {
        assert_eq!(escape_pdf_name("Bg Img"), "Bg#20Img");
    }

    #[test]
    fn test_escape_pdf_name_with_delimiters() {
        assert_eq!(escape_pdf_name("A(B)"), "A#28B#29");
        assert_eq!(escape_pdf_name("A/B"), "A#2FB");
        assert_eq!(escape_pdf_name("A#B"), "A#23B");
    }

    #[test]
    fn test_escape_pdf_name_non_printable() {
        assert_eq!(escape_pdf_name("A\x00B"), "A#00B");
        assert_eq!(escape_pdf_name("A\x7FB"), "A#7FB");
    }

    #[test]
    fn test_create_background_xobject() {
        let jpeg_data: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0];
        let mut writer = MrcPageWriter::new();
        let obj_id = writer.add_background_xobject(&jpeg_data, 640, 480);
        assert!(obj_id.0 > 0, "object id should be positive");
    }

    #[test]
    fn test_create_mask_xobject() {
        let jbig2_data: Vec<u8> = vec![0x97, 0x4A, 0x42, 0x32];
        let mut writer = MrcPageWriter::new();
        let obj_id = writer.add_mask_xobject(&jbig2_data, 800, 600);
        assert!(obj_id.0 > 0, "object id should be positive");
    }

    #[test]
    fn test_create_foreground_xobject_with_smask() {
        let fg_jpeg: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let mask_jbig2: Vec<u8> = vec![0x97, 0x4A, 0x42, 0x32];
        let mut writer = MrcPageWriter::new();
        let mask_id = writer.add_mask_xobject(&mask_jbig2, 640, 480);
        let fg_id = writer.add_foreground_xobject(&fg_jpeg, 640, 480, mask_id);

        // Verify SMask reference via direct document access
        let obj = writer.doc.get_object(fg_id).expect("get object");
        let stream = obj.as_stream().expect("as stream");
        let smask = stream.dict.get(b"SMask").expect("get SMask");
        match smask {
            Object::Reference(ref_id) => {
                assert_eq!(*ref_id, mask_id, "SMask should reference the mask object");
            }
            _ => panic!("SMask should be a Reference, got {:?}", smask),
        }
    }

    #[test]
    fn test_save_to_bytes_without_catalog_fails() {
        let mut writer = MrcPageWriter::new();
        let result = writer.save_to_bytes();
        assert!(result.is_err(), "save without Catalog should fail");
    }

    #[test]
    fn test_save_to_bytes_with_valid_document() {
        let layers = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
        };
        let mut writer = MrcPageWriter::new();
        writer.write_mrc_page(&layers).expect("write MRC page");
        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");
        assert_eq!(doc.get_pages().len(), 1);
    }

    #[test]
    fn test_multi_page_write() {
        let layers1 = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
        };
        let layers2 = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x01],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1, 0x01],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32, 0x01],
            width: 800,
            height: 600,
        };
        let layers3 = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x02],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1, 0x02],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32, 0x02],
            width: 1024,
            height: 768,
        };

        let mut writer = MrcPageWriter::new();
        let id1 = writer.write_mrc_page(&layers1).expect("write page 1");
        let id2 = writer.write_mrc_page(&layers2).expect("write page 2");
        let id3 = writer.write_mrc_page(&layers3).expect("write page 3");

        // All page IDs should be unique
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);

        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");
        assert_eq!(doc.get_pages().len(), 3, "should have 3 pages");
    }
}
