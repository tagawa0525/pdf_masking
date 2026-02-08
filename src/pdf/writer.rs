// Phase 7: MRC XObject構築、SMask参照、コンテンツストリーム組立

use lopdf::{Document, Object, Stream, dictionary};

use crate::mrc::MrcLayers;

/// MrcLayersからPDF XObjectを作成し、ページに追加する。
pub struct MrcPageWriter {
    doc: Document,
}

impl MrcPageWriter {
    pub fn new() -> Self {
        Self {
            doc: Document::with_version("1.5"),
        }
    }

    /// 背景JPEG XObjectを追加する。
    ///
    /// 戻り値はXObjectのオブジェクトID。
    pub fn add_background_xobject(
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
    pub fn add_mask_xobject(
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
    pub fn add_foreground_xobject(
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
        format!(
            "q {width} 0 0 {height} 0 0 cm /{bg_name} Do Q q {width} 0 0 {height} 0 0 cm /{fg_name} Do Q"
        )
        .into_bytes()
    }

    /// MrcLayersからPDFページを構築する。
    pub fn write_mrc_page(&mut self, layers: &MrcLayers) -> crate::error::Result<()> {
        let width = layers.width;
        let height = layers.height;

        // XObjectを追加
        let bg_id = self.add_background_xobject(&layers.background_jpeg, width, height);
        let mask_id = self.add_mask_xobject(&layers.mask_jbig2, width, height);
        let fg_id = self.add_foreground_xobject(&layers.foreground_jpeg, width, height, mask_id);

        // Pagesノードを作成
        let pages_id = self.doc.new_object_id();

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

        // Pagesノードを設定
        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        };
        self.doc.objects.insert(pages_id, Object::Dictionary(pages));

        // Catalogを作成
        let catalog_id = self.doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        self.doc.trailer.set("Root", catalog_id);

        Ok(())
    }

    /// PDFドキュメントをバイト列として出力する。
    pub fn save_to_bytes(&self) -> crate::error::Result<Vec<u8>> {
        let mut buf = Vec::new();
        // clone to avoid borrowing issues with save_to (takes &mut self in lopdf)
        self.doc
            .clone()
            .save_to(&mut buf)
            .map_err(|e| crate::error::PdfMaskError::pdf_write(e.to_string()))?;
        Ok(buf)
    }
}
