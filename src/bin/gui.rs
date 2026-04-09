use std::rc::Rc;

use slint::{Model, ModelRc, SharedString, VecModel};
use whashreonator::gui_app::{GuiController, ScanForm, ScanRunResult, ScanStartResult};

slint::slint! {
    import { Button, CheckBox, HorizontalBox, LineEdit, StandardListView, TabWidget, TextEdit, VerticalBox } from "std-widgets.slint";

    export component MainWindow inherits Window {
        width: 1320px;
        height: 900px;
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
        in-out property <string> compare_summary_text;
        in-out property <string> compare_old_text;
        in-out property <string> compare_new_text;

        callback run_scan();
        callback confirm_rescan();
        callback cancel_rescan();
        callback refresh_versions();
        callback open_selected_version();
        callback compare_selected_versions();

        VerticalBox {
            padding: 12px;
            spacing: 10px;

            Text {
                text: "WhashReonator Desktop";
                font-size: 24px;
            }

            Text {
                text: "Version-oriented report storage and compare workflow.";
                wrap: word-wrap;
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
                        Text { text: "Artifact root"; }
                        Text { text: root.artifact_root_text; wrap: word-wrap; }
                        Text { text: "Version library root"; }
                        Text { text: root.reports_root_text; wrap: word-wrap; }

                        Text { text: "Game source root"; }
                        LineEdit { text <=> root.source_root; }

                        CheckBox {
                            text: "Show Advanced";
                            checked <=> root.show_advanced;
                        }

                        if root.show_advanced : VerticalBox {
                            spacing: 6px;
                            Text { text: "Version override (optional)"; }
                            LineEdit { text <=> root.version_override; }
                            Text { text: "WWMI knowledge JSON (optional, reserved)"; }
                            LineEdit { text <=> root.knowledge_path; }
                        }

                        Button {
                            text: "Run Scan";
                            clicked => { root.run_scan(); }
                        }

                        Text { text: "Latest scan summary"; }
                        TextEdit {
                            text <=> root.version_summary_text;
                            read-only: true;
                            wrap: word-wrap;
                            height: 180px;
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
                                text: "Refresh Versions";
                                clicked => { root.refresh_versions(); }
                            }
                            Button {
                                text: "Open Selected Version";
                                clicked => { root.open_selected_version(); }
                            }
                        }

                        Text { text: "Versions"; }
                        StandardListView {
                            for item[i] in root.version_rows : Text {
                                text: item;
                                vertical-alignment: center;
                            }
                            current-item <=> root.selected_version_index;
                            height: 220px;
                        }

                        Text { text: "Artifacts in selected version"; }
                        StandardListView {
                            for item[i] in root.artifact_rows : Text {
                                text: item;
                                vertical-alignment: center;
                            }
                            height: 170px;
                        }

                        Text { text: "Version summary"; }
                        TextEdit {
                            text <=> root.version_summary_text;
                            read-only: true;
                            wrap: word-wrap;
                            height: 180px;
                        }
                    }
                }

                Tab {
                    title: "Compare Versions";
                    VerticalBox {
                        spacing: 8px;
                        Text { text: "Select two versions from library data and compare."; wrap: word-wrap; }
                        HorizontalBox {
                            spacing: 12px;
                            VerticalBox {
                                spacing: 6px;
                                Text { text: "Old version"; }
                                StandardListView {
                                    for item[i] in root.version_rows : Text {
                                        text: item;
                                        vertical-alignment: center;
                                    }
                                    current-item <=> root.selected_compare_old_index;
                                    height: 180px;
                                }
                            }
                            VerticalBox {
                                spacing: 6px;
                                Text { text: "New version"; }
                                StandardListView {
                                    for item[i] in root.version_rows : Text {
                                        text: item;
                                        vertical-alignment: center;
                                    }
                                    current-item <=> root.selected_compare_new_index;
                                    height: 180px;
                                }
                            }
                        }

                        Button {
                            text: "Compare Selected Versions";
                            clicked => { root.compare_selected_versions(); }
                        }

                        Text { text: "Compare summary"; }
                        TextEdit {
                            text <=> root.compare_summary_text;
                            read-only: true;
                            wrap: word-wrap;
                            height: 120px;
                        }

                        HorizontalBox {
                            spacing: 12px;
                            VerticalBox {
                                spacing: 6px;
                                Text { text: "Old version detail"; }
                                TextEdit {
                                    text <=> root.compare_old_text;
                                    read-only: true;
                                    wrap: word-wrap;
                                }
                            }
                            VerticalBox {
                                spacing: 6px;
                                Text { text: "New version detail"; }
                                TextEdit {
                                    text <=> root.compare_new_text;
                                    read-only: true;
                                    wrap: word-wrap;
                                }
                            }
                        }
                    }
                }
            }

            if root.show_confirm_dialog : Rectangle {
                background: #ddf0f8;
                border-color: #1b4d8a;
                border-width: 1px;
                border-radius: 8px;
                min-height: 120px;

                VerticalBox {
                    padding: 12px;
                    spacing: 10px;
                    Text {
                        text: root.confirm_dialog_text;
                        wrap: word-wrap;
                    }
                    HorizontalBox {
                        spacing: 8px;
                        Button {
                            text: "Re-scan";
                            clicked => { root.confirm_rescan(); }
                        }
                        Button {
                            text: "Cancel";
                            clicked => { root.cancel_rescan(); }
                        }
                    }
                }
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let controller = Rc::new(GuiController::default());
    let pending_scan = Rc::new(std::cell::RefCell::new(
        None::<whashreonator::scan::PreparedVersionScan>,
    ));
    let version_ids = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let version_rows_model = Rc::new(VecModel::<SharedString>::from(vec![]));
    let artifact_rows_model = Rc::new(VecModel::<SharedString>::from(vec![]));
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
    window.set_selected_version_index(-1);
    window.set_selected_compare_old_index(-1);
    window.set_selected_compare_new_index(-1);
    window.set_show_advanced(false);
    window.set_show_confirm_dialog(false);

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
        let controller = controller.clone();
        let version_ids = version_ids.clone();
        window.unwrap().on_compare_selected_versions(move || {
            let Some(window) = window.upgrade() else {
                return;
            };

            let old_index = window.get_selected_compare_old_index();
            let new_index = window.get_selected_compare_new_index();
            if old_index < 0 || new_index < 0 {
                window.set_status_text("Select both old/new version first.".into());
                return;
            }

            let ids = version_ids.borrow();
            let Some(old_version) = ids.get(old_index as usize) else {
                window.set_status_text("Old version index is out of range.".into());
                return;
            };
            let Some(new_version) = ids.get(new_index as usize) else {
                window.set_status_text("New version index is out of range.".into());
                return;
            };

            match controller.compare_versions(old_version, new_version, "") {
                Ok(detail) => {
                    window.set_status_text(
                        format!("Compared versions {} -> {}.", old_version, new_version).into(),
                    );
                    window.set_compare_summary_text(detail.summary.into());
                    window.set_compare_old_text(detail.old_column.into());
                    window.set_compare_new_text(detail.new_column.into());
                }
                Err(error) => {
                    window.set_status_text(format!("Compare failed: {error}").into());
                }
            }
        });
    }

    window.invoke_refresh_versions();
    window.run()
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
