//! Copying selected text. Once the panel captures the mouse, herdr's own selection no longer
//! works inside it, so the panel copies through the platform's clipboard tool.

use std::io::{self, Write};
use std::process::{Command, Stdio};

/// Tried in order; the first one that runs wins. macOS ships `pbcopy`; Linux needs one of
/// the others installed (Wayland `wl-copy`, X11 `xclip` / `xsel`).
const TOOLS: &[(&str, &[&str])] =
    &[("pbcopy", &[]), ("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["--clipboard", "--input"])];

pub fn copy(text: &str) -> io::Result<()> {
    for (tool, args) in TOOLS {
        let Ok(mut child) = Command::new(tool).args(*args).stdin(Stdio::piped()).stderr(Stdio::null()).spawn() else {
            continue;
        };
        child.stdin.take().expect("stdin is piped").write_all(text.as_bytes())?;
        if child.wait()?.success() {
            return Ok(());
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "no clipboard tool (pbcopy, wl-copy, xclip, xsel)"))
}
