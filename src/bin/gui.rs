use std::cell::RefCell;
use std::rc::Rc;

use slint::{Model, ModelRc, SharedString, VecModel};
use whashreonator::gui_app::{
    CompareTableRow, GuiController, ScanForm, ScanRunResult, ScanStartResult,
};

thread_local! {
    static COMPARE_ROWS_MODEL: RefCell<Option<Rc<VecModel<CompareRow>>>> = const { RefCell::new(None) };
}

slint::slint! {
    import { Button, CheckBox, ComboBox, HorizontalBox, LineEdit, ListView, ScrollView, TabWidget, TextEdit, VerticalBox } from "std-widgets.slint";

    struct CompareRow {
        resonator: string,
        item_type: string,
        status: string,
        confidence: string,
        path: string,
        asset_hash: string,
        shader_hash: string,
    }

    export component MainWindow inherits Window {
        preferred-width: 1320px;
        preferred-height: 900px;
        min-width: 900px;
        min-height: 600px;
        title: "WhashReonator Desktop";

        in-out property <string> status_text;
        in-out property <string> artifact_root_text;
        in-out property <string> reports_root_text;

        in-out property <string> source_root;
        in-out property <string> version_override;
        in-out property <string> knowledge_path;
        in-out property <bool> show_advanced;
        in-out property <bool> show_confirm_dialog;
        in-out property <string> confirm_dialog_text;

        in-out property <[string]> version_rows;
        in-out property <int> selected_version_index;
        in-out property <[string]> artifact_rows;
        in-out property <string> version_summary_text;

        in-out property <int> selected_compare_old_index;
        in-out property <int> selected_compare_new_index;
        in-out property <bool> compare_show_unchanged;
        in-out property <string> compare_summary_text;
        in-out property <[CompareRow]> compare_rows;
        in-out property <string> compare_gate_text;
        in-out property <string> compare_inference_text;
        in-out property <string> compare_proposal_text;
        in-out property <string> compare_human_summary_text;

        callback run_scan();
        callback confirm_rescan();
        callback cancel_rescan();
        callback refresh_versions();
        callback open_selected_version();
        callback compare_selected_versions();

        VerticalBox {
            padding: 12px;
            spacing: 8px;

            Text {
                text: "WhashReonator Desktop";
                font-size: 22px;
            }

            Text {
                text: root.status_text;
                color: #1b4d8a;
                wrap: word-wrap;
            }

            TabWidget {
                Tab {
                    title: "Scan Version";
                    VerticalBox {
                        spacing: 8px;

                        Text { text: "Game source root"; font-weight: 700; }
                        LineEdit { text <=> root.source_root; }

                        HorizontalBox {
                            spacing: 10px;
                            Button {
                                text: "Run Scan";
                                clicked => { root.run_scan(); }
                            }
                            CheckBox {
                                text: "Show advanced";
                                checked <=> root.show_advanced;
                            }
                        }

                        if root.show_advanced : VerticalBox {
                            spacing: 6px;
                            Text { text: "Version override (optional)"; }
                            LineEdit { text <=> root.version_override; }
                            Text { text: "WWMI knowledge JSON (optional)"; }
                            LineEdit { text <=> root.knowledge_path; }
                            Text { text: "Artifact root: " + root.artifact_root_text; wrap: word-wrap; font-size: 11px; color: #9ca3af; }
                            Text { text: "Version library root: " + root.reports_root_text; wrap: word-wrap; font-size: 11px; color: #9ca3af; }
                        }

                        Text { text: "Latest scan summary"; font-weight: 700; }
                        TextEdit {
                            text <=> root.version_summary_text;
                            read-only: true;
                            wrap: word-wrap;
                            vertical-stretch: 1;
                        }
                    }
                }

                Tab {
                    title: "Version Library";
                    VerticalBox {
                        spacing: 8px;
                        HorizontalBox {
                            spacing: 8px;
                            Button {
                                text: "Refresh";
                                clicked => { root.refresh_versions(); }
                            }
                            Button {
                                text: "Open Selected Version";
                                clicked => { root.open_selected_version(); }
                            }
                        }

                        HorizontalBox {
                            spacing: 10px;

                            VerticalBox {
                                horizontal-stretch: 1;
                                spacing: 4px;
                                Text { text: "Versions"; font-weight: 700; }
                                ListView {
                                    vertical-stretch: 1;
                                    min-height: 180px;
                                    for item[i] in root.version_rows : Rectangle {
                                        height: 26px;
                                        background: i == root.selected_version_index ? #1b4d8a
                                                  : mod(i, 2) == 0 ? #1f2937 : #111827;
                                        Text {
                                            text: item;
                                            color: white;
                                            vertical-alignment: center;
                                            x: 8px;
                                        }
                                        TouchArea {
                                            clicked => { root.selected_version_index = i; }
                                        }
                                    }
                                }

                                Text { text: "Artifacts"; font-weight: 700; }
                                ListView {
                                    min-height: 140px;
                                    for item[i] in root.artifact_rows : Rectangle {
                                        height: 22px;
                                        background: mod(i, 2) == 0 ? #1f2937 : #111827;
                                        Text {
                                            text: item;
                                            color: white;
                                            vertical-alignment: center;
                                            x: 8px;
                                            font-size: 12px;
                                        }
                                    }
                                }
                            }

                            VerticalBox {
                                horizontal-stretch: 2;
                                spacing: 4px;
                                Text { text: "Version summary"; font-weight: 700; }
                                TextEdit {
                                    text <=> root.version_summary_text;
                                    read-only: true;
                                    wrap: word-wrap;
                                    vertical-stretch: 1;
                                }
                            }
                        }
                    }
                }

                Tab {
                    title: "Compare Versions";
                    VerticalBox {
                        spacing: 6px;

                        HorizontalBox {
                            spacing: 10px;
                            alignment: start;

                            VerticalBox {
                                spacing: 2px;
                                Text { text: "Old"; font-size: 12px; color: #9ca3af; }
                                ComboBox {
                                    width: 200px;
                                    model: root.version_rows;
                                    current-index <=> root.selected_compare_old_index;
                                }
                            }
                            VerticalBox {
                                spacing: 2px;
                                Text { text: "New"; font-size: 12px; color: #9ca3af; }
                                ComboBox {
                                    width: 200px;
                                    model: root.version_rows;
                                    current-index <=> root.selected_compare_new_index;
                                }
                            }
                            VerticalBox {
                                spacing: 2px;
                                Text { text: ""; font-size: 12px; }
                                Button {
                                    text: "Compare";
                                    clicked => { root.compare_selected_versions(); }
                                }
                            }
                            VerticalBox {
                                spacing: 2px;
                                Text { text: ""; font-size: 12px; }
                                CheckBox {
                                    text: "Show unchanged";
                                    checked <=> root.compare_show_unchanged;
                                    toggled => { root.compare_selected_versions(); }
                                }
                            }
                        }

                        Text {
                            text: root.compare_summary_text;
                            wrap: word-wrap;
                            font-size: 12px;
                        }

                        Text { text: "Diff items"; font-weight: 700; }

                        Rectangle {
                            background: #1b4d8a;
                            height: 28px;
                            HorizontalLayout {
                                padding-left: 8px;
                                padding-right: 8px;
                                spacing: 8px;
                                Text { text: "Resonator"; color: white; font-weight: 700; horizontal-stretch: 2; vertical-alignment: center; overflow: elide; }
                                Text { text: "Type"; color: white; font-weight: 700; horizontal-stretch: 1; vertical-alignment: center; }
                                Text { text: "Status"; color: white; font-weight: 700; horizontal-stretch: 1; vertical-alignment: center; }
                                Text { text: "Conf"; color: white; font-weight: 700; horizontal-stretch: 1; vertical-alignment: center; }
                                Text { text: "Path"; color: white; font-weight: 700; horizontal-stretch: 5; vertical-alignment: center; overflow: elide; }
                                Text { text: "Asset hash"; color: white; font-weight: 700; horizontal-stretch: 6; vertical-alignment: center; overflow: elide; }
                                Text { text: "Shader hash"; color: white; font-weight: 700; horizontal-stretch: 6; vertical-alignment: center; overflow: elide; }
                            }
                        }

                        ListView {
                            vertical-stretch: 1;
                            min-height: 200px;
                            for row[i] in root.compare_rows : Rectangle {
                                height: 28px;
                                background: mod(i, 2) == 0 ? #1f2937 : #111827;
                                HorizontalLayout {
                                    padding-left: 8px;
                                    padding-right: 8px;
                                    spacing: 8px;
                                    Text { text: row.resonator; color: white; horizontal-stretch: 2; vertical-alignment: center; overflow: elide; }
                                    Text { text: row.item_type; color: white; horizontal-stretch: 1; vertical-alignment: center; overflow: elide; }
                                    Text {
                                        text: row.status;
                                        color: row.status == "Removed" ? rgb(252, 165, 165)
                                             : row.status == "Added" ? rgb(134, 239, 172)
                                             : row.status == "Changed" ? rgb(252, 211, 77)
                                             : white;
                                        horizontal-stretch: 1;
                                        vertical-alignment: center;
                                        overflow: elide;
                                    }
                                    Text { text: row.confidence; color: white; horizontal-stretch: 1; vertical-alignment: center; overflow: elide; }
                                    Text { text: row.path; color: white; horizontal-stretch: 5; vertical-alignment: center; overflow: elide; font-size: 12px; }
                                    Text { text: row.asset_hash; color: rgb(147, 197, 253); horizontal-stretch: 6; vertical-alignment: center; font-size: 12px; font-family: "Consolas"; }
                                    Text { text: row.shader_hash; color: rgb(196, 181, 253); horizontal-stretch: 6; vertical-alignment: center; font-size: 12px; font-family: "Consolas"; }
                                }
                            }
                        }

                        TabWidget {
                            height: 260px;
                            Tab {
                                title: "Quality / scope";
                                TextEdit {
                                    text <=> root.compare_gate_text;
                                    read-only: true;
                                    wrap: word-wrap;
                                }
                            }
                            Tab {
                                title: "Inference";
                                TextEdit {
                                    text <=> root.compare_inference_text;
                                    read-only: true;
                                    wrap: word-wrap;
                                }
                            }
                            Tab {
                                title: "Mapping proposal";
                                TextEdit {
                                    text <=> root.compare_proposal_text;
                                    read-only: true;
                                    wrap: word-wrap;
                                }
                            }
                            Tab {
                                title: "Human summary";
                                TextEdit {
                                    text <=> root.compare_human_summary_text;
                                    read-only: true;
                                    wrap: word-wrap;
                                }
                            }
                        }
                    }
                }
            }

        }

        if root.show_confirm_dialog : Rectangle {
            width: parent.width;
            height: parent.height;
            x: 0;
            y: 0;
            background: #00000099;

            TouchArea {
                width: parent.width;
                height: parent.height;
            }

            Rectangle {
                width: 480px;
                height: 220px;
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                background: #1e293b;
                border-color: #1b4d8a;
                border-width: 2px;
                border-radius: 10px;
                drop-shadow-color: #000000aa;
                drop-shadow-blur: 24px;
                drop-shadow-offset-y: 6px;

                VerticalBox {
                    padding: 20px;
                    spacing: 14px;

                    Text {
                        text: "Version already exists";
                        font-size: 16px;
                        font-weight: 700;
                        color: white;
                    }

                    Text {
                        text: root.confirm_dialog_text;
                        wrap: word-wrap;
                        vertical-stretch: 1;
                        color: #dbe2ea;
                    }

                    HorizontalBox {
                        spacing: 10px;
                        alignment: end;
                        Button {
                            text: "Cancel";
                            clicked => { root.cancel_rescan(); }
                        }
                        Button {
                            text: "Re-scan";
                            primary: true;
                            clicked => { root.confirm_rescan(); }
                        }
                    }
                }
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    // Slint + debug build có thể vượt stack 1MB mặc định của Windows khi init
    // MainWindow lớn. Chạy event loop trên thread riêng với stack 16MB.
    let handle = std::thread::Builder::new()
        .name("whashreonator-gui".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run_gui)
        .map_err(|error| {
            slint::PlatformError::Other(format!("failed to spawn gui thread: {error}"))
        })?;
    handle
        .join()
        .map_err(|_| slint::PlatformError::Other("gui thread panicked".to_string()))?
}

fn run_gui() -> Result<(), slint::PlatformError> {
    let controller = Rc::new(GuiController::default());
    let pending_scan = Rc::new(std::cell::RefCell::new(
        None::<whashreonator::scan::PreparedVersionScan>,
    ));
    let version_ids = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let version_rows_model = Rc::new(VecModel::<SharedString>::from(vec![]));
    let artifact_rows_model = Rc::new(VecModel::<SharedString>::from(vec![]));
    let compare_rows_model = Rc::new(VecModel::<CompareRow>::from(vec![]));
    COMPARE_ROWS_MODEL.with(|cell| *cell.borrow_mut() = Some(compare_rows_model.clone()));
    let window = MainWindow::new()?;

    window.set_status_text("Ready.".into());
    window.set_artifact_root_text(controller.artifact_root_label().into());
    window.set_reports_root_text(controller.reports_root_label().into());
    window.set_knowledge_path(
        whashreonator::output_policy::resolve_artifact_root()
            .join("wwmi-knowledge.json")
            .display()
            .to_string()
            .into(),
    );
    window.set_version_rows(ModelRc::from(version_rows_model.clone()));
    window.set_artifact_rows(ModelRc::from(artifact_rows_model.clone()));
    window.set_compare_rows(ModelRc::from(compare_rows_model.clone()));
    window.set_selected_version_index(-1);
    window.set_selected_compare_old_index(-1);
    window.set_selected_compare_new_index(-1);
    window.set_show_advanced(false);
    window.set_show_confirm_dialog(false);
    window.set_compare_show_unchanged(false);

    {
        let window = window.as_weak();
        let controller = controller.clone();
        let pending_scan = pending_scan.clone();
        let version_ids = version_ids.clone();
        let version_rows_model = version_rows_model.clone();
        let artifact_rows_model = artifact_rows_model.clone();
        window.unwrap().on_run_scan(move || {
            let Some(window) = window.upgrade() else {
                return;
            };

            window.set_status_text("Detecting version...".into());

            let form = ScanForm {
                source_root: window.get_source_root().to_string(),
                version_override: window.get_version_override().to_string(),
                knowledge_path: window.get_knowledge_path().to_string(),
            };

            match controller.prepare_scan(&form) {
                Ok(ScanStartResult::Ready(prepared)) => {
                    pending_scan.borrow_mut().replace(prepared.clone());
                    let scanned_version = apply_scan_result(
                        &window,
                        controller.run_scan(
                            &prepared,
                            false,
                            &window.get_knowledge_path().to_string(),
                        ),
                    );
                    pending_scan.borrow_mut().take();
                    refresh_version_library(
                        &window,
                        &controller,
                        &version_ids,
                        &version_rows_model,
                        &artifact_rows_model,
                    );
                    if let Some(version_id) = scanned_version {
                        select_version(&window, &version_ids, &version_id);
                        open_current_version(&window, &controller, &version_ids, &artifact_rows_model);
                    }
                }
                Ok(ScanStartResult::VersionAlreadyExists(prepared)) => {
                    pending_scan.borrow_mut().replace(prepared.clone());
                    window.set_status_text("Version already exists".into());
                    window.set_confirm_dialog_text(
                        format!(
                            "Version {} already exists.\nDo you want to re-scan and overwrite the stored snapshot if data changed?",
                            prepared.version_id
                        )
                        .into(),
                    );
                    window.set_show_confirm_dialog(true);
                }
                Err(error) => {
                    window.set_status_text(format!("Scan failed: {error}").into());
                }
            }
        });
    }

    {
        let window = window.as_weak();
        let controller = controller.clone();
        let pending_scan = pending_scan.clone();
        let version_ids = version_ids.clone();
        let version_rows_model = version_rows_model.clone();
        let artifact_rows_model = artifact_rows_model.clone();
        window.unwrap().on_confirm_rescan(move || {
            let Some(window) = window.upgrade() else {
                return;
            };

            let Some(prepared) = pending_scan.borrow().clone() else {
                window.set_show_confirm_dialog(false);
                window.set_status_text("No pending re-scan.".into());
                return;
            };

            window.set_show_confirm_dialog(false);
            window.set_status_text("Re-scan confirmed".into());
            let scanned_version = apply_scan_result(
                &window,
                controller.run_scan(&prepared, true, &window.get_knowledge_path().to_string()),
            );
            pending_scan.borrow_mut().take();
            refresh_version_library(
                &window,
                &controller,
                &version_ids,
                &version_rows_model,
                &artifact_rows_model,
            );
            if let Some(version_id) = scanned_version {
                select_version(&window, &version_ids, &version_id);
                open_current_version(&window, &controller, &version_ids, &artifact_rows_model);
            }
        });
    }

    {
        let window = window.as_weak();
        let pending_scan = pending_scan.clone();
        window.unwrap().on_cancel_rescan(move || {
            let Some(window) = window.upgrade() else {
                return;
            };

            pending_scan.borrow_mut().take();
            window.set_show_confirm_dialog(false);
            window.set_status_text("Re-scan cancelled".into());
        });
    }

    {
        let window = window.as_weak();
        let controller = controller.clone();
        let version_ids = version_ids.clone();
        let version_rows_model = version_rows_model.clone();
        let artifact_rows_model = artifact_rows_model.clone();
        window.unwrap().on_refresh_versions(move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            refresh_version_library(
                &window,
                &controller,
                &version_ids,
                &version_rows_model,
                &artifact_rows_model,
            );
        });
    }

    {
        let window = window.as_weak();
        let controller = controller.clone();
        let version_ids = version_ids.clone();
        let artifact_rows_model = artifact_rows_model.clone();
        window.unwrap().on_open_selected_version(move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            open_current_version(&window, &controller, &version_ids, &artifact_rows_model);
        });
    }

    {
        let window = window.as_weak();
        let controller_rc = controller.clone();
        let version_ids = version_ids.clone();
        let compare_rows_model = compare_rows_model.clone();
        window.unwrap().on_compare_selected_versions(move || {
            let Some(window_strong) = window.upgrade() else {
                return;
            };
            let window = window_strong;

            let old_index = window.get_selected_compare_old_index();
            let new_index = window.get_selected_compare_new_index();
            if old_index < 0 || new_index < 0 {
                window.set_status_text("Select both old/new version first.".into());
                return;
            }

            let (old_version, new_version) = {
                let ids = version_ids.borrow();
                let Some(old_version) = ids.get(old_index as usize).cloned() else {
                    window.set_status_text("Old version index is out of range.".into());
                    return;
                };
                let Some(new_version) = ids.get(new_index as usize).cloned() else {
                    window.set_status_text("New version index is out of range.".into());
                    return;
                };
                (old_version, new_version)
            };

            window.set_status_text(
                format!(
                    "Comparing wuwa_{} -> wuwa_{} ... please wait",
                    old_version, new_version
                )
                .into(),
            );
            window.set_compare_summary_text("Comparing, please wait...".into());
            compare_rows_model.set_vec(Vec::<CompareRow>::new());
            window.set_compare_gate_text("".into());
            window.set_compare_inference_text("".into());
            window.set_compare_proposal_text("".into());
            window.set_compare_human_summary_text("".into());

            let hide_unchanged = !window.get_compare_show_unchanged();
            let controller_owned = (*controller_rc).clone();
            let weak_for_thread = window.as_weak();
            std::thread::spawn(move || {
                let result = controller_owned.compare_versions(
                    &old_version,
                    &new_version,
                    "",
                    hide_unchanged,
                );
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak_for_thread.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(detail) => {
                            window.set_status_text(
                                format!("Compared wuwa_{} -> wuwa_{}.", old_version, new_version)
                                    .into(),
                            );
                            window.set_compare_summary_text(detail.summary.into());
                            let rows = detail
                                .table_rows
                                .into_iter()
                                .map(to_compare_row)
                                .collect::<Vec<_>>();
                            COMPARE_ROWS_MODEL.with(|cell| {
                                if let Some(model) = cell.borrow().as_ref() {
                                    model.set_vec(rows);
                                }
                            });
                            window.set_compare_gate_text(detail.quality_gate_text.into());
                            window.set_compare_inference_text(detail.inference_text.into());
                            window.set_compare_proposal_text(detail.proposal_text.into());
                            window.set_compare_human_summary_text(detail.human_summary_text.into());
                        }
                        Err(error) => {
                            window.set_status_text(format!("Compare failed: {error}").into());
                            window.set_compare_summary_text("".into());
                        }
                    }
                });
            });
        });
    }

    window.invoke_refresh_versions();
    window.run()
}

fn to_compare_row(row: CompareTableRow) -> CompareRow {
    CompareRow {
        resonator: row.resonator.into(),
        item_type: row.item_type.into(),
        status: row.status.into(),
        confidence: row.confidence.into(),
        path: row.path.into(),
        asset_hash: row.asset_hash.into(),
        shader_hash: row.shader_hash.into(),
    }
}

fn refresh_version_library(
    window: &MainWindow,
    controller: &GuiController,
    version_ids: &std::cell::RefCell<Vec<String>>,
    version_rows_model: &VecModel<SharedString>,
    artifact_rows_model: &VecModel<SharedString>,
) {
    match controller.list_versions() {
        Ok(versions) => {
            version_ids.borrow_mut().clear();
            let labels = versions
                .into_iter()
                .map(|version| {
                    version_ids.borrow_mut().push(version.version_id);
                    SharedString::from(version.label)
                })
                .collect::<Vec<_>>();
            version_rows_model.set_vec(labels);
            artifact_rows_model.set_vec(Vec::new());
            window.set_status_text(
                format!("Loaded {} version(s).", version_rows_model.row_count()).into(),
            );
            if version_rows_model.row_count() > 0 {
                window.set_selected_version_index(0);
                window.set_selected_compare_old_index(0);
                if version_rows_model.row_count() > 1 {
                    window.set_selected_compare_new_index(1);
                } else {
                    window.set_selected_compare_new_index(0);
                }
            }
        }
        Err(error) => {
            window.set_status_text(format!("Version refresh failed: {error}").into());
        }
    }
}

fn open_current_version(
    window: &MainWindow,
    controller: &GuiController,
    version_ids: &std::cell::RefCell<Vec<String>>,
    artifact_rows_model: &VecModel<SharedString>,
) {
    let index = window.get_selected_version_index();
    if index < 0 {
        window.set_status_text("No version selected.".into());
        return;
    }

    let ids = version_ids.borrow();
    let Some(version_id) = ids.get(index as usize) else {
        window.set_status_text("Selected version index is out of range.".into());
        return;
    };

    match controller.open_version(version_id) {
        Ok(detail) => {
            artifact_rows_model.set_vec(
                detail
                    .artifacts
                    .into_iter()
                    .map(SharedString::from)
                    .collect::<Vec<_>>(),
            );
            window.set_version_summary_text(detail.summary.into());
            window.set_status_text(format!("Opened version wuwa_{}.", version_id).into());
        }
        Err(error) => {
            window.set_status_text(format!("Open version failed: {error}").into());
        }
    }
}

fn select_version(
    window: &MainWindow,
    version_ids: &std::cell::RefCell<Vec<String>>,
    version_id: &str,
) {
    let ids = version_ids.borrow();
    if let Some(index) = ids.iter().position(|item| item == version_id) {
        window.set_selected_version_index(index as i32);
    }
}

fn apply_scan_result(
    window: &MainWindow,
    result: Result<ScanRunResult, whashreonator::error::AppError>,
) -> Option<String> {
    match result {
        Ok(ScanRunResult::Created {
            version_id,
            saved_path,
            summary,
        }) => {
            window.set_status_text(
                format!(
                    "Scan completed for version {version_id}. Saved: {}",
                    saved_path.display()
                )
                .into(),
            );
            window.set_version_summary_text(summary.into());
            Some(version_id)
        }
        Ok(ScanRunResult::NoChangesDetected {
            version_id,
            saved_path,
            summary,
        }) => {
            window.set_status_text("No changes detected".into());
            window.set_version_summary_text(
                format!("{summary}\nStored snapshot: {}", saved_path.display()).into(),
            );
            Some(version_id)
        }
        Ok(ScanRunResult::Overwritten {
            version_id,
            saved_path,
            summary,
        }) => {
            window.set_status_text("Overwritten successfully".into());
            window.set_version_summary_text(
                format!("{summary}\nOverwritten snapshot: {}", saved_path.display()).into(),
            );
            Some(version_id)
        }
        Err(error) => {
            window.set_status_text(format!("Scan failed: {error}").into());
            None
        }
    }
}
