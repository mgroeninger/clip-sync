use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSource {
    path: PathBuf,
}

impl MediaSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl From<PathBuf> for MediaSource {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl From<&Path> for MediaSource {
    fn from(path: &Path) -> Self {
        Self::new(path.to_path_buf())
    }
}
