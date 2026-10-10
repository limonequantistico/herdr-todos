//! `TODOS.md` on disk: loading, safe writes, and noticing changes made by anyone else
//! (an editor, an agent, git).

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::doc::Doc;

pub const FILE_NAME: &str = "TODOS.md";

pub struct Store {
    path: PathBuf,
    /// The file's content as we last read or wrote it; `None` while it doesn't exist.
    known: Option<String>,
}

pub enum Freshness {
    Unchanged,
    /// Someone else changed the file; here is its new content.
    Changed(Doc),
}

impl Store {
    pub fn open(dir: &Path) -> io::Result<(Self, Doc)> {
        // Follow a symlinked TODOS.md to the real file, so saving updates that file instead of
        // replacing the link with a local copy.
        let path = dir.join(FILE_NAME);
        let path = fs::canonicalize(&path).unwrap_or(path);
        let mut store = Store { path, known: None };
        store.known = store.read()?;
        let doc = store.known.as_deref().map_or_else(Doc::empty, Doc::parse);
        Ok((store, doc))
    }

    fn read(&self) -> io::Result<Option<String>> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Compare the file with what we last saw. Called on watcher events, and before every
    /// write so we never overwrite a change we haven't shown yet.
    pub fn check(&mut self) -> io::Result<Freshness> {
        let now = self.read()?;
        if now == self.known {
            return Ok(Freshness::Unchanged);
        }
        let doc = now.as_deref().map_or_else(Doc::empty, Doc::parse);
        self.known = now;
        Ok(Freshness::Changed(doc))
    }

    /// Write atomically: a temp file next to the real one, then a rename, so a crash or a
    /// concurrent reader never sees half a file.
    pub fn write(&mut self, doc: &Doc) -> io::Result<()> {
        let text = doc.render();
        let name = self.path.file_name().and_then(|n| n.to_str()).unwrap_or(FILE_NAME);
        let tmp = self.path.with_file_name(format!(".{name}.tmp"));
        fs::write(&tmp, &text)?;
        // Keep the file's permissions; a fresh temp file would otherwise reset them.
        if let Ok(meta) = fs::metadata(&self.path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, &self.path)?;
        self.known = Some(text);
        Ok(())
    }

    /// Whether the file exists (as of the last read or write).
    pub fn exists(&self) -> bool {
        self.known.is_some()
    }

    /// The file actually read and written (a symlink's target).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A live watch, and its signal that the file may have changed.
pub type Watch = (RecommendedWatcher, Receiver<()>);

/// Watch the folder holding `file` (not the file itself: an atomic rename replaces the file,
/// which would end a watch on it) and signal whenever the file may have changed.
pub fn watch(file: &Path) -> notify::Result<Watch> {
    let name = file.file_name().map(|n| n.to_owned()).unwrap_or_else(|| FILE_NAME.into());
    let dir = file.parent().unwrap_or(Path::new("."));
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && event.paths.iter().any(|p| p.file_name().is_some_and(|n| n == name))
        {
            let _ = tx.send(());
        }
    })?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok((watcher, rx))
}
