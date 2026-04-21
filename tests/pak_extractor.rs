use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::ExtractPakArgs,
    ingest::pak_extractor::{AesKey, PakExtractionManifest, PakExtractionRequest, extract_pak},
    pipeline::run_extract_pak_command,
};

#[test]
fn extracts_synthetic_unencrypted_pak_to_temp_dir() {
    let test_root = unique_test_dir("extract");
    let pak_path = test_root.join("sample.pak");
    build_sample_pak(&pak_path, false);
    let output_dir = test_root.join("out").join("extracted");

    let result = extract_pak(PakExtractionRequest {
        pak_path: pak_path.clone(),
        aes_key: None,
        output_dir: output_dir.clone(),
        path_filter: None,
    })
    .expect("extract synthetic pak");

    assert_eq!(result.manifest.summary.total_entries_in_pak, 5);
    assert_eq!(result.manifest.summary.extracted_entries, 5);
    assert_eq!(result.manifest.summary.filtered_out_entries, 0);
    assert_eq!(result.manifest.entries.len(), 5);

    assert_file_bytes(
        &output_dir.join("Content/Character/Encore/Body.uasset"),
        &fixture_bytes(10 * 1024, 0x11),
    );
    assert_file_bytes(
        &output_dir.join("Content/Character/Encore/Body.uexp"),
        &fixture_bytes(50 * 1024, 0x22),
    );
    assert_file_bytes(
        &output_dir.join("Content/Character/Carlotta/Hair.uasset"),
        &fixture_bytes(8 * 1024, 0x33),
    );
    assert_file_bytes(
        &output_dir.join("Engine/Binaries/Win64/dummy.dll"),
        &fixture_bytes(4 * 1024, 0x44),
    );
    assert_file_bytes(
        &output_dir.join("Audio/Music/dummy.wem"),
        &fixture_bytes(12 * 1024, 0x55),
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn applies_path_filter_substring_match() {
    let test_root = unique_test_dir("filter");
    let pak_path = test_root.join("sample.pak");
    build_sample_pak(&pak_path, false);
    let output_dir = test_root.join("out").join("characters");

    let result = extract_pak(PakExtractionRequest {
        pak_path,
        aes_key: None,
        output_dir: output_dir.clone(),
        path_filter: Some("Content/Character/**".to_string()),
    })
    .expect("extract filtered pak");

    assert_eq!(result.manifest.summary.total_entries_in_pak, 5);
    assert_eq!(result.manifest.summary.extracted_entries, 3);
    assert_eq!(result.manifest.summary.filtered_out_entries, 2);
    assert!(
        output_dir
            .join("Content/Character/Encore/Body.uasset")
            .exists()
    );
    assert!(
        output_dir
            .join("Content/Character/Encore/Body.uexp")
            .exists()
    );
    assert!(
        output_dir
            .join("Content/Character/Carlotta/Hair.uasset")
            .exists()
    );
    assert!(!output_dir.join("Engine/Binaries/Win64/dummy.dll").exists());
    assert!(!output_dir.join("Audio/Music/dummy.wem").exists());

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn aes_key_from_hex_accepts_with_and_without_prefix() {
    let with_prefix =
        AesKey::from_hex("0xE0D4C0AA387A268B29C397E3C0CAD934522EFC96BE5526D6288EA26351CDACC9")
            .expect("parse prefixed key");
    let without_prefix =
        AesKey::from_hex("E0D4C0AA387A268B29C397E3C0CAD934522EFC96BE5526D6288EA26351CDACC9")
            .expect("parse plain key");

    assert_eq!(with_prefix, without_prefix);
}

#[test]
fn aes_key_from_hex_rejects_invalid_length_or_chars() {
    assert!(AesKey::from_hex("AABB").is_err());
    assert!(
        AesKey::from_hex("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX")
            .is_err()
    );
}

#[test]
fn aes_key_does_not_appear_in_debug_or_display() {
    let hex = "E0D4C0AA387A268B29C397E3C0CAD934522EFC96BE5526D6288EA26351CDACC9";
    let key = AesKey::from_hex(hex).expect("parse key");
    let rendered = format!("{key:?}");

    assert!(!rendered.contains(hex));
    assert!(rendered.contains("redacted"));
}

#[test]
fn manifest_serializes_to_expected_schema_version() {
    let test_root = unique_test_dir("manifest");
    let pak_path = test_root.join("sample.pak");
    build_sample_pak(&pak_path, false);
    let output_dir = test_root.join("out").join("manifest");

    let result = extract_pak(PakExtractionRequest {
        pak_path,
        aes_key: None,
        output_dir,
        path_filter: None,
    })
    .expect("extract pak");

    let parsed: PakExtractionManifest =
        serde_json::from_str(&fs::read_to_string(&result.manifest_path).expect("read manifest"))
            .expect("parse manifest");

    assert_eq!(
        parsed.schema_version,
        "whashreonator.pak-extraction-manifest.v1"
    );
    assert!(!parsed.aes_key_used);

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn cli_command_writes_manifest_under_temp_dir() {
    let pak_path = ensure_sample_smoke_fixture();
    let test_root = unique_test_dir("cli");
    let output_dir = test_root.join("out").join("cli-extract");

    run_extract_pak_command(ExtractPakArgs {
        pak: pak_path,
        aes_key: None,
        aes_key_file: None,
        output_dir: output_dir.clone(),
        path_filter: Some("Content/Character/".to_string()),
    })
    .expect("run extract-pak command");

    let manifest_path = output_dir.join("extraction_manifest.v1.json");
    assert!(manifest_path.exists());

    let parsed: PakExtractionManifest =
        serde_json::from_str(&fs::read_to_string(manifest_path).expect("read manifest"))
            .expect("parse manifest");
    assert!(!parsed.entries.is_empty());
    assert!(parsed.summary.extracted_entries >= 1);

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn cli_command_rejects_writing_into_src_or_tests() {
    let pak_path = ensure_sample_smoke_fixture();

    let error = run_extract_pak_command(ExtractPakArgs {
        pak: pak_path,
        aes_key: None,
        aes_key_file: None,
        output_dir: PathBuf::from("src/extracted"),
        path_filter: None,
    })
    .expect_err("src output path should be rejected");

    assert!(error.to_string().contains("not allowed under src"));
}

#[test]
fn pak_entry_with_path_traversal_is_rejected() {
    let test_root = unique_test_dir("traversal");
    let pak_path = test_root.join("traversal.pak");
    build_sample_pak(&pak_path, true);
    let output_dir = test_root.join("out").join("traversal");

    let result = extract_pak(PakExtractionRequest {
        pak_path,
        aes_key: None,
        output_dir: output_dir.clone(),
        path_filter: None,
    })
    .expect("extract traversal pak");

    assert_eq!(result.manifest.summary.total_entries_in_pak, 6);
    assert_eq!(result.manifest.summary.extracted_entries, 5);
    assert_eq!(result.manifest.summary.filtered_out_entries, 1);
    assert!(!test_root.join("escaped.txt").exists());
    assert!(!output_dir.join("..").join("escaped.txt").exists());

    let _ = fs::remove_dir_all(&test_root);
}

fn build_sample_pak(path: &Path, include_traversal: bool) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create pak parent");
    }

    let file = fs::File::create(path).expect("create pak file");
    let mut writer = repak::PakBuilder::new().writer(
        file,
        repak::Version::V11,
        "../../../".to_string(),
        Some(0x205C5A7D),
    );

    for (entry_path, data) in sample_entries(include_traversal) {
        writer
            .write_file(&entry_path, false, data)
            .expect("write pak entry");
    }
    writer.write_index().expect("write pak index");
}

fn sample_entries(include_traversal: bool) -> Vec<(String, Vec<u8>)> {
    let mut entries = vec![
        (
            "Content/Character/Encore/Body.uasset".to_string(),
            fixture_bytes(10 * 1024, 0x11),
        ),
        (
            "Content/Character/Encore/Body.uexp".to_string(),
            fixture_bytes(50 * 1024, 0x22),
        ),
        (
            "Content/Character/Carlotta/Hair.uasset".to_string(),
            fixture_bytes(8 * 1024, 0x33),
        ),
        (
            "Engine/Binaries/Win64/dummy.dll".to_string(),
            fixture_bytes(4 * 1024, 0x44),
        ),
        (
            "Audio/Music/dummy.wem".to_string(),
            fixture_bytes(12 * 1024, 0x55),
        ),
    ];

    if include_traversal {
        entries.push(("../escaped.txt".to_string(), fixture_bytes(1024, 0x66)));
    }

    entries
}

fn ensure_sample_smoke_fixture() -> PathBuf {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_fixtures")
        .join("sample.pak");

    if !fixture_path.exists() {
        build_sample_pak(&fixture_path, false);
    }

    fixture_path
}

fn assert_file_bytes(path: &Path, expected: &[u8]) {
    let actual = fs::read(path).expect("read extracted file");
    assert_eq!(actual, expected);
}

fn fixture_bytes(size: usize, fill: u8) -> Vec<u8> {
    vec![fill; size]
}

fn unique_test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();
    std::env::temp_dir().join(format!("whashreonator-pak-{label}-{nanos}"))
}
