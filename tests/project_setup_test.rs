// Phase 0: プロジェクト基盤テスト

#[test]
fn test_cargo_dependencies_present() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml should exist");

    // [dependencies] セクション内のキー名として存在するか確認
    // 行頭が依存名で始まるパターンでマッチし、部分文字列の偽陽性を防ぐ
    let required_deps = [
        "thiserror",
        "serde ", // "serde_yml" と区別するためスペース付き
        "serde_yml",
        "rayon",
        "sha2",
        "hex",
    ];

    for dep in required_deps {
        let dep_trimmed = dep.trim();
        let found = manifest.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with(dep_trimmed)
                && trimmed[dep_trimmed.len()..].starts_with([' ', '=', '.'])
        });
        assert!(
            found,
            "Cargo.toml should contain dependency: {}",
            dep_trimmed
        );
    }
}

#[test]
fn test_all_module_stubs_exist() {
    let module_paths = [
        "src/error.rs",
        "src/config/mod.rs",
        "src/config/settings.rs",
        "src/config/job.rs",
        "src/config/merged.rs",
        "src/pdf/mod.rs",
        "src/pdf/reader.rs",
        "src/pdf/content_stream.rs",
        "src/pdf/writer.rs",
        "src/pdf/optimizer.rs",
        "src/pdf/image_xobject.rs",
        "src/render/mod.rs",
        "src/render/pdfium.rs",
        "src/mrc/mod.rs",
        "src/mrc/segmenter.rs",
        "src/mrc/jbig2.rs",
        "src/mrc/jpeg.rs",
        "src/mrc/compositor.rs",
        "src/ffi/mod.rs",
        "src/ffi/leptonica_sys.rs",
        "src/ffi/leptonica.rs",
        "src/ffi/jbig2enc_sys.rs",
        "src/ffi/jbig2enc.rs",
        "src/cache/mod.rs",
        "src/cache/hash.rs",
        "src/cache/store.rs",
        "src/pipeline/mod.rs",
        "src/pipeline/page_processor.rs",
        "src/pipeline/job_runner.rs",
        "src/pipeline/orchestrator.rs",
        "src/linearize.rs",
    ];

    for path in module_paths {
        assert!(
            std::path::Path::new(path).exists(),
            "Module file should exist: {}",
            path
        );
    }
}
