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

use std::path::{Path, PathBuf};

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
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    pick_data_dir(&data_dir_candidates(
        exe_dir.as_deref(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &cwd,
    ))
}

/// The data-dir candidates, in resolution order. A candidate
/// counts only when it actually holds the shipped catalogue
/// file — a bare `data/` directory is not evidence.
///
/// 1. Installed layout: `<prefix>/bin/torori` with data at
///    `<prefix>/data`.
/// 2. Build-tree layout: the binary was built from a source
///    checkout and copied out of it (the estate install pattern:
///    the deployed binary lives in `~/.local/bin` with no data
///    tree beside it, while the checkout's `data/` is the data
///    of record). The build tree is pinned at compile time via
///    `CARGO_MANIFEST_DIR`; before this candidate existed, the
///    deployed server fell through to the cwd-relative layout,
///    found no catalogue file under the client's cwd, and
///    silently served an empty catalogue (TR-1).
/// 3. Source layout: `./data` under the cwd — correct only when
///    the client happens to launch the server from a checkout,
///    so it is the last resort, never the deciding one.
fn data_dir_candidates(exe_dir: Option<&Path>, build_tree: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(3);
    if let Some(prefix) = exe_dir.and_then(|bin| bin.parent()) {
        candidates.push(prefix.join("data"));
    }
    candidates.push(build_tree.join("data"));
    candidates.push(cwd.join("data"));
    candidates
}

/// Pick the first candidate that holds the catalogue file. When
/// none does, return the last candidate (the legacy cwd-relative
/// layout) so first-run and fixture behaviour — an explicitly
/// empty data tree serving an empty catalogue — is unchanged;
/// the missing file is then named by the catalogue loader's
/// stderr warning instead of being masked by a nearer miss.
fn pick_data_dir(candidates: &[PathBuf]) -> PathBuf {
    for candidate in candidates {
        if candidate.join("catalogue").join("entries.ron").is_file() {
            return candidate.clone();
        }
    }
    candidates
        .last()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("torori-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn with_catalogue(dir: &Path) -> PathBuf {
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("catalogue")).unwrap();
        std::fs::write(data.join("catalogue").join("entries.ron"), "(entries: [])").unwrap();
        data
    }

    #[test]
    fn pick_finds_build_tree_data_when_exe_layout_has_none() {
        // The deployed shape (TR-1): exe in <prefix>/bin with no
        // <prefix>/data, cwd elsewhere; the build tree's data
        // must win over the cwd fallback.
        let base = scratch("buildtree");
        let exe_dir = base.join("prefix").join("bin");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let build_tree = base.join("checkout");
        let build_data = with_catalogue(&build_tree);
        let cwd = base.join("client-cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        let candidates = data_dir_candidates(Some(&exe_dir), &build_tree, &cwd);
        assert_eq!(pick_data_dir(&candidates), build_data);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pick_prefers_exe_relative_data_over_build_tree() {
        let base = scratch("installed");
        let prefix = base.join("prefix");
        let exe_dir = prefix.join("bin");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let installed_data = with_catalogue(&prefix);
        let build_tree = base.join("checkout");
        let _ = with_catalogue(&build_tree);
        let cwd = base.join("client-cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        let candidates = data_dir_candidates(Some(&exe_dir), &build_tree, &cwd);
        assert_eq!(pick_data_dir(&candidates), installed_data);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pick_falls_back_to_cwd_data_when_no_candidate_has_a_catalogue() {
        let base = scratch("fallback");
        let exe_dir = base.join("prefix").join("bin");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let build_tree = base.join("checkout");
        std::fs::create_dir_all(&build_tree).unwrap();
        let cwd = base.join("client-cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        let candidates = data_dir_candidates(Some(&exe_dir), &build_tree, &cwd);
        assert_eq!(pick_data_dir(&candidates), cwd.join("data"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn default_data_dir_resolves_the_shipped_catalogue() {
        // In the test layout the exe-relative candidate is the
        // cargo target tree (no data), so this passes only if a
        // later candidate that actually holds the catalogue is
        // reached — the TR-1 regression in miniature.
        let file = Paths::resolve(None, None).catalogue_file();
        assert!(
            file.is_file(),
            "default resolution must find the shipped catalogue, got {}",
            file.display()
        );
    }
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
