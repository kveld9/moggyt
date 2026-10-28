//! moggyt: native YouTube Music desktop client (Slint).
//!
//! Composition root: launches the native Slint application shell wired to live
//! adapters. Domain logic lives in [`moggyt::domain`]; all I/O hides behind
//! ports implemented in [`moggyt::infra`].

fn main() -> anyhow::Result<()> {
    moggyt::slint_shell::app::run()
}
