//! moggyt library: pure domain, ports, and UI shell.
//!
//! The binary (`src/main.rs`) is only the composition root: it builds
//! logging, instantiates adapters, injects them into the UI shell, and
//! bootstraps Slint.

pub mod domain;
pub mod i18n;
pub mod infra;
pub mod slint_shell;
