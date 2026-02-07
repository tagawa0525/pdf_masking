// Phase 0: プロジェクト基盤テスト

#[test]
fn test_cargo_dependencies_present() {
    // Cargo.tomlに必要な依存関係が全て定義されていることを確認
    let manifest = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml should exist");

    let required_deps = [
        "thiserror",
        "serde",
        "serde_yaml",
        "rayon",
        "sha2",
        "hex",
        "anyhow",
    ];

    for dep in required_deps {
        assert!(
            manifest.contains(dep),
            "Cargo.toml should contain dependency: {}",
            dep
        );
    }
}

#[test]
fn test_error_module_compiles() {
    // error.rs モジュールがコンパイルできることを確認
    // 実際のコンパイルテストはcargo buildで行われる
}

#[test]
fn test_all_module_stubs_exist() {
    // 全モジュールファイルが存在することを確認
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
