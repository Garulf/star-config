//! The clipboard, the trash and the file manager, behind seams tests can replace.

use super::decoded;
use crate::screens::words::words;
use sc_core::screenshots;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

pub const NO_CLIPBOARD: &str = "No clipboard is available in this session.";

/// Where copies go. The real one must stay alive while the app runs: on Linux the
/// owner of the clipboard answers every paste.
pub trait Clipboard: Send {
    fn set_image(&mut self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String>;
    fn set_text(&mut self, text: String) -> Result<(), String>;
}

struct System(arboard::Clipboard);

impl Clipboard for System {
    fn set_image(&mut self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        self.0
            .set_image(arboard::ImageData {
                width,
                height,
                bytes: rgba.into(),
            })
            .map_err(|err| err.to_string())
    }

    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.0.set_text(text).map_err(|err| err.to_string())
    }
}

type Opener = Box<dyn FnOnce() -> Option<Box<dyn Clipboard>> + Send>;

enum Slot {
    Unopened(Opener),
    Open(Box<dyn Clipboard>),
    Missing,
}

/// One clipboard for the life of the app, opened on first use.
#[derive(Clone)]
pub struct ClipboardCell(Arc<Mutex<Slot>>);

impl ClipboardCell {
    pub fn system() -> Self {
        Self::opened_by(|| {
            arboard::Clipboard::new()
                .ok()
                .map(|clipboard| Box::new(System(clipboard)) as Box<dyn Clipboard>)
        })
    }

    pub fn with(clipboard: impl Clipboard + 'static) -> Self {
        Self(Arc::new(Mutex::new(Slot::Open(Box::new(clipboard)))))
    }

    pub fn missing() -> Self {
        Self(Arc::new(Mutex::new(Slot::Missing)))
    }

    fn opened_by(open: impl FnOnce() -> Option<Box<dyn Clipboard>> + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Slot::Unopened(Box::new(open)))))
    }

    /// Runs `put` on the clipboard. Blocking: call it from background work.
    pub fn put(
        &self,
        put: impl FnOnce(&mut dyn Clipboard) -> Result<(), String>,
    ) -> Result<(), CopyError> {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(*slot, Slot::Unopened(_))
            && let Slot::Unopened(open) = std::mem::replace(&mut *slot, Slot::Missing)
            && let Some(clipboard) = open()
        {
            *slot = Slot::Open(clipboard);
        }
        match &mut *slot {
            Slot::Open(clipboard) => put(clipboard.as_mut()).map_err(CopyError::Failed),
            Slot::Unopened(_) | Slot::Missing => Err(CopyError::NoClipboard),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyError {
    NoClipboard,
    Failed(String),
}

pub type FileAction = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// Everything the screen reaches outside its own files.
#[derive(Clone)]
pub struct Services {
    pub clipboard: ClipboardCell,
    pub trash: FileAction,
    pub reveal: FileAction,
    /// The archive folder used when Settings names none.
    pub default_archive: Option<PathBuf>,
}

impl Services {
    pub fn system() -> Self {
        Self {
            clipboard: ClipboardCell::system(),
            trash: Arc::new(|path: &Path| screenshots::trash(path).map_err(|err| err.to_string())),
            reveal: Arc::new(|folder: &Path| {
                open::that_detached(folder).map_err(|err| err.to_string())
            }),
            default_archive: pictures_archive(),
        }
    }
}

fn pictures_archive() -> Option<PathBuf> {
    let user = directories::UserDirs::new()?;
    Some(user.picture_dir()?.join("Star Citizen"))
}

/// What a copy put on the clipboard, for the toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copied {
    Image,
    Smaller {
        width: u32,
        height: u32,
        bytes: usize,
    },
    Path,
}

pub fn copy_image(clipboard: &ClipboardCell, path: &Path) -> Result<Copied, CopyError> {
    let rgba =
        decoded(|| screenshots::load_rgba(path)).map_err(|err| CopyError::Failed(words(&err)))?;
    clipboard
        .put(|board| board.set_image(rgba.width as usize, rgba.height as usize, rgba.pixels))?;
    Ok(Copied::Image)
}

pub fn copy_smaller(
    clipboard: &ClipboardCell,
    path: &Path,
    max_bytes: u64,
    max_edge: u32,
) -> Result<Copied, CopyError> {
    let failed = |err: String| CopyError::Failed(err);
    let copy = decoded(|| screenshots::share_copy(path, max_bytes, max_edge))
        .map_err(|e| failed(words(&e)))?;
    let rgba = image::load_from_memory_with_format(&copy.jpeg, image::ImageFormat::Jpeg)
        .map_err(|e| failed(e.to_string()))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    clipboard.put(|board| board.set_image(width as usize, height as usize, rgba.into_raw()))?;
    Ok(Copied::Smaller {
        width: copy.width,
        height: copy.height,
        bytes: copy.jpeg.len(),
    })
}

pub fn copy_path(clipboard: &ClipboardCell, path: &Path) -> Result<Copied, CopyError> {
    clipboard.put(|board| board.set_text(path.display().to_string()))?;
    Ok(Copied::Path)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counting(Arc<Mutex<Vec<String>>>);

    impl Clipboard for Counting {
        fn set_image(&mut self, width: usize, height: usize, _: Vec<u8>) -> Result<(), String> {
            self.0.lock().unwrap().push(format!("{width}x{height}"));
            Ok(())
        }

        fn set_text(&mut self, text: String) -> Result<(), String> {
            self.0.lock().unwrap().push(text);
            Ok(())
        }
    }

    #[test]
    fn the_clipboard_opens_once_and_is_kept() {
        let opened = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let cell = {
            let opened = opened.clone();
            let log = log.clone();
            ClipboardCell::opened_by(move || {
                *opened.lock().unwrap() += 1;
                Some(Box::new(Counting(log)) as Box<dyn Clipboard>)
            })
        };
        for text in ["one", "two"] {
            cell.put(|board| board.set_text(text.into())).unwrap();
        }
        assert_eq!(*opened.lock().unwrap(), 1);
        assert_eq!(*log.lock().unwrap(), ["one", "two"]);
    }

    #[test]
    fn a_clipboard_that_cannot_open_reports_missing() {
        let cell = ClipboardCell::opened_by(|| None);
        let result = cell.put(|board| board.set_text("x".into()));
        assert_eq!(result, Err(CopyError::NoClipboard));
        assert_eq!(
            ClipboardCell::missing().put(|board| board.set_text("x".into())),
            Err(CopyError::NoClipboard)
        );
    }
}
