//! StageCraft's GUI.
//!
//! [`Studio`] holds the engine and the UI state and draws the window into any
//! `egui::Ui`, so `egui_kittest` drives every screen headlessly. [`run`] opens the
//! native window with a control server in GUI mode.
//!
//! The GUI never changes the document itself: every edit is a core command through
//! [`Studio::run`]. UI state has its own small registry of `ui.*` commands
//! ([`commands`]), which the control server answers next to the core one.

pub mod app;
pub mod canvas;
pub mod commands;
pub mod demo;
pub mod dialogs;
pub mod fuzzy;
pub mod i18n;
pub mod inspector;
pub mod library;
pub mod palette;
pub mod panels;
pub mod schema_form;
pub mod sheets;
pub mod sketch_ui;
pub mod state;
pub mod table;
pub mod theme;
pub mod tools;
pub mod view3d;
pub mod widgets;

pub use app::{StageApp, Studio, run};
pub use state::{ThemePref, Tool, UiState, View};
