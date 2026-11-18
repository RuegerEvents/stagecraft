//! File and message dialogs through `craft-dialogs`: a [`FileRequest`] is queued, the native
//! dialog shows after the frame and its answer arrives on a later frame, where the matching
//! command runs. Nothing blocks inside `App::ui`: a modal AppKit loop there let winit re-enter
//! its event handler (a file dragged over the window) and abort the app.

use std::path::PathBuf;
use std::sync::Arc;

use craft_dialogs::{Answer, Dialogs, FileDialog, Message, MessageButtons, MessageLevel, MessageResult};
use serde_json::{Value, json};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::FileRequest;

/// What to do with a dialog's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    /// The "discard changes?" box; on OK the request goes on.
    Discard(FileRequest),
    /// A file dialog; the paths go to the request's command.
    File(FileRequest),
}

/// The dialog queue. The app gives it a wake-up that repaints; tests answer through
/// [`Dialogs::set_override`].
pub fn queue() -> Dialogs<Then> {
    Dialogs::new(Arc::new(|| {}))
}

impl Studio {
    /// Queue the pending [`FileRequest`] and act on answers that arrived. The app calls this
    /// every frame, then `dialogs.show_pending(window)`.
    pub fn file_dialogs(&mut self) {
        for (then, answer) in self.dialogs.take_answers() {
            match (then, answer) {
                (Then::Discard(req), Answer::Message(MessageResult::Ok | MessageResult::Yes)) => self.proceed(req),
                (Then::File(req), Answer::Paths(paths)) if !paths.is_empty() => self.answered(req, paths),
                (_, Answer::Unavailable(why)) => self.ui.error(tf("status.dialog_unavailable", &[("why", &why)])),
                _ => {}
            }
        }
        let Some(req) = self.ui.file_request.take() else { return };
        let discards = matches!(req, FileRequest::Open | FileRequest::New | FileRequest::Quit);
        if discards && self.engine.dirty() && self.ui.native_dialogs {
            let (title, text) = match req {
                FileRequest::Quit => ("dialog.quit.title", "dialog.quit.text"),
                _ => ("dialog.discard.title", "dialog.discard.text"),
            };
            let message = Message {
                level: MessageLevel::Warning,
                title: t(title).to_owned(),
                description: t(text).to_owned(),
                buttons: MessageButtons::OkCancel,
            };
            self.dialogs.ask_message(message, Then::Discard(req));
        } else {
            self.proceed(req);
        }
    }

    /// Show the request's file dialog (or, for New, just do it).
    fn proceed(&mut self, req: FileRequest) {
        let stem = |s: &Studio, default: &str| {
            s.engine
                .path()
                .and_then(|p| p.file_stem())
                .map_or_else(|| default.to_owned(), |n| n.to_string_lossy().into_owned())
        };
        let dialog = match req {
            FileRequest::New => {
                let _ = self.run("doc.new", json!({ "discard": true }));
                return;
            }
            FileRequest::Quit => {
                // The app sees the flag and closes the window on its next frame.
                self.ui.quit_confirmed = true;
                return;
            }
            FileRequest::Open => FileDialog::open().filter("StageCraft", &["stagecraft"]),
            FileRequest::SaveAs => FileDialog::save()
                .filter("StageCraft", &["stagecraft"])
                .file_name(format!("{}.stagecraft", stem(self, "Untitled"))),
            FileRequest::ExportPdf => {
                FileDialog::save().filter("PDF", &["pdf"]).file_name(format!("{}.pdf", stem(self, "Sheets")))
            }
            FileRequest::ExportMvr => {
                FileDialog::save().filter("MVR", &["mvr"]).file_name(format!("{}.mvr", stem(self, "Untitled")))
            }
            FileRequest::ImportGdtf => FileDialog::open_many().filter("GDTF", &["gdtf"]),
            FileRequest::ImportMvr => FileDialog::open().filter("MVR", &["mvr"]),
            FileRequest::ImportModel => FileDialog::open().filter("3D model", MODEL_EXTENSIONS),
            FileRequest::ImportUnderlay => FileDialog::open().filter("Drawing", UNDERLAY_EXTENSIONS),
            FileRequest::Logo => FileDialog::open().filter("Image", &["png", "jpg", "jpeg"]),
        };
        self.dialogs.ask(dialog, Then::File(req));
    }

    /// Run the request's command with the chosen paths.
    fn answered(&mut self, req: FileRequest, paths: Vec<PathBuf>) {
        let path = paths[0].clone();
        match req {
            FileRequest::New | FileRequest::Quit => {}
            FileRequest::Open => {
                let _ = self.run("doc.open", json!({ "path": path, "discard": true }));
            }
            FileRequest::SaveAs => {
                if self.run("doc.save", json!({ "path": path })).is_ok() {
                    self.ui.info(t("status.saved"));
                }
            }
            FileRequest::ExportPdf => {
                if self.run("sheet.export_pdf", json!({ "path": path })).is_ok() {
                    self.ui.info(tf("status.exported", &[("path", &path.display())]));
                }
            }
            FileRequest::ExportMvr => {
                if self.run("mvr.export", json!({ "path": path })).is_ok() {
                    self.ui.info(tf("status.exported", &[("path", &path.display())]));
                }
            }
            FileRequest::ImportGdtf => {
                let steps: Vec<Value> =
                    paths.iter().map(|p| json!({ "command": "gdtf.import", "args": { "path": p } })).collect();
                if self.run("batch", json!({ "commands": steps })).is_ok() {
                    self.ui.info(tf("library.imported", &[("n", &paths.len())]));
                }
            }
            FileRequest::ImportMvr => {
                if let Ok(r) = self.run("mvr.import", json!({ "path": path })) {
                    self.ui.fit_pending = true;
                    self.ui.view3d.fit_pending = true;
                    let n = |k: &str| r[k].as_u64().unwrap_or(0).to_string();
                    let warnings = r["warnings"].as_array().map_or(0, Vec::len).to_string();
                    self.ui.info(tf(
                        "status.mvr_imported",
                        &[
                            ("fixtures", &n("fixtures")),
                            ("trusses", &n("trusses")),
                            ("models", &n("models")),
                            ("warnings", &warnings),
                        ],
                    ));
                }
            }
            FileRequest::ImportModel => {
                let mut args = json!({ "path": path });
                args["layer"] = self.active_layer_arg();
                if self.run("model.import", args).is_ok() {
                    self.ui.view3d.fit_pending = true;
                }
            }
            FileRequest::ImportUnderlay => {
                if let Ok(r) = self.run("underlay.import", json!({ "path": path })) {
                    self.ui.fit_pending = true;
                    if let Ok(id) = serde_json::from_value::<stagecraft_core::Id>(r["id"].clone()) {
                        let _ = self.run("selection.set", json!({ "ids": [id] }));
                    }
                    self.ui.info(t("status.underlay_imported"));
                }
            }
            FileRequest::Logo => {
                let _ = self.run("doc.set_meta", json!({ "logo_file": path }));
            }
        }
    }
}

/// What File > Import model offers (`model.import` reads all of them).
pub const MODEL_EXTENSIONS: &[&str] = &stagecraft_core::model_file::EXTENSIONS;

/// What File > Import underlay offers.
pub const UNDERLAY_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "pdf", "svg", "dxf", "dwg"];
