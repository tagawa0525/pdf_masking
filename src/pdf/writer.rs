// Phase 7: MRC XObject構築、SMask参照、コンテンツストリーム組立

use std::collections::HashMap;

use lopdf::{Document, Object, Stream, dictionary};
use tracing::debug;

use crate::config::job::ColorMode;
use crate::error::PdfMaskError;
#[cfg(feature = "mrc")]
use crate::mrc::{BwLayers, MrcLayers};
use crate::mrc::{ImageModification, TextMaskedData, TextRegionCrop};

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
    /// ソースPDFオブジェクトIDから出力PDFオブジェクトIDへのマッピング。
    /// ページコピー間で共有し、同一オブジェクト（フォント、画像等）の重複を防ぐ。
    copy_id_map: HashMap<lopdf::ObjectId, lopdf::ObjectId>,
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
            copy_id_map: HashMap::new(),
        }
    }

    /// 内部のlopdf::Documentへの可変参照を返す。
    /// PDF最適化などの後処理に使用する。
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// 画像XObjectを追加する共通ヘルパー。
    #[allow(clippy::too_many_arguments)]
    fn add_image_xobject(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        color_space: &str,
        bits_per_component: i64,
        filter: &str,
        smask_id: Option<lopdf::ObjectId>,
    ) -> lopdf::ObjectId {
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => Object::Name(color_space.as_bytes().to_vec()),
            "BitsPerComponent" => bits_per_component,
            "Filter" => Object::Name(filter.as_bytes().to_vec()),
        };
        if let Some(mask_id) = smask_id {
            dict.set("SMask", Object::Reference(mask_id));
        }
        let stream = Stream::new(dict, data.to_vec());
        self.doc.add_object(Object::Stream(stream))
    }

    /// 背景JPEG XObjectを追加する。
    pub(crate) fn add_background_xobject(
        &mut self,
        jpeg_data: &[u8],
        width: u32,
        height: u32,
        color_space: &str,
    ) -> lopdf::ObjectId {
        self.add_image_xobject(jpeg_data, width, height, color_space, 8, "DCTDecode", None)
    }

    /// マスクJBIG2 XObjectを追加する。
    pub(crate) fn add_mask_xobject(
        &mut self,
        jbig2_data: &[u8],
        width: u32,
        height: u32,
    ) -> lopdf::ObjectId {
        self.add_image_xobject(
            jbig2_data,
            width,
            height,
            "DeviceGray",
            1,
            "JBIG2Decode",
            None,
        )
    }

    /// テキスト領域用のImageMask JBIG2 XObjectを追加する。
    ///
    /// ImageMaskはステンシルとして動作し、1のピクセルのみを描画色で塗り、
    /// 0のピクセルは透過される。これにより、背景を上書きせずにテキストだけを描画できる。
    pub(crate) fn add_text_mask_xobject(
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
            "ImageMask" => true,
            "Decode" => vec![Object::Integer(1), Object::Integer(0)],
            "BitsPerComponent" => 1,
            "Filter" => Object::Name(b"JBIG2Decode".to_vec()),
        };
        let stream = Stream::new(dict, jbig2_data.to_vec());
        self.doc.add_object(Object::Stream(stream))
    }

    /// 前景JPEG XObjectを追加する（SMaskとしてmask_idを参照）。
    pub(crate) fn add_foreground_xobject(
        &mut self,
        jpeg_data: &[u8],
        width: u32,
        height: u32,
        mask_id: lopdf::ObjectId,
        color_space: &str,
    ) -> lopdf::ObjectId {
        self.add_image_xobject(
            jpeg_data,
            width,
            height,
            color_space,
            8,
            "DCTDecode",
            Some(mask_id),
        )
    }

    /// MRC用のコンテンツストリームバイト列を生成する。
    pub fn build_mrc_content_stream(
        bg_name: &str,
        fg_name: &str,
        width: f64,
        height: f64,
    ) -> Vec<u8> {
        let bg = escape_pdf_name(bg_name);
        let fg = escape_pdf_name(fg_name);
        format!("q {width} 0 0 {height} 0 0 cm /{bg} Do Q q {width} 0 0 {height} 0 0 cm /{fg} Do Q")
            .into_bytes()
    }

    /// BW用のコンテンツストリームバイト列を生成する。
    fn build_bw_content_stream(img_name: &str, width: f64, height: f64) -> Vec<u8> {
        let name = escape_pdf_name(img_name);
        format!("q {width} 0 0 {height} 0 0 cm /{name} Do Q").into_bytes()
    }

    /// PagesノードのIDを取得（初回時は新規作成）。
    fn ensure_pages_id(&mut self) -> lopdf::ObjectId {
        match self.pages_id {
            Some(id) => id,
            None => {
                let id = self.doc.new_object_id();
                self.pages_id = Some(id);
                id
            }
        }
    }

    /// ページをPages Kidsに追加する。初回呼び出しでPages+Catalogを作成。
    fn append_page_to_kids(&mut self, pages_id: lopdf::ObjectId, page_id: lopdf::ObjectId) {
        if let Some(Object::Dictionary(pages_dict)) = self.doc.objects.get_mut(&pages_id) {
            if let Ok(kids) = pages_dict.get_mut(b"Kids")
                && let Ok(kids_array) = kids.as_array_mut()
            {
                kids_array.push(page_id.into());
            }
            if let Ok(count_obj) = pages_dict.get_mut(b"Count")
                && let Ok(count) = count_obj.as_i64()
            {
                *count_obj = Object::Integer(count + 1);
            }
        } else {
            let pages = dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            };
            self.doc.objects.insert(pages_id, Object::Dictionary(pages));

            let catalog_id = self.doc.add_object(dictionary! {
                "Type" => "Catalog",
                "Pages" => pages_id,
            });
            self.doc.trailer.set("Root", catalog_id);
        }
    }

    /// MrcLayersからPDFページを構築する。
    #[cfg(feature = "mrc")]
    pub fn write_mrc_page(&mut self, layers: &MrcLayers) -> crate::error::Result<lopdf::ObjectId> {
        let width = layers.width;
        let height = layers.height;
        let page_width_pts = layers.page_width_pts;
        let page_height_pts = layers.page_height_pts;
        let color_space = match layers.color_mode {
            ColorMode::Grayscale => "DeviceGray",
            _ => "DeviceRGB",
        };

        let bg_id =
            self.add_background_xobject(&layers.background_jpeg, width, height, color_space);
        let mask_id = self.add_mask_xobject(&layers.mask_jbig2, width, height);
        let fg_id = self.add_foreground_xobject(
            &layers.foreground_jpeg,
            width,
            height,
            mask_id,
            color_space,
        );

        let pages_id = self.ensure_pages_id();

        let mut xobject_dict = lopdf::Dictionary::new();
        xobject_dict.set("BgImg", Object::Reference(bg_id));
        xobject_dict.set("FgImg", Object::Reference(fg_id));

        let resources_id = self.doc.add_object(dictionary! {
            "XObject" => Object::Dictionary(xobject_dict),
        });

        let content_bytes =
            Self::build_mrc_content_stream("BgImg", "FgImg", page_width_pts, page_height_pts);
        let content_stream = Stream::new(dictionary! {}, content_bytes);
        let content_id = self.doc.add_object(Object::Stream(content_stream));

        let page_id = self.doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(page_width_pts as f32),
                Object::Real(page_height_pts as f32),
            ],
            "Resources" => resources_id,
            "Contents" => content_id,
        });

        self.append_page_to_kids(pages_id, page_id);

        debug!("write_mrc_page complete");
        Ok(page_id)
    }

    /// BwLayersからPDFページを構築する（JBIG2マスクのみ）。
    #[cfg(feature = "mrc")]
    pub fn write_bw_page(&mut self, layers: &BwLayers) -> crate::error::Result<lopdf::ObjectId> {
        let width = layers.width;
        let height = layers.height;
        let page_width_pts = layers.page_width_pts;
        let page_height_pts = layers.page_height_pts;

        let mask_id = self.add_mask_xobject(&layers.mask_jbig2, width, height);

        // BWページとしてマスクをそのまま画像として描画する場合、
        // 現在のビット定義は text=1, non-text=0 であり、
        // DeviceGray + BitsPerComponent=1 の既定デコード (0=黒, 1=白) のままだと
        // テキストが白・背景が黒に反転してしまう。
        // そのため、このBW用XObjectに対してのみ Decode 配列で極性を反転させる。
        if let Some(Object::Stream(stream)) = self.doc.objects.get_mut(&mask_id) {
            stream.dict.set(
                "Decode",
                Object::Array(vec![Object::Integer(1), Object::Integer(0)]),
            );
        }

        let pages_id = self.ensure_pages_id();

        let mut xobject_dict = lopdf::Dictionary::new();
        xobject_dict.set("BwImg", Object::Reference(mask_id));

        let resources_id = self.doc.add_object(dictionary! {
            "XObject" => Object::Dictionary(xobject_dict),
        });

        let content_bytes = Self::build_bw_content_stream("BwImg", page_width_pts, page_height_pts);
        let content_stream = Stream::new(dictionary! {}, content_bytes);
        let content_id = self.doc.add_object(Object::Stream(content_stream));

        let page_id = self.doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(page_width_pts as f32),
                Object::Real(page_height_pts as f32),
            ],
            "Resources" => resources_id,
            "Contents" => content_id,
        });

        self.append_page_to_kids(pages_id, page_id);

        debug!("write_bw_page complete");
        Ok(page_id)
    }

    /// TextMaskedDataからPDFページを構築する。
    ///
    /// ソースPDFからページをdeep copyし、以下を変更する:
    /// 1. コンテンツストリーム → テキスト除去済み + テキスト画像Doオペレータ
    /// 2. Resources/XObject → テキスト領域XObject追加 + リダクション済み画像差替え
    pub fn write_text_masked_page(
        &mut self,
        source: &Document,
        page_num: u32,
        data: &TextMaskedData,
    ) -> crate::error::Result<lopdf::ObjectId> {
        let pages = source.get_pages();
        let source_page_id = pages.get(&page_num).ok_or_else(|| {
            PdfMaskError::pdf_read(format!("page {} not found in source document", page_num))
        })?;

        let pages_id = self.ensure_pages_id();
        let new_page_id = self.deep_copy_object(source, *source_page_id)?;

        // Parentを出力PDFのPagesに差し替え
        if let Some(Object::Dictionary(dict)) = self.doc.objects.get_mut(&new_page_id) {
            dict.set("Parent", Object::Reference(pages_id));
        }

        // テキスト領域XObjectを作成（ImageMaskとして）
        let text_xobjects = self.create_text_region_xobjects(&data.text_regions);

        // 新しいコンテンツストリームを構築して差し替え
        let content = Self::build_text_masked_content(
            &data.stripped_content_stream,
            &data.text_regions,
            &text_xobjects,
        );
        let content_stream = Stream::new(dictionary! {}, content);
        let content_id = self.doc.add_object(Object::Stream(content_stream));
        if let Some(Object::Dictionary(page_dict)) = self.doc.objects.get_mut(&new_page_id) {
            page_dict.set("Contents", Object::Reference(content_id));
        }

        // Resources/XObjectを更新
        let resources_obj_id = self.ensure_resources_as_object(new_page_id)?;
        let xobj_dict_id = self.ensure_xobject_dict_as_object(resources_obj_id)?;

        // テキスト領域XObjectをResources/XObjectに追加
        for (name, xobj_id) in &text_xobjects {
            if let Some(Object::Dictionary(dict)) = self.doc.objects.get_mut(&xobj_dict_id) {
                dict.set(name.as_bytes(), Object::Reference(*xobj_id));
            }
        }

        // リダクション済み画像のストリームデータを差し替え
        self.replace_modified_images(xobj_dict_id, &data.modified_images);

        self.append_page_to_kids(pages_id, new_page_id);
        Ok(new_page_id)
    }

    /// テキスト領域ごとにImageMask XObjectを作成し、名前とIDのペアを返す。
    fn create_text_region_xobjects(
        &mut self,
        text_regions: &[TextRegionCrop],
    ) -> Vec<(String, lopdf::ObjectId)> {
        text_regions
            .iter()
            .enumerate()
            .map(|(i, region)| {
                let name = format!("TxtRgn{}", i);
                let xobj_id = self.add_text_mask_xobject(
                    &region.jbig2_data,
                    region.pixel_width,
                    region.pixel_height,
                );
                (name, xobj_id)
            })
            .collect()
    }

    /// テキスト除去済みコンテンツにImageMask描画オペレータを追加して最終コンテンツを構築する。
    fn build_text_masked_content(
        stripped: &[u8],
        text_regions: &[TextRegionCrop],
        text_xobjects: &[(String, lopdf::ObjectId)],
    ) -> Vec<u8> {
        let mut content = stripped.to_vec();
        for (i, (name, _)) in text_xobjects.iter().enumerate() {
            let region = &text_regions[i];
            let w = region.bbox_points.x_max - region.bbox_points.x_min;
            let h = region.bbox_points.y_max - region.bbox_points.y_min;
            let x = region.bbox_points.x_min;
            let y = region.bbox_points.y_min;
            let escaped = escape_pdf_name(name);
            content.extend_from_slice(
                format!("\nq 0 g {w} 0 0 {h} {x} {y} cm /{escaped} Do Q").as_bytes(),
            );
        }
        content
    }

    /// XObject辞書内の画像ストリームデータをリダクション済みデータに差し替える。
    fn replace_modified_images(
        &mut self,
        xobj_dict_id: lopdf::ObjectId,
        modified_images: &HashMap<String, ImageModification>,
    ) {
        for (name, modification) in modified_images {
            let img_obj_id = {
                if let Some(Object::Dictionary(dict)) = self.doc.objects.get(&xobj_dict_id) {
                    dict.get(name.as_bytes())
                        .ok()
                        .and_then(|obj| obj.as_reference().ok())
                } else {
                    None
                }
            };

            if let Some(img_id) = img_obj_id
                && let Some(Object::Stream(stream)) = self.doc.objects.get_mut(&img_id)
            {
                stream.content = modification.data.clone();
                if modification.filter.is_empty() {
                    stream.dict.remove(b"Filter");
                    stream.dict.remove(b"DecodeParms");
                } else {
                    stream.dict.set(
                        "Filter",
                        Object::Name(modification.filter.as_bytes().to_vec()),
                    );
                }
                stream.dict.set(
                    "ColorSpace",
                    Object::Name(modification.color_space.as_bytes().to_vec()),
                );
                stream.dict.set(
                    "BitsPerComponent",
                    Object::Integer(modification.bits_per_component as i64),
                );
                stream.dict.remove(b"Length");
            }
        }
    }

    /// 親オブジェクト内の辞書エントリをインラインから独立オブジェクトに昇格させる。
    /// 既に参照の場合はそのIDを返す。
    fn ensure_dict_entry_as_object(
        &mut self,
        parent_id: lopdf::ObjectId,
        key: &[u8],
    ) -> crate::error::Result<lopdf::ObjectId> {
        let is_reference = {
            let parent_dict = self
                .doc
                .get_dictionary(parent_id)
                .map_err(|e| PdfMaskError::pdf_write(e.to_string()))?;
            matches!(parent_dict.get(key), Ok(Object::Reference(_)))
        };

        if is_reference {
            let parent_dict = self
                .doc
                .get_dictionary(parent_id)
                .map_err(|e| PdfMaskError::pdf_write(e.to_string()))?;
            Ok(parent_dict.get(key).unwrap().as_reference().unwrap())
        } else {
            let dict = {
                let parent_dict = self
                    .doc
                    .get_dictionary(parent_id)
                    .map_err(|e| PdfMaskError::pdf_write(e.to_string()))?;
                match parent_dict.get(key) {
                    Ok(Object::Dictionary(d)) => d.clone(),
                    _ => lopdf::Dictionary::new(),
                }
            };
            let id = self.doc.add_object(Object::Dictionary(dict));
            if let Some(Object::Dictionary(parent_dict)) = self.doc.objects.get_mut(&parent_id) {
                parent_dict.set(key.to_vec(), Object::Reference(id));
            }
            Ok(id)
        }
    }

    /// ページのResourcesをインライン辞書から独立オブジェクトに昇格させる。
    fn ensure_resources_as_object(
        &mut self,
        page_id: lopdf::ObjectId,
    ) -> crate::error::Result<lopdf::ObjectId> {
        self.ensure_dict_entry_as_object(page_id, b"Resources")
    }

    /// Resources内のXObject辞書をインラインから独立オブジェクトに昇格させる。
    fn ensure_xobject_dict_as_object(
        &mut self,
        resources_id: lopdf::ObjectId,
    ) -> crate::error::Result<lopdf::ObjectId> {
        self.ensure_dict_entry_as_object(resources_id, b"XObject")
    }

    /// ソースPDFからページをコピーする（Skipモード用）。
    ///
    /// lopdfオブジェクトの深コピーを行い、Parent参照を出力PDFのPagesノードに差し替える。
    pub fn copy_page_from(
        &mut self,
        source: &Document,
        page_num: u32,
    ) -> crate::error::Result<lopdf::ObjectId> {
        let pages = source.get_pages();
        let source_page_id = pages.get(&page_num).ok_or_else(|| {
            crate::error::PdfMaskError::pdf_read(format!(
                "page {} not found in source document",
                page_num
            ))
        })?;

        let pages_id = self.ensure_pages_id();

        let new_page_id = self.deep_copy_object(source, *source_page_id)?;

        // Parentを出力PDFのPagesに差し替え
        if let Some(Object::Dictionary(dict)) = self.doc.objects.get_mut(&new_page_id) {
            dict.set("Parent", Object::Reference(pages_id));
        }

        self.append_page_to_kids(pages_id, new_page_id);

        debug!(page = page_num, "copy_page_from complete");
        Ok(new_page_id)
    }

    /// ソースPDFのオブジェクトを再帰的に深コピーする。
    ///
    /// `self.copy_id_map` を使い、ページ間で共有されるオブジェクトの重複コピーを防ぐ。
    fn deep_copy_object(
        &mut self,
        source: &Document,
        source_id: lopdf::ObjectId,
    ) -> crate::error::Result<lopdf::ObjectId> {
        // 既にコピー済みならマッピングを返す
        if let Some(&mapped_id) = self.copy_id_map.get(&source_id) {
            return Ok(mapped_id);
        }

        // 先にIDを予約して循環参照を防ぐ
        let new_id = self.doc.new_object_id();
        self.copy_id_map.insert(source_id, new_id);

        // 処理を試行し、エラー時はマップをクリーンアップ
        let result = (|| -> crate::error::Result<lopdf::ObjectId> {
            let source_obj = source
                .get_object(source_id)
                .map_err(|e| crate::error::PdfMaskError::pdf_read(e.to_string()))?;

            let new_obj = self.deep_copy_value(source, source_obj)?;
            self.doc.objects.insert(new_id, new_obj);

            Ok(new_id)
        })();

        match result {
            Ok(id) => Ok(id),
            Err(e) => {
                self.copy_id_map.remove(&source_id);
                Err(e)
            }
        }
    }

    /// オブジェクト値を再帰的にコピーし、Reference先もコピーする。
    fn deep_copy_value(&mut self, source: &Document, obj: &Object) -> crate::error::Result<Object> {
        match obj {
            Object::Reference(ref_id) => {
                let new_id = self.deep_copy_object(source, *ref_id)?;
                Ok(Object::Reference(new_id))
            }
            Object::Dictionary(dict) => {
                let mut new_dict = lopdf::Dictionary::new();
                for (key, value) in dict.iter() {
                    // Parentはコピーしない（呼び出し側で差し替え）
                    if key == b"Parent" {
                        continue;
                    }
                    let new_value = self.deep_copy_value(source, value)?;
                    new_dict.set(key.clone(), new_value);
                }
                Ok(Object::Dictionary(new_dict))
            }
            Object::Array(arr) => {
                let mut new_arr = Vec::with_capacity(arr.len());
                for item in arr {
                    new_arr.push(self.deep_copy_value(source, item)?);
                }
                Ok(Object::Array(new_arr))
            }
            Object::Stream(stream) => {
                let mut new_dict = lopdf::Dictionary::new();
                for (key, value) in stream.dict.iter() {
                    if key == b"Parent" {
                        continue;
                    }
                    let new_value = self.deep_copy_value(source, value)?;
                    new_dict.set(key.clone(), new_value);
                }
                let new_stream = Stream::new(new_dict, stream.content.clone());
                Ok(Object::Stream(new_stream))
            }
            // プリミティブ型はそのままクローン
            other => Ok(other.clone()),
        }
    }

    /// PDFドキュメントをバイト列として出力する。
    pub fn save_to_bytes(&mut self) -> crate::error::Result<Vec<u8>> {
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
        debug!(bytes = buf.len(), "save_to_bytes complete");
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

    #[cfg(feature = "mrc")]
    #[test]
    fn test_create_background_xobject() {
        let jpeg_data: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0];
        let mut writer = MrcPageWriter::new();
        let obj_id = writer.add_background_xobject(&jpeg_data, 640, 480, "DeviceRGB");
        assert!(obj_id.0 > 0, "object id should be positive");
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_create_mask_xobject() {
        let jbig2_data: Vec<u8> = vec![0x97, 0x4A, 0x42, 0x32];
        let mut writer = MrcPageWriter::new();
        let obj_id = writer.add_mask_xobject(&jbig2_data, 800, 600);
        assert!(obj_id.0 > 0, "object id should be positive");
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_create_foreground_xobject_with_smask() {
        let fg_jpeg: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let mask_jbig2: Vec<u8> = vec![0x97, 0x4A, 0x42, 0x32];
        let mut writer = MrcPageWriter::new();
        let mask_id = writer.add_mask_xobject(&mask_jbig2, 640, 480);
        let fg_id = writer.add_foreground_xobject(&fg_jpeg, 640, 480, mask_id, "DeviceRGB");

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

    #[cfg(feature = "mrc")]
    #[test]
    fn test_save_to_bytes_without_catalog_fails() {
        let mut writer = MrcPageWriter::new();
        let result = writer.save_to_bytes();
        assert!(result.is_err(), "save without Catalog should fail");
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_save_to_bytes_with_valid_document() {
        let layers = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
            color_mode: ColorMode::Rgb,
        };
        let mut writer = MrcPageWriter::new();
        writer.write_mrc_page(&layers).expect("write MRC page");
        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");
        assert_eq!(doc.get_pages().len(), 1);
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_multi_page_write() {
        let layers1 = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
            color_mode: ColorMode::Rgb,
        };
        let layers2 = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x01],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1, 0x01],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32, 0x01],
            width: 800,
            height: 600,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
            color_mode: ColorMode::Rgb,
        };
        let layers3 = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x02],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1, 0x02],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32, 0x02],
            width: 1024,
            height: 768,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
            color_mode: ColorMode::Rgb,
        };

        let mut writer = MrcPageWriter::new();
        let id1 = writer.write_mrc_page(&layers1).expect("write page 1");
        let id2 = writer.write_mrc_page(&layers2).expect("write page 2");
        let id3 = writer.write_mrc_page(&layers3).expect("write page 3");

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);

        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");
        assert_eq!(doc.get_pages().len(), 3, "should have 3 pages");
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_write_bw_page() {
        let layers = crate::mrc::BwLayers {
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
        };
        let mut writer = MrcPageWriter::new();
        writer.write_bw_page(&layers).expect("write BW page");
        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");
        assert_eq!(doc.get_pages().len(), 1);
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_write_grayscale_mrc_page() {
        let layers = crate::mrc::MrcLayers {
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
            color_mode: ColorMode::Grayscale,
        };
        let mut writer = MrcPageWriter::new();
        let page_id = writer
            .write_mrc_page(&layers)
            .expect("write grayscale MRC page");

        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");
        assert_eq!(doc.get_pages().len(), 1);

        // Verify ColorSpace is DeviceGray
        let page_dict = doc.get_dictionary(page_id).expect("page dict");
        let resources_ref = page_dict
            .get(b"Resources")
            .expect("Resources")
            .as_reference()
            .expect("Resources ref");
        let resources = doc.get_dictionary(resources_ref).expect("Resources dict");
        let xobject = resources
            .get(b"XObject")
            .expect("XObject")
            .as_dict()
            .expect("XObject dict");
        let bg_ref = xobject
            .get(b"BgImg")
            .expect("BgImg")
            .as_reference()
            .expect("BgImg ref");
        let bg_stream = doc
            .get_object(bg_ref)
            .expect("bg obj")
            .as_stream()
            .expect("bg stream");
        let cs = bg_stream.dict.get(b"ColorSpace").expect("ColorSpace");
        match cs {
            Object::Name(name) => assert_eq!(name, b"DeviceGray"),
            _ => panic!("ColorSpace should be a Name, got {:?}", cs),
        }
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_copy_page_from() {
        // Create a source document with 2 pages
        let mut source = Document::with_version("1.4");
        let pages_id = source.new_object_id();

        let content1 = Stream::new(dictionary! {}, b"q 612 0 0 792 0 0 cm Q".to_vec());
        let content1_id = source.add_object(content1);
        let page1_id = source.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ],
            "Contents" => content1_id,
            "Resources" => dictionary! {},
        });

        let content2 = Stream::new(dictionary! {}, b"q 100 0 0 100 0 0 cm Q".to_vec());
        let content2_id = source.add_object(content2);
        let page2_id = source.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(100), Object::Integer(100),
            ],
            "Contents" => content2_id,
            "Resources" => dictionary! {},
        });

        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page1_id.into(), page2_id.into()],
            "Count" => 2,
        };
        source.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = source.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        source.trailer.set("Root", catalog_id);

        // Copy page 1 into a new writer
        let mut writer = MrcPageWriter::new();
        writer.copy_page_from(&source, 1).expect("copy page 1");

        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load output PDF");
        assert_eq!(doc.get_pages().len(), 1, "output should have 1 copied page");

        // Verify MediaBox is preserved
        let out_pages = doc.get_pages();
        let out_page_id = out_pages.values().next().expect("should have a page");
        let out_page = doc.get_dictionary(*out_page_id).expect("page dict");
        let media_box = out_page.get(b"MediaBox").expect("MediaBox");
        let arr = media_box.as_array().expect("MediaBox array");
        assert_eq!(arr.len(), 4);
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_copy_shared_resources_deduplication() {
        // 2ページが同一フォントオブジェクトを共有するソースPDFを作成
        let mut source = Document::with_version("1.4");
        let pages_id = source.new_object_id();

        // 共有フォントオブジェクト（両ページのResourcesが参照）
        let shared_font_id = source.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });

        let resources_1 = source.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => Object::Reference(shared_font_id),
            },
        });
        let resources_2 = source.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => Object::Reference(shared_font_id),
            },
        });

        let content1 = Stream::new(dictionary! {}, b"BT /F1 12 Tf (Hello) Tj ET".to_vec());
        let content1_id = source.add_object(content1);
        let page1_id = source.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ],
            "Contents" => content1_id,
            "Resources" => resources_1,
        });

        let content2 = Stream::new(dictionary! {}, b"BT /F1 12 Tf (World) Tj ET".to_vec());
        let content2_id = source.add_object(content2);
        let page2_id = source.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ],
            "Contents" => content2_id,
            "Resources" => resources_2,
        });

        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page1_id.into(), page2_id.into()],
            "Count" => 2,
        };
        source.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = source.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        source.trailer.set("Root", catalog_id);

        // 両ページをコピー
        let mut writer = MrcPageWriter::new();
        writer.copy_page_from(&source, 1).expect("copy page 1");
        writer.copy_page_from(&source, 2).expect("copy page 2");

        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load output PDF");
        assert_eq!(doc.get_pages().len(), 2);

        // 共有フォントオブジェクトが1つだけ存在することを確認
        let font_objects: Vec<_> = doc
            .objects
            .values()
            .filter(|obj| {
                if let Object::Dictionary(dict) = obj {
                    dict.get(b"Type")
                        .ok()
                        .and_then(|t| t.as_name().ok())
                        .is_some_and(|n| n == b"Font")
                } else {
                    false
                }
            })
            .collect();
        assert_eq!(
            font_objects.len(),
            1,
            "shared font should be copied only once, found {}",
            font_objects.len()
        );
    }

    #[cfg(feature = "mrc")]
    #[test]
    fn test_mixed_mode_pages() {
        let mrc_layers = crate::mrc::MrcLayers {
            #[cfg(feature = "mrc")]
            background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
            foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
            color_mode: ColorMode::Rgb,
        };
        let bw_layers = crate::mrc::BwLayers {
            mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],
            width: 640,
            height: 480,
            page_width_pts: 595.276,
            page_height_pts: 841.89,
        };

        let mut writer = MrcPageWriter::new();
        writer.write_mrc_page(&mrc_layers).expect("write MRC page");
        writer.write_bw_page(&bw_layers).expect("write BW page");

        let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
        let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");
        assert_eq!(
            doc.get_pages().len(),
            2,
            "should have 2 pages (1 MRC + 1 BW)"
        );
    }
}
