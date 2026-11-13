//! The dictionary as the running app holds it: shared between the
//! controller thread (applies it) and the UI thread (edits it), and saved to
//! `dictionary.toml` on every edit so a change takes effect immediately.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use super::{Dictionary, Entry};

#[derive(Debug, thiserror::Error)]
pub enum DictionaryError {
    #[error("cannot write dictionary file: {0}")]
    Io(#[from] std::io::Error),
}

pub struct DictionaryStore {
    /// `None` keeps the dictionary in memory only (tests).
    path: Option<PathBuf>,
    dictionary: RwLock<Dictionary>,
    /// The most recent text handed to the injector, kept in memory only so
    /// the dictionary window can offer it for correction.
    last_text: Mutex<Option<String>>,
}

impl DictionaryStore {
    pub fn in_memory(dictionary: Dictionary) -> Self {
        Self {
            path: None,
            dictionary: RwLock::new(dictionary),
            last_text: Mutex::new(None),
        }
    }

    /// Loads `path`; a missing file is an empty dictionary. A file that
    /// does not parse is moved aside to `dictionary.toml.bad` instead of
    /// being overwritten by the next save.
    pub fn load(path: PathBuf) -> Self {
        let dictionary = match std::fs::read_to_string(&path) {
            Ok(text) => match Dictionary::from_toml(&text) {
                Ok(dictionary) => {
                    log::info!(
                        "loaded {} dictionary entries from {}",
                        dictionary.entries.len(),
                        path.display()
                    );
                    dictionary
                }
                Err(e) => {
                    let bad = path.with_extension("toml.bad");
                    log::error!(
                        "{} is not a valid dictionary ({e}); moved to {}",
                        path.display(),
                        bad.display()
                    );
                    let _ = std::fs::rename(&path, &bad);
                    Dictionary::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Dictionary::default(),
            Err(e) => {
                log::error!("cannot read {}: {e}", path.display());
                Dictionary::default()
            }
        };
        Self {
            path: Some(path),
            dictionary: RwLock::new(dictionary),
            last_text: Mutex::new(None),
        }
    }

    /// `dictionary.toml` beside the config in use: the working directory
    /// when it holds a `config.toml` (or already a `dictionary.toml`), else
    /// the platform config directory.
    pub fn default_path() -> Option<PathBuf> {
        if Path::new("dictionary.toml").exists() || Path::new("config.toml").exists() {
            return Some(PathBuf::from("dictionary.toml"));
        }
        directories::ProjectDirs::from("", "", "voice_input")
            .map(|d| d.config_dir().join("dictionary.toml"))
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn apply(&self, text: &str) -> String {
        self.dictionary.read().unwrap().apply(text)
    }

    pub fn hints(&self) -> Vec<String> {
        self.dictionary.read().unwrap().hints()
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.dictionary.read().unwrap().entries.clone()
    }

    /// Replaces the entries and saves them. The new entries are in effect
    /// even when saving fails.
    pub fn set_entries(&self, entries: Vec<Entry>) -> Result<(), DictionaryError> {
        let dictionary = Dictionary::new(entries);
        let text = dictionary.to_toml();
        *self.dictionary.write().unwrap() = dictionary;
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        // Write beside the target and rename, so a crash never leaves a
        // half-written dictionary.
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn record_text(&self, text: &str) {
        *self.last_text.lock().unwrap() = Some(text.to_owned());
    }

    pub fn last_text(&self) -> Option<String> {
        self.last_text.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("voice_input_dict_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_missing_file_is_an_empty_dictionary() {
        let store = DictionaryStore::load(temp_dir("missing").join("dictionary.toml"));
        assert!(store.entries().is_empty());
        assert_eq!(store.apply("そのまま"), "そのまま");
    }

    #[test]
    fn edits_apply_at_once_and_survive_a_reload() {
        let dir = temp_dir("reload");
        let path = dir.join("dictionary.toml");
        let store = DictionaryStore::load(path.clone());
        store
            .set_entries(vec![Entry::new("クラウドコード", "Claude Code")])
            .unwrap();
        assert_eq!(store.apply("クラウドコード"), "Claude Code");

        let reloaded = DictionaryStore::load(path);
        assert_eq!(
            reloaded.entries(),
            vec![Entry::new("クラウドコード", "Claude Code")]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unparsable_file_is_moved_aside_not_overwritten() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.toml");
        std::fs::write(&path, "this is = = not toml").unwrap();

        let store = DictionaryStore::load(path.clone());
        assert!(store.entries().is_empty());
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("dictionary.toml.bad")).unwrap(),
            "this is = = not toml"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_last_text_is_remembered() {
        let store = DictionaryStore::in_memory(Dictionary::default());
        assert_eq!(store.last_text(), None);
        store.record_text("こんにちは");
        assert_eq!(store.last_text().as_deref(), Some("こんにちは"));
    }
}
