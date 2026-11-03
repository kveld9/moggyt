//! Platform browser opener adapter.
//!
//! Hexagonal invariant: pure command argument derivation separated from process launching.

use anyhow::Context;

/// Derives the platform opener binary and argument vector for a URL.
pub fn opener_command(url: &str) -> (String, Vec<String>) {
    if cfg!(target_os = "macos") {
        ("open".to_string(), vec![url.to_string()])
    } else if cfg!(target_os = "windows") {
        ("rundll32".to_string(), vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()])
    } else {
        ("xdg-open".to_string(), vec![url.to_string()])
    }
}

/// Spawns the default platform browser targeting `url` without blocking.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    let (program, args) = opener_command(url);
    let mut child = std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .with_context(|| format!("failed to spawn browser opener: {program}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opener_command_for_current_target() {
        let test_url = "https://www.last.fm/api/auth/?api_key=key&token=tok";
        let (prog, args) = opener_command(test_url);

        if cfg!(target_os = "macos") {
            assert_eq!(prog, "open");
            assert_eq!(args, vec![test_url]);
        } else if cfg!(target_os = "windows") {
            assert_eq!(prog, "rundll32");
            assert_eq!(args, vec!["url.dll,FileProtocolHandler", test_url]);
        } else {
            assert_eq!(prog, "xdg-open");
            assert_eq!(args, vec![test_url]);
        }
    }
}
