//! What the app keeps between launches by name: the panels' layout, that
//! the tour was dismissed, and, in the browser, each run's bookmarks.
//!
//! The desktop app keeps each in a file of its settings directory. The
//! browser build has no files, and keeps each in the page's localStorage
//! instead, where a write can fail: storage turned off, full, or a private
//! window. A failure there costs only the setting, so it is ignored.

/// Where settings are kept.
pub trait Backend {
    /// The setting `name`; None when it was never kept.
    fn get(&self, name: &str) -> Result<Option<String>, String>;

    /// Keeps `value` as the setting `name`.
    fn set(&self, name: &str, value: &str) -> Result<(), String>;

    /// Forgets the setting `name`.
    fn remove(&self, name: &str) -> Result<(), String>;
}

/// Settings over a backend whose failures are ignored.
pub struct Settings<B: Backend>(pub B);

impl<B: Backend> Settings<B> {
    /// The setting `name`; None when it was never kept or does not read.
    pub fn read(&self, name: &str) -> Option<String> {
        self.0.get(name).ok().flatten()
    }

    /// Keeps `value` as the setting `name`, or does nothing.
    pub fn write(&self, name: &str, value: &str) {
        let _ = self.0.set(name, value);
    }

    /// Forgets the setting `name`, or does nothing.
    pub fn remove(&self, name: &str) {
        let _ = self.0.remove(name);
    }
}

/// The settings of this build: files in the settings directory on the
/// desktop, the page's localStorage in the browser.
pub fn kept() -> Settings<impl Backend> {
    #[cfg(not(target_family = "wasm"))]
    let backend = Files;
    #[cfg(target_family = "wasm")]
    let backend = LocalStorage;
    Settings(backend)
}

/// Each setting a file of its name in the settings directory.
#[cfg(not(target_family = "wasm"))]
struct Files;

#[cfg(not(target_family = "wasm"))]
impl Files {
    fn path(name: &str) -> Result<std::path::PathBuf, String> {
        let dir = crate::tour::config_dir().ok_or("no settings directory")?;
        Ok(dir.join(name))
    }
}

#[cfg(not(target_family = "wasm"))]
impl Backend for Files {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match std::fs::read_to_string(Files::path(name)?) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let path = Files::path(name)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, value).map_err(|e| e.to_string())
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        match std::fs::remove_file(Files::path(name)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

/// Each setting a localStorage item, under a prefix of the app's own.
#[cfg(target_family = "wasm")]
struct LocalStorage;

#[cfg(target_family = "wasm")]
impl LocalStorage {
    /// The prefix of the app's items, apart from the site's own.
    const PREFIX: &str = "rewind/";

    fn storage() -> Result<web_sys::Storage, String> {
        let window = web_sys::window().ok_or("no window")?;
        let storage = window.local_storage().map_err(|e| format!("{e:?}"))?;
        storage.ok_or_else(|| "no localStorage".to_string())
    }

    fn key(name: &str) -> String {
        format!("{}{name}", LocalStorage::PREFIX)
    }
}

#[cfg(target_family = "wasm")]
impl Backend for LocalStorage {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        LocalStorage::storage()?
            .get_item(&LocalStorage::key(name))
            .map_err(|e| format!("{e:?}"))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        LocalStorage::storage()?
            .set_item(&LocalStorage::key(name), value)
            .map_err(|e| format!("{e:?}"))
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        LocalStorage::storage()?
            .remove_item(&LocalStorage::key(name))
            .map_err(|e| format!("{e:?}"))
    }
}

#[cfg(test)]
mod tests {
    // Settings over two hand-made backends: one in memory, which keeps
    // what is written, and one that refuses everything, as localStorage
    // does when storage is turned off.
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct Memory(RefCell<HashMap<String, String>>);

    impl Backend for Memory {
        fn get(&self, name: &str) -> Result<Option<String>, String> {
            Ok(self.0.borrow().get(name).cloned())
        }

        fn set(&self, name: &str, value: &str) -> Result<(), String> {
            self.0.borrow_mut().insert(name.into(), value.into());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<(), String> {
            self.0.borrow_mut().remove(name);
            Ok(())
        }
    }

    /// Storage turned off: every call fails.
    struct Refusing;

    impl Backend for Refusing {
        fn get(&self, _name: &str) -> Result<Option<String>, String> {
            Err("SecurityError: storage is disabled".into())
        }

        fn set(&self, _name: &str, _value: &str) -> Result<(), String> {
            Err("QuotaExceededError".into())
        }

        fn remove(&self, _name: &str) -> Result<(), String> {
            Err("SecurityError: storage is disabled".into())
        }
    }

    #[test]
    fn a_setting_written_reads_back_until_removed() {
        // The layout written, read back, then forgotten.
        let settings = Settings(Memory::default());
        assert_eq!(settings.read("layout.json"), None);
        settings.write("layout.json", "{\"log\":2}");
        assert_eq!(settings.read("layout.json").as_deref(), Some("{\"log\":2}"));
        settings.remove("layout.json");
        assert_eq!(settings.read("layout.json"), None);
    }

    #[test]
    fn storage_that_fails_reads_as_nothing_kept() {
        // Every call fails underneath; reads come back empty and writes
        // and removals do nothing, without a panic.
        let settings = Settings(Refusing);
        settings.write("layout.json", "{}");
        settings.remove("layout.json");
        assert_eq!(settings.read("layout.json"), None);
    }
}
