//! Server state: filesystem roots and the loaded stores.
//!
//! Two roots, deliberately separate:
//! - `data_dir` — shipped, versioned data (catalogue data files,
//!   pair records, the VoiceIdentity registry). Read-only at
//!   runtime; the repo's `data/` tree in a source checkout.
//! - `state_dir` — writable runtime state (library catalog store,
//!   bundle quarantine + ingested store, project attribution
//!   manifests, generated assets). Defaults to
//!   `$XDG_DATA_HOME/torori` (primo: `~/.local/share/torori`,
//!   where the brief's §8 bundle staging lives).

use std::path::PathBuf;

use crate::error::{TororiError, TororiResult};

#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    /// Resolve roots from CLI overrides and the environment.
    pub fn resolve(data_flag: Option<PathBuf>, root_flag: Option<PathBuf>) -> Paths {
        let data_dir = data_flag
            .or_else(|| std::env::var_os("TORORI_DATA_DIR").map(PathBuf::from))
            .unwrap_or_else(default_data_dir);
        let state_dir = root_flag
            .or_else(|| std::env::var_os("TORORI_ROOT").map(PathBuf::from))
            .unwrap_or_else(default_state_dir);
        Paths {
            data_dir,
            state_dir,
        }
    }

    pub fn catalogue_file(&self) -> PathBuf {
        self.data_dir.join("catalogue").join("entries.ron")
    }

    pub fn pairs_dir(&self) -> PathBuf {
        self.data_dir.join("pairs")
    }

    pub fn voices_file(&self) -> PathBuf {
        self.data_dir.join("voices").join("identities.ron")
    }

    pub fn library_catalog(&self) -> PathBuf {
        self.state_dir.join("library").join("catalog.ron")
    }

    pub fn bundles_quarantine(&self) -> PathBuf {
        self.state_dir.join("bundles").join("quarantine")
    }

    pub fn bundles_store(&self) -> PathBuf {
        self.state_dir.join("bundles").join("ingested")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.state_dir.join("projects")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.state_dir.join("assets")
    }

    /// Create the writable state layout. Idempotent.
    pub fn ensure_state_dirs(&self) -> TororiResult<()> {
        for dir in [
            self.state_dir.join("library"),
            self.bundles_quarantine(),
            self.bundles_store(),
            self.projects_dir(),
            self.assets_dir(),
        ] {
            std::fs::create_dir_all(&dir).map_err(|e| {
                TororiError::new("io", format!("cannot create {}: {e}", dir.display()))
            })?;
        }
        Ok(())
    }
}

fn default_data_dir() -> PathBuf {
    // Installed layout: <prefix>/bin/torori with data at
    // <prefix>/data. Source layout: ./data under the cwd.
    if let Ok(exe) = std::env::current_exe()
        && let Some(prefix) = exe.parent().and_then(|bin| bin.parent())
    {
        let candidate = prefix.join("data");
        if candidate.is_dir() {
            return candidate;
        }
    }
    PathBuf::from("data")
}

fn default_state_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("torori");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("torori");
    }
    PathBuf::from(".torori-state")
}
