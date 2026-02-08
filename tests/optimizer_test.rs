use lopdf::{Document, Object, Stream, dictionary};

/// Helper: create a minimal PDF document with one page that has Font and XObject resources.
fn create_doc_with_font_page() -> (Document, lopdf::ObjectId) {
    let mut doc = Document::with_version("1.5");

    let pages_id = doc.new_object_id();

    // Create a page with inline Resources containing Font and XObject
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F1" => dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                },
            },
            "XObject" => dictionary! {
                "Im0" => Object::Integer(999),
            },
        },
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    (doc, page_id)
}

/// Helper: create a doc with two pages, both having Font resources.
fn create_doc_with_two_font_pages() -> (Document, lopdf::ObjectId, lopdf::ObjectId) {
    let mut doc = Document::with_version("1.5");

    let pages_id = doc.new_object_id();

    let page1_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F1" => dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                },
            },
        },
    });

    let page2_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F2" => dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Courier",
                },
            },
        },
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page1_id.into(), page2_id.into()],
        "Count" => 2,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    (doc, page1_id, page2_id)
}

#[test]
fn test_remove_fonts_from_page() {
    let (mut doc, page_id) = create_doc_with_font_page();

    // Verify Font exists before removal
    let page_dict = doc.get_dictionary(page_id).unwrap();
    let resources = page_dict.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(
        resources.get(b"Font").is_ok(),
        "Font should exist before removal"
    );

    pdf_masking::pdf::optimizer::remove_fonts_from_pages(&mut doc, &[page_id]);

    // Verify Font is removed
    let page_dict = doc.get_dictionary(page_id).unwrap();
    let resources = page_dict.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(
        resources.get(b"Font").is_err(),
        "Font should be removed after optimization"
    );
}

#[test]
fn test_remove_fonts_preserves_xobject() {
    let (mut doc, page_id) = create_doc_with_font_page();

    pdf_masking::pdf::optimizer::remove_fonts_from_pages(&mut doc, &[page_id]);

    // Verify XObject is still present
    let page_dict = doc.get_dictionary(page_id).unwrap();
    let resources = page_dict.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(
        resources.get(b"XObject").is_ok(),
        "XObject should be preserved"
    );
}

#[test]
fn test_remove_fonts_skips_unmasked_pages() {
    let (mut doc, page1_id, page2_id) = create_doc_with_two_font_pages();

    // Only remove fonts from page 1
    pdf_masking::pdf::optimizer::remove_fonts_from_pages(&mut doc, &[page1_id]);

    // Page 1 should have no Font
    let page1_dict = doc.get_dictionary(page1_id).unwrap();
    let resources1 = page1_dict.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(
        resources1.get(b"Font").is_err(),
        "Page 1 Font should be removed"
    );

    // Page 2 should still have Font
    let page2_dict = doc.get_dictionary(page2_id).unwrap();
    let resources2 = page2_dict.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(
        resources2.get(b"Font").is_ok(),
        "Page 2 Font should be preserved"
    );
}

#[test]
fn test_compress_streams() {
    let mut doc = Document::with_version("1.5");

    // Create an uncompressed stream with repetitive data (compresses well)
    let data = b"Hello World! ".repeat(100);
    let stream = Stream::new(dictionary! {}, data.clone());
    let stream_id = doc.add_object(Object::Stream(stream));

    // Set up minimal valid document structure
    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => vec![],
        "Count" => 0,
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    pdf_masking::pdf::optimizer::compress_streams(&mut doc);

    // Verify the stream now has a FlateDecode filter
    let obj = doc.get_object(stream_id).unwrap();
    let compressed_stream = obj.as_stream().unwrap();
    let filter = compressed_stream
        .dict
        .get(b"Filter")
        .expect("Filter should be set");
    assert_eq!(
        filter.as_name().unwrap(),
        b"FlateDecode",
        "Filter should be FlateDecode"
    );

    // Compressed data should be smaller than original
    assert!(
        compressed_stream.content.len() < data.len(),
        "Compressed data ({}) should be smaller than original ({})",
        compressed_stream.content.len(),
        data.len()
    );
}

#[test]
fn test_compress_streams_skips_already_compressed() {
    let mut doc = Document::with_version("1.5");

    // Create a stream that already has a filter
    let data = b"Already compressed data".to_vec();
    let stream = Stream::new(
        dictionary! {
            "Filter" => "DCTDecode",
        },
        data.clone(),
    );
    let stream_id = doc.add_object(Object::Stream(stream));

    // Set up minimal valid document structure
    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => vec![],
        "Count" => 0,
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    pdf_masking::pdf::optimizer::compress_streams(&mut doc);

    // Verify the stream still has original filter, not double-compressed
    let obj = doc.get_object(stream_id).unwrap();
    let stream = obj.as_stream().unwrap();
    let filter = stream
        .dict
        .get(b"Filter")
        .expect("Filter should still be set");
    assert_eq!(
        filter.as_name().unwrap(),
        b"DCTDecode",
        "Filter should remain DCTDecode, not be overwritten"
    );

    // Data should be unchanged
    assert_eq!(stream.content, data, "Data should not be modified");
}

#[test]
fn test_delete_unused_objects() {
    let mut doc = Document::with_version("1.5");

    // Create a valid document structure
    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => vec![],
        "Count" => 0,
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    // Add an orphaned object (not referenced by anything)
    let orphan_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    // Verify orphan exists
    assert!(
        doc.get_object(orphan_id).is_ok(),
        "Orphan should exist before deletion"
    );

    let object_count_before = doc.objects.len();
    pdf_masking::pdf::optimizer::delete_unused_objects(&mut doc);

    // Verify orphan was removed
    assert!(
        doc.objects.len() < object_count_before,
        "Object count should decrease after removing orphans"
    );
    assert!(
        doc.get_object(orphan_id).is_err(),
        "Orphaned object should be removed"
    );
}

#[test]
fn test_optimize_full() {
    let mut doc = Document::with_version("1.5");

    let pages_id = doc.new_object_id();

    // Create a page with Font resources and an uncompressed content stream
    let content_data = b"BT /F1 12 Tf (Hello) Tj ET ".repeat(50);
    let content_stream = Stream::new(dictionary! {}, content_data.clone());
    let content_id = doc.add_object(Object::Stream(content_stream));

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F1" => dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                },
            },
        },
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    // Add an orphaned object
    let _orphan_id = doc.add_object(dictionary! {
        "Orphan" => "yes",
    });

    let object_count_before = doc.objects.len();

    // Run full optimization
    pdf_masking::pdf::optimizer::optimize(&mut doc, &[page_id]);

    // 1. Font should be removed from the masked page
    let page_dict = doc.get_dictionary(page_id).unwrap();
    let resources = page_dict.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(
        resources.get(b"Font").is_err(),
        "Font should be removed by optimize"
    );

    // 2. Content stream should be compressed
    let content_obj = doc.get_object(content_id).unwrap();
    let content_stream = content_obj.as_stream().unwrap();
    assert!(
        content_stream.dict.get(b"Filter").is_ok(),
        "Content stream should be compressed"
    );

    // 3. Orphaned object should be removed (object count should decrease)
    assert!(
        doc.objects.len() < object_count_before,
        "Orphaned objects should be removed by optimize"
    );
}
