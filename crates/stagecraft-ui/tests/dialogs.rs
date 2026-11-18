//! File dialogs never block the frame: a request is queued, shown after the frame and
//! answered on a later one (craft-dialogs, answered here by an override).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use craft_dialogs::{Answer, MessageResult, Request};
use serde_json::json;
use stagecraft_core::Engine;
use stagecraft_ui::Studio;
use stagecraft_ui::state::FileRequest;

/// One frame of the app's dialog handling, without a window.
fn frame(s: &mut Studio) {
    s.file_dialogs();
    s.dialogs.show_pending::<eframe::Frame>(None);
}

fn answer_with(s: &mut Studio, answers: Vec<Answer>) -> Rc<RefCell<Vec<Request>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let mut answers = answers.into_iter();
    s.dialogs.set_override(Some(Box::new(move |r: &Request| {
        log.borrow_mut().push(r.clone());
        answers.next().unwrap_or(Answer::Cancelled)
    })));
    seen
}

fn saved_doc(dir: &std::path::Path) -> PathBuf {
    let mut e = Engine::new();
    e.execute("layer.create", json!({ "name": "From disk" })).unwrap();
    let path = dir.join("a.stagecraft");
    e.execute("doc.save", json!({ "path": path })).unwrap();
    path
}

#[test]
fn open_is_answered_on_a_later_frame() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_doc(dir.path());
    let mut s = Studio::new(Engine::new());
    let seen = answer_with(&mut s, vec![Answer::Paths(vec![path.clone()])]);
    s.ui.file_request = Some(FileRequest::Open);
    s.file_dialogs();
    assert!(seen.borrow().is_empty(), "nothing shows inside the frame");
    assert_eq!(s.dialogs.queued(), 1);
    s.dialogs.show_pending::<eframe::Frame>(None);
    frame(&mut s);
    assert_eq!(s.engine.path(), Some(&path));
    assert!(s.engine.doc().layers.values().any(|l| l.name == "From disk"));
    let Request::File(d) = &seen.borrow()[0] else { panic!("a file dialog") };
    assert_eq!(d.filters[0].extensions, ["stagecraft"]);
}

#[test]
fn unsaved_changes_ask_first_and_cancel_keeps_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_doc(dir.path());
    let mut s = Studio::new(Engine::new());
    s.ui.native_dialogs = true;
    s.run("layer.create", json!({ "name": "Unsaved" })).unwrap();
    let seen = answer_with(&mut s, vec![Answer::Message(MessageResult::Cancel)]);
    s.ui.file_request = Some(FileRequest::Open);
    for _ in 0..3 {
        frame(&mut s);
    }
    assert_eq!(seen.borrow().len(), 1);
    assert!(matches!(seen.borrow()[0], Request::Message(_)));
    assert!(s.engine.doc().layers.values().any(|l| l.name == "Unsaved"));

    // OK goes on to the file dialog, then opens.
    let seen = answer_with(&mut s, vec![Answer::Message(MessageResult::Ok), Answer::Paths(vec![path.clone()])]);
    s.ui.file_request = Some(FileRequest::Open);
    for _ in 0..4 {
        frame(&mut s);
    }
    assert_eq!(seen.borrow().len(), 2);
    assert_eq!(s.engine.path(), Some(&path));
}

#[test]
fn new_with_changes_asks_and_ok_discards() {
    let mut s = Studio::new(Engine::new());
    s.ui.native_dialogs = true;
    s.run("layer.create", json!({ "name": "Unsaved" })).unwrap();
    answer_with(&mut s, vec![Answer::Message(MessageResult::Ok)]);
    stagecraft_ui::panels::new_document(&mut s);
    assert!(s.engine.doc().layers.values().any(|l| l.name == "Unsaved"), "not before the answer");
    for _ in 0..3 {
        frame(&mut s);
    }
    assert!(!s.engine.doc().layers.values().any(|l| l.name == "Unsaved"));
}

#[test]
fn every_request_has_a_dialog_and_cancel_does_nothing() {
    for req in [
        FileRequest::SaveAs,
        FileRequest::ExportPdf,
        FileRequest::Logo,
        FileRequest::ImportGdtf,
        FileRequest::ImportMvr,
        FileRequest::ExportMvr,
        FileRequest::ImportModel,
        FileRequest::ImportUnderlay,
    ] {
        let mut s = Studio::new(Engine::new());
        let seen = answer_with(&mut s, vec![]);
        s.ui.file_request = Some(req);
        frame(&mut s);
        frame(&mut s);
        assert_eq!(seen.borrow().len(), 1, "{req:?}");
        assert!(s.ui.status.is_none(), "{req:?}: {:?}", s.ui.status);
    }
}

#[test]
fn unavailable_is_an_error() {
    let mut s = Studio::new(Engine::new());
    answer_with(&mut s, vec![Answer::Unavailable("no window".into())]);
    s.ui.file_request = Some(FileRequest::ImportMvr);
    frame(&mut s);
    frame(&mut s);
    let (text, error) = s.ui.status.clone().unwrap();
    assert!(error && text.contains("no window"), "{text}");
}
