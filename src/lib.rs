//! paper-rs: a native infinite canvas for planning, working and sketching.

pub mod app;
pub mod demo;
pub mod editor;
pub mod geom;
pub mod history;
pub mod ink;
pub mod io;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod model;
pub mod render;
pub mod text;
pub mod theme;
pub mod ui;
