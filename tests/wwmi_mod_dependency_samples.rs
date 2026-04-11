use std::path::PathBuf;

use whashreonator::wwmi::dependency::{WwmiModDependencyKind, scan_mod_dependency_profile};

#[test]
fn real_wwmi_mod_samples_expose_repair_relevant_dependency_categories() {
    let mods_root = PathBuf::from("D:/mod/WWMI/Mods");

    let aemeth = scan_mod_dependency_profile(&mods_root.join("Aemeth")).expect("scan Aemeth");
    assert!(aemeth.has_kind(WwmiModDependencyKind::TextureOverrideHash));
    assert!(aemeth.has_kind(WwmiModDependencyKind::ResourceFileReference));

    let augusta =
        scan_mod_dependency_profile(&mods_root.join("AugustaMod")).expect("scan AugustaMod");
    assert!(augusta.has_kind(WwmiModDependencyKind::ObjectGuid));
    assert!(augusta.has_kind(WwmiModDependencyKind::DrawCallTarget));
    assert!(augusta.has_kind(WwmiModDependencyKind::MeshVertexCount));
    assert!(augusta.has_kind(WwmiModDependencyKind::ShapeKeyVertexCount));

    let buling = scan_mod_dependency_profile(&mods_root.join("Buling")).expect("scan Buling");
    assert!(buling.has_kind(WwmiModDependencyKind::ObjectGuid));
    assert!(buling.has_kind(WwmiModDependencyKind::DrawCallTarget));
    assert!(buling.has_kind(WwmiModDependencyKind::SkeletonMergeDependency));
    assert!(buling.has_kind(WwmiModDependencyKind::BufferLayoutHint));

    let carlotta =
        scan_mod_dependency_profile(&mods_root.join("CarlottaMod")).expect("scan CarlottaMod");
    assert!(carlotta.has_kind(WwmiModDependencyKind::ObjectGuid));
    assert!(carlotta.has_kind(WwmiModDependencyKind::DrawCallTarget));
    assert!(carlotta.has_kind(WwmiModDependencyKind::FilterIndex));
    assert!(carlotta.has_kind(WwmiModDependencyKind::SkeletonMergeDependency));
    assert!(carlotta.has_kind(WwmiModDependencyKind::BufferLayoutHint));
}
