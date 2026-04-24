use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{cli::ArchiveDumpArgs, pipeline::run_archive_dump_command};

#[test]
fn archives_synthetic_dump_to_expected_folder_and_writes_meta() {
    let test_root = unique_test_dir();
    let raw_dump = test_root.join("FrameAnalysis-synthetic");
    seed_raw_dump(&raw_dump);
    let archive_root = test_root.join("archive");

    run_archive_dump_command(ArchiveDumpArgs {
        raw_dump_dir: raw_dump.clone(),
        character: "ameath".to_string(),
        version: "3.2.2".to_string(),
        archive_root: archive_root.clone(),
    })
    .expect("archive command should succeed");

    let character_dir = archive_root.join("wuwa_3.2.2").join("ameath");
    let entries: Vec<_> = fs::read_dir(&character_dir)
        .expect("read character dir")
        .filter_map(|entry| entry.ok())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one timestamped archive subfolder under {}",
        character_dir.display()
    );
    let archive_subfolder = entries[0].path();
    assert!(archive_subfolder.is_dir());

    let archived_log = archive_subfolder.join("log.txt");
    assert!(archived_log.exists(), "archived log.txt must exist");
    let log_text = fs::read_to_string(&archived_log).expect("read archived log");
    assert!(log_text.starts_with("analyse_options"));

    let meta_path = archive_subfolder.join("dump_meta.json");
    let meta_text = fs::read_to_string(&meta_path).expect("read dump_meta.json");
    assert!(meta_text.contains("\"whashreonator.fa-dump-archive.v1\""));
    assert!(meta_text.contains("\"character\": \"ameath\""));
    assert!(meta_text.contains("\"version_id\": \"3.2.2\""));
    assert!(meta_text.contains("\"raw_dump_size_bytes\":"));
    assert!(meta_text.contains("\"archived_log_size_bytes\":"));

    let raw_log_unchanged = fs::read_to_string(raw_dump.join("log.txt"))
        .expect("raw dump log.txt should still exist after archive");
    assert_eq!(raw_log_unchanged, log_text);

    cleanup(&test_root);
}

#[test]
fn rejects_missing_log_txt() {
    let test_root = unique_test_dir();
    let raw_dump = test_root.join("EmptyDump");
    fs::create_dir_all(&raw_dump).expect("create empty dump dir");
    let archive_root = test_root.join("archive");

    let error = run_archive_dump_command(ArchiveDumpArgs {
        raw_dump_dir: raw_dump,
        character: "ameath".to_string(),
        version: "3.2.2".to_string(),
        archive_root,
    })
    .expect_err("dump directory without log.txt should be rejected");

    assert!(
        error.to_string().contains("must contain log.txt"),
        "unexpected error message: {error}"
    );

    cleanup(&test_root);
}

#[test]
fn rejects_empty_character_or_version_label() {
    let test_root = unique_test_dir();
    let raw_dump = test_root.join("FrameAnalysis-synthetic");
    seed_raw_dump(&raw_dump);
    let archive_root = test_root.join("archive");

    let error = run_archive_dump_command(ArchiveDumpArgs {
        raw_dump_dir: raw_dump.clone(),
        character: "   ".to_string(),
        version: "3.2.2".to_string(),
        archive_root: archive_root.clone(),
    })
    .expect_err("empty character label should be rejected");
    assert!(error.to_string().contains("character must not be empty"));

    let error = run_archive_dump_command(ArchiveDumpArgs {
        raw_dump_dir: raw_dump,
        character: "ameath".to_string(),
        version: "".to_string(),
        archive_root,
    })
    .expect_err("empty version label should be rejected");
    assert!(error.to_string().contains("version must not be empty"));

    cleanup(&test_root);
}

#[test]
fn rejects_labels_with_path_separator_or_reserved_characters() {
    let test_root = unique_test_dir();
    let raw_dump = test_root.join("FrameAnalysis-synthetic");
    seed_raw_dump(&raw_dump);
    let archive_root = test_root.join("archive");

    for bad in [
        "../escape",
        "ameath/body",
        "ameath\\body",
        "ameath:body",
        "ameath*body",
    ] {
        let error = run_archive_dump_command(ArchiveDumpArgs {
            raw_dump_dir: raw_dump.clone(),
            character: bad.to_string(),
            version: "3.2.2".to_string(),
            archive_root: archive_root.clone(),
        })
        .expect_err(&format!("character label {bad:?} should be rejected"));
        assert!(
            error
                .to_string()
                .contains("must not contain path separator"),
            "unexpected error for {bad:?}: {error}"
        );
    }

    cleanup(&test_root);
}

#[test]
fn rejects_archive_root_under_src_or_tests() {
    let raw_dump = unique_test_dir().join("FrameAnalysis-src-reject");
    seed_raw_dump(&raw_dump);

    let error = run_archive_dump_command(ArchiveDumpArgs {
        raw_dump_dir: raw_dump.clone(),
        character: "ameath".to_string(),
        version: "3.2.2".to_string(),
        archive_root: PathBuf::from("src/archive"),
    })
    .expect_err("archive-root under src/ should be rejected");
    assert!(
        error.to_string().contains("not allowed under src"),
        "unexpected error: {error}"
    );

    let _ = fs::remove_dir_all(raw_dump.parent().unwrap_or(Path::new("")));
}

#[test]
fn repeated_archives_create_distinct_timestamp_folders() {
    let test_root = unique_test_dir();
    let raw_dump = test_root.join("FrameAnalysis-synthetic");
    seed_raw_dump(&raw_dump);
    let archive_root = test_root.join("archive");

    for _ in 0..2 {
        run_archive_dump_command(ArchiveDumpArgs {
            raw_dump_dir: raw_dump.clone(),
            character: "ameath".to_string(),
            version: "3.2.2".to_string(),
            archive_root: archive_root.clone(),
        })
        .expect("archive command should succeed");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let character_dir = archive_root.join("wuwa_3.2.2").join("ameath");
    let subfolders: Vec<_> = fs::read_dir(&character_dir)
        .expect("read character dir")
        .filter_map(|entry| entry.ok())
        .collect();
    assert!(
        subfolders.len() >= 2,
        "expected at least two archive subfolders, got {}",
        subfolders.len()
    );

    cleanup(&test_root);
}

fn seed_raw_dump(raw_dump: &Path) {
    fs::create_dir_all(raw_dump).expect("create raw dump dir");
    fs::write(
        raw_dump.join("log.txt"),
        "analyse_options: 0000063d\n000001 DrawIndexed(IndexCount:6, StartIndexLocation:0, BaseVertexLocation:0)\n",
    )
    .expect("write fake log.txt");
    fs::write(
        raw_dump.join("000001-vb=deadbeef-vs=cafebabe.buf"),
        vec![0u8; 1024],
    )
    .expect("write fake .buf to simulate a heavy raw dump");
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();
    std::env::temp_dir().join(format!("whashreonator-archive-dump-test-{nanos}"))
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
