use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::test_support::ffmpeg_util;

#[derive(Debug, Deserialize)]
pub struct CorpusSourcesManifest {
    pub version: u32,
    #[serde(default)]
    pub source: Vec<CorpusSource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorpusSource {
    pub id: String,
    pub filename: String,
    #[allow(dead_code)]
    pub url: String,
    #[allow(dead_code)]
    pub commons_page: String,
    #[allow(dead_code)]
    pub license: String,
    #[allow(dead_code)]
    pub author: String,
    #[allow(dead_code)]
    pub title: String,
    pub sha256: String,
}

pub fn corpus_root() -> PathBuf {
    if let Ok(root) = std::env::var("CLIP_SYNC_WORKSPACE_ROOT") {
        return PathBuf::from(root).join("tests").join("corpus");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join("corpus")
}

pub fn sources_root() -> PathBuf {
    corpus_root().join("_sources")
}

pub fn load_sources() -> CorpusSourcesManifest {
    let path = corpus_root().join("sources.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    toml::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

pub fn source_cache_path(source: &CorpusSource) -> PathBuf {
    sources_root().join(&source.filename)
}

pub fn find_source<'a>(manifest: &'a CorpusSourcesManifest, id: &str) -> &'a CorpusSource {
    manifest
        .source
        .iter()
        .find(|source| source.id == id)
        .unwrap_or_else(|| panic!("unknown source id {id:?} (see tests/corpus/sources.toml)"))
}

pub fn source_ready(id: &str) -> bool {
    let manifest = load_sources();
    let source = match manifest.source.iter().find(|source| source.id == id) {
        Some(source) => source,
        None => return false,
    };
    source_cache_path(source).is_file()
}

pub fn all_required_sources_ready(source_ids: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    source_ids
        .into_iter()
        .all(|id| source_ready(id.as_ref()))
}

/// Decode any ffmpeg-supported input to mono PCM WAV, optionally trimmed.
pub fn prepare_source_master_wav(
    source_path: &Path,
    output_wav: &Path,
    sample_rate: u32,
    max_duration_secs: Option<u32>,
) -> bool {
    ffmpeg_util::decode_to_mono_wav(source_path, output_wav, sample_rate, max_duration_secs)
}
