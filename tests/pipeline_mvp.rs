use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{config::AppConfig, pipeline::run_map};

#[test]
fn pipeline_exports_expected_json_report() {
    let test_root = unique_test_dir();
    fs::create_dir_all(&test_root).expect("create temp test dir");

    let input_path = test_root.join("input.json");
    let output_path = test_root.join("out").join("report.json");

    fs::write(
        &input_path,
        r#"
        {
          "old_assets": [
            {
              "id": "old-body",
              "path": "Content/Character/HeroA/Body.mesh",
              "kind": "mesh",
              "metadata": {
                "logical_name": "HeroA_Body",
                "vertex_count": 12000,
                "index_count": 18000,
                "material_slots": 3,
                "section_count": 2,
                "tags": ["hero", "body", "playable"]
              }
            },
            {
              "id": "old-weapon",
              "path": "Content/Weapon/Sword.weapon",
              "kind": "mesh",
              "metadata": {
                "logical_name": "Sword_Main",
                "vertex_count": 1000,
                "index_count": 2000,
                "material_slots": 1,
                "section_count": 1,
                "tags": ["weapon"]
              }
            }
          ],
          "new_assets": [
            {
              "id": "new-body",
              "path": "Content/Character/HeroA/Body_v2.mesh",
              "kind": "mesh",
              "metadata": {
                "logical_name": "HeroA_Body",
                "vertex_count": 12000,
                "index_count": 18000,
                "material_slots": 3,
                "section_count": 2,
                "tags": ["hero", "body", "playable"]
              }
            },
            {
              "id": "new-weapon-ambiguous",
              "path": "Content/Weapon/Sword.weapon",
              "kind": "mesh",
              "metadata": {
                "logical_name": "Sword_Main",
                "vertex_count": 1000,
                "index_count": 2000,
                "material_slots": 1,
                "section_count": 1,
                "tags": ["weapon"]
              }
            },
            {
              "id": "new-weapon-ambiguous-2",
              "path": "Content/Weapon/Sword_Copy.weapon",
              "kind": "mesh",
              "metadata": {
                "logical_name": "Sword_Main",
                "vertex_count": 1000,
                "index_count": 2000,
                "material_slots": 1,
                "section_count": 1,
                "tags": ["weapon"]
              }
            }
          ]
        }
        "#,
    )
    .expect("write input");

    let report = run_map(&input_path, &output_path, &AppConfig::default()).expect("run pipeline");
    let output = fs::read_to_string(&output_path).expect("read output");

    assert_eq!(report.summary.total_old_assets, 2);
    assert!(output.contains("\"status\": \"matched\""));
    assert!(output.contains("\"status\": \"needs_review\""));
    assert!(output.contains("\"confidence\""));
    assert!(output.contains("\"reasons\""));
    assert!(!output.contains("\"schema_version\""));

    let _ = fs::remove_dir_all(&test_root);
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-test-{nanos}"))
}
