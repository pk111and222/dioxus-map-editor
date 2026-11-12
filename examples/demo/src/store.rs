//! Local demonstration adapters; storage is outside the published canvas crate.
use dioxus_map_editor::{document::*, error::*, storage::*};

/// Example resource storage; synchronous methods must be offloaded for file IO.
/// Desktop coordinates writes in-process, Web uses an explicitly single-tab demo.
#[derive(Clone, Default)]
pub struct DemoStore;

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use super::*;
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, Ordering},
            Mutex,
        },
    };
    static WRITE_LOCK: Mutex<()> = Mutex::new(());
    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn path(id: &MapId) -> MapResult<PathBuf> {
        if !["session-a", "session-b", "editor-map", "atlas-studio"].contains(&id.0.as_str()) {
            return Err(MapError::Invalid("unknown demo resource".into()));
        }
        let root = if let Some(path) = std::env::var_os("DME_DEMO_STORE_DIR") {
            PathBuf::from(path)
        } else if let Some(home) =
            std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
        {
            PathBuf::from(home).join(".dioxus-map-editor-demo")
        } else {
            return Err(MapError::Storage(
                "set DME_DEMO_STORE_DIR to a writable directory".into(),
            ));
        };
        Ok(root.join(format!("{}.json", id.0)))
    }
    fn io(error: std::io::Error) -> MapError {
        MapError::Storage(error.to_string())
    }
    fn read(path: &Path, limits: &Limits) -> MapResult<Option<StoredMap>> {
        use std::io::Read;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
        };
        // Bound the read before allocating/decoding an untrusted file.
        let mut raw = String::new();
        file.take(limits.max_bytes as u64 + 1)
            .read_to_string(&mut raw)
            .map_err(io)?;
        Ok(Some(StoredMap::decode(&raw, limits)?))
    }
    fn replace(path: &Path, raw: &str) -> MapResult<()> {
        let parent = path
            .parent()
            .ok_or_else(|| MapError::Storage("missing storage directory".into()))?;
        fs::create_dir_all(parent).map_err(io)?;
        let unique = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = path.with_extension(format!("{}-{unique}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(io)?;
        let outcome = (|| {
            file.write_all(raw.as_bytes()).map_err(io)?;
            file.sync_all().map_err(io)?;
            drop(file);
            fs::rename(&temporary, path).map_err(io)?;
            #[cfg(unix)]
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(io)?;
            Ok(())
        })();
        if outcome.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        outcome
    }
    impl MapStore for DemoStore {
        fn load(&mut self, id: &MapId, limits: &Limits) -> MapResult<Option<StoredMap>> {
            let _guard = WRITE_LOCK
                .lock()
                .map_err(|_| MapError::Storage("demo lock poisoned".into()))?;
            read(&path(id)?, limits)
        }
        fn save(&mut self, request: &SaveRequest, limits: &Limits) -> MapResult<()> {
            validate_save_request(request, limits)?;
            let encoded = request.snapshot.encode(limits)?;
            let _guard = WRITE_LOCK
                .lock()
                .map_err(|_| MapError::Storage("demo lock poisoned".into()))?;
            let path = path(&request.stamp.map_id)?;
            if read(&path, limits)? != request.baseline {
                return Err(MapError::Conflict(
                    "file changed since load; draft retained".into(),
                ));
            }
            replace(&path, &encoded)
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    fn storage() -> MapResult<web_sys::Storage> {
        web_sys::window()
            .ok_or_else(|| MapError::Storage("no browser window".into()))?
            .local_storage()
            .map_err(|_| MapError::Storage("localStorage unavailable".into()))?
            .ok_or_else(|| MapError::Storage("localStorage disabled".into()))
    }
    fn key(id: &MapId) -> MapResult<String> {
        if !["session-a", "session-b", "editor-map", "atlas-studio"].contains(&id.0.as_str()) {
            return Err(MapError::Invalid("unknown demo resource".into()));
        }
        Ok(format!("dme:session-lab:v1:{}", id.0))
    }
    impl MapStore for DemoStore {
        fn load(&mut self, id: &MapId, limits: &Limits) -> MapResult<Option<StoredMap>> {
            let raw = storage()?
                .get_item(&key(id)?)
                .map_err(|_| MapError::Storage("localStorage read failed".into()))?;
            raw.map(|raw| StoredMap::decode(&raw, limits)).transpose()
        }
        fn save(&mut self, request: &SaveRequest, limits: &Limits) -> MapResult<()> {
            validate_save_request(request, limits)?;
            let raw = request.snapshot.encode(limits)?;
            if self.load(&request.stamp.map_id, limits)? != request.baseline {
                return Err(MapError::Conflict(
                    "browser storage changed; draft retained".into(),
                ));
            }
            // A single setItem is atomic and all calls in this tab are synchronous.
            // No cross-tab CAS claim: use a transactional IndexedDB/Web Locks
            // adapter before supporting concurrent browser editing.
            storage()?
                .set_item(&key(&request.stamp.map_id)?, &raw)
                .map_err(|_| {
                    MapError::Storage("localStorage write failed (quota/permission)".into())
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus_map_editor::{command::Command, history::HistoryBudget, session::MapSession};
    #[test]
    fn desktop_adapter_reopens_and_rejects_external_replacement() {
        // Unique process-local directory, never touching a user's demo files.
        let dir = std::env::temp_dir().join(format!("dme-store-test-{}", std::process::id()));
        // Tests use one serialized case; no mutation of HOME or other system vars.
        std::env::set_var("DME_DEMO_STORE_DIR", &dir);
        let result = (|| -> MapResult<()> {
            let mut store = DemoStore;
            let limits = Limits::default();
            let mut session = MapSession::open(
                MapSnapshot::standalone(MapDocument::new("session-a")),
                None,
                limits,
                HistoryBudget::default(),
            )?;
            session.execute(vec![Command::SetStyle(ResourceRef {
                key: "test:saved".into(),
                version: 1,
            })])?;
            session.flush(&mut store)?;
            let loaded = store
                .load(&"session-a".into(), &limits)?
                .ok_or_else(|| MapError::Missing("saved resource".into()))?;
            assert_eq!(loaded.document, session.snapshot().document);
            session.execute(vec![Command::SetStyle(ResourceRef {
                key: "test:next".into(),
                version: 1,
            })])?;
            std::fs::write(dir.join("session-a.json"), "malformed external content")
                .map_err(|e| MapError::Storage(e.to_string()))?;
            assert!(session.flush(&mut store).is_err());
            assert!(session.is_dirty());
            assert_eq!(
                std::fs::read_to_string(dir.join("session-a.json"))
                    .map_err(|e| MapError::Storage(e.to_string()))?,
                "malformed external content"
            );
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("DME_DEMO_STORE_DIR");
        assert_eq!(result, Ok(()));
    }
}
