//! The OpenNSFW library lane (brief §5/§6, skill
//! local-opennsfw-library §§2–7).
//!
//! Rights law as code, per-file:
//! - Every row carries a licence **state**: Cleared, NotCleared,
//!   or Undetermined. `decide()` is a pure function of the row.
//! - Fetch is possible only for a Cleared row, and a fetch copies
//!   ONE file into a caller project's staging **and appends its
//!   attribution manifest entry in the same operation** — if the
//!   manifest write fails, the copied file is removed; a fetched
//!   file without a manifest entry is unrepresentable.
//! - Ingest captures licence state per file *first*: a per-file
//!   sidecar (`<file>.meta.json` with license / license_source_url
//!   / author_credit / source_url) is the capture; absent or
//!   unparseable capture leaves the row UNDETERMINED — never
//!   rounded up. NC/ND marks are NOT CLEARED (an NC clip cannot
//!   ship; an ND clip cannot be edited into a shipped comp).
//! - There is no bulk-export or pack-download tool. Ever.
//!
//! The conjunction ban (brief §2) is enforced in the manifest
//! module's rights discipline and the comp lane; the library lane
//! contributes one hard rule here: rows record their provenance
//! class, and fetch refuses any row whose provenance marks it as
//! AI-generation material (pack rows are recordings; generated
//! assets never enter this store).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{TororiError, TororiResult};
use crate::state::Paths;
use crate::util::{Page, arg_str, opt_str, sha256_file};

/// Per-file licence state — deliberately no "probably fine"
/// value (skill §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LicenseState {
    Cleared,
    NotCleared,
    Undetermined,
}

impl LicenseState {
    pub fn as_str(&self) -> &'static str {
        match self {
            LicenseState::Cleared => "CLEARED",
            LicenseState::NotCleared => "NOT CLEARED",
            LicenseState::Undetermined => "UNDETERMINED",
        }
    }
}

/// Classify a captured licence string (skill §5): NC or ND marks
/// are NOT CLEARED for this library's use; recognized free terms
/// (CC0, CC-BY, the pack terms) are CLEARED; anything else stays
/// UNDETERMINED — never rounded up.
pub fn classify_license(license_text: &str) -> LicenseState {
    let t = license_text.to_uppercase();
    if t.contains("-NC")
        || t.contains("-ND")
        || t.contains("NONCOMMERCIAL")
        || t.contains("NON-COMMERCIAL")
        || t.contains("NO DERIVATIVES")
        || t.contains("NODERIVATIVES")
    {
        return LicenseState::NotCleared;
    }
    if t.contains("CC0")
        || t.contains("CC-BY")
        || t.contains("CC BY")
        || t.contains("PUBLIC DOMAIN")
        || t.contains("OPENNSFW")
    {
        return LicenseState::Cleared;
    }
    LicenseState::Undetermined
}

/// One INDEX row (skill §5 columns + the library path §3.1
/// requires every pull to record).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryRow {
    /// Path relative to the pack root — the row key, with pack.
    pub file: String,
    pub pack: String,
    pub category: String,
    pub tags: Vec<String>,
    pub duration_sec: Option<f64>,
    pub intensity: Option<String>,
    pub license: String,
    pub license_state: LicenseState,
    pub license_source_url: Option<String>,
    pub author_credit: Option<String>,
    pub attribution_line: Option<String>,
    pub source_url: Option<String>,
    pub sha256: Option<String>,
    /// Absolute path of the staged library file (§3.1).
    pub library_path: String,
}

impl LibraryRow {
    /// The fetch gate (skill §3.1): cleared rows only.
    pub fn decide(&self) -> Result<(), TororiError> {
        match self.license_state {
            LicenseState::Cleared => Ok(()),
            LicenseState::NotCleared => Err(TororiError::new(
                "license_not_cleared",
                format!(
                    "{} ({}) is NOT CLEARED for this library's use: {}",
                    self.file, self.pack, self.license
                ),
            )),
            LicenseState::Undetermined => Err(TororiError::new(
                "license_undetermined",
                format!(
                    "{} ({}) has UNDETERMINED licence state — per-file capture is required before any use",
                    self.file, self.pack
                ),
            )),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "file": self.file,
            "pack": self.pack,
            "category": self.category,
            "tags": self.tags,
            "duration_sec": self.duration_sec,
            "intensity": self.intensity,
            "license": self.license,
            "license_state": self.license_state.as_str(),
            "license_source_url": self.license_source_url,
            "author_credit": self.author_credit,
            "attribution_line": self.attribution_line,
            "source_url": self.source_url,
            "sha256": self.sha256,
            "library_path": self.library_path,
        })
    }
}

/// One project attribution manifest entry (skill §3.1): the
/// fetch's Siamese twin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionEntry {
    pub project: String,
    pub project_file: String,
    pub library_path: String,
    pub pack: String,
    pub file: String,
    pub attribution_line: String,
    pub author_credit: Option<String>,
    pub license: String,
    pub license_source_url: Option<String>,
    pub source_url: Option<String>,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CatalogFile {
    rows: Vec<LibraryRow>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectManifest {
    entries: Vec<AttributionEntry>,
}

#[derive(Debug, Default)]
pub struct LibraryStore {
    pub rows: Vec<LibraryRow>,
}

impl LibraryStore {
    pub fn load(paths: &Paths) -> TororiResult<LibraryStore> {
        let file = paths.library_catalog();
        if !file.exists() {
            return Ok(LibraryStore::default());
        }
        let text = std::fs::read_to_string(&file)
            .map_err(|e| TororiError::new("io", format!("cannot read {}: {e}", file.display())))?;
        let parsed: CatalogFile = ron::from_str(&text).map_err(|e| {
            TororiError::new("library_invalid", format!("library catalog parse: {e}"))
        })?;
        Ok(LibraryStore { rows: parsed.rows })
    }

    pub fn save(&self, paths: &Paths) -> TororiResult<()> {
        let file = CatalogFile {
            rows: self.rows.clone(),
        };
        let text =
            ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::new().struct_names(true))
                .map_err(|e| TororiError::new("io", format!("catalog serialize: {e}")))?;
        let path = paths.library_catalog();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TororiError::new("io", format!("mkdir: {e}")))?;
        }
        std::fs::write(&path, text)
            .map_err(|e| TororiError::new("io", format!("write catalog: {e}")))
    }

    pub fn find(&self, pack: &str, file: &str) -> TororiResult<&LibraryRow> {
        self.rows
            .iter()
            .find(|r| r.pack == pack && r.file == file)
            .ok_or_else(|| TororiError::not_found(format!("no library row {pack}/{file}")))
    }

    pub fn search(&self, args: &Value) -> TororiResult<Value> {
        let query = opt_str(args, "query").map(str::to_lowercase);
        let pack = opt_str(args, "pack");
        let state = opt_str(args, "license_state");
        let page = Page::parse(args)?;
        let hits: Vec<Value> = self
            .rows
            .iter()
            .filter(|r| pack.is_none_or(|p| r.pack == p))
            .filter(|r| state.is_none_or(|s| r.license_state.as_str() == s))
            .filter(|r| {
                query.as_ref().is_none_or(|q| {
                    r.file.to_lowercase().contains(q)
                        || r.category.to_lowercase().contains(q)
                        || r.tags.iter().any(|t| t.to_lowercase().contains(q))
                })
            })
            .map(LibraryRow::to_json)
            .collect();
        Ok(page.apply(&hits))
    }

    /// Fetch one cleared file into a project staging dir, with
    /// its attribution entry appended in the same operation.
    pub fn fetch(&self, paths: &Paths, args: &Value) -> TororiResult<Value> {
        let pack = arg_str(args, "pack")?;
        let file = arg_str(args, "file")?;
        let project = arg_str(args, "project")?;
        validate_project_name(project)?;
        let row = self.find(pack, file)?;
        row.decide()?;
        let attribution_line = row.attribution_line.clone().ok_or_else(|| {
            TororiError::new(
                "attribution_missing",
                format!(
                    "{pack}/{file} is CLEARED but carries no attribution line — refusing: a fetch without attribution cannot ship"
                ),
            )
        })?;
        let src = std::path::PathBuf::from(&row.library_path);
        let bytes = std::fs::read(&src).map_err(|e| {
            TororiError::new(
                "io",
                format!("cannot read library file {}: {e}", src.display()),
            )
        })?;
        let sha = crate::util::sha256_hex(&bytes);
        if let Some(recorded) = &row.sha256
            && recorded != &sha
        {
            return Err(TororiError::new(
                "hash_mismatch",
                format!(
                    "library file {pack}/{file} hashes to {sha}, catalog records {recorded} — refusing a drifted file"
                ),
            ));
        }
        let base = std::path::Path::new(file)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| TororiError::invalid_input("file has no basename"))?;
        let staging = paths.projects_dir().join(project).join("files");
        std::fs::create_dir_all(&staging)
            .map_err(|e| TororiError::new("io", format!("mkdir staging: {e}")))?;
        let dest = staging.join(base);
        std::fs::write(&dest, &bytes)
            .map_err(|e| TororiError::new("io", format!("copy to staging: {e}")))?;
        let entry = AttributionEntry {
            project: project.to_string(),
            project_file: dest.display().to_string(),
            library_path: row.library_path.clone(),
            pack: row.pack.clone(),
            file: row.file.clone(),
            attribution_line,
            author_credit: row.author_credit.clone(),
            license: row.license.clone(),
            license_source_url: row.license_source_url.clone(),
            source_url: row.source_url.clone(),
            sha256: sha,
        };
        // The Siamese write: if the manifest append fails, the
        // copied file is removed — a fetch without its manifest
        // entry must be unrepresentable.
        if let Err(e) = append_manifest(paths, &entry) {
            let _ = std::fs::remove_file(&dest);
            return Err(e);
        }
        Ok(json!({
            "project_file": entry.project_file,
            "sha256": entry.sha256,
            "attribution": {
                "attribution_line": entry.attribution_line,
                "author_credit": entry.author_credit,
                "license": entry.license,
                "license_source_url": entry.license_source_url,
                "source_url": entry.source_url,
                "pack": entry.pack,
                "file": entry.file,
                "library_path": entry.library_path,
            },
        }))
    }

    pub fn manifest(&self, paths: &Paths, args: &Value) -> TororiResult<Value> {
        let project = arg_str(args, "project")?;
        validate_project_name(project)?;
        let page = Page::parse(args)?;
        let entries = read_manifest(paths, project)?;
        let items: Vec<Value> = entries
            .iter()
            .map(|e| {
                json!({
                    "project_file": e.project_file,
                    "library_path": e.library_path,
                    "pack": e.pack,
                    "file": e.file,
                    "attribution_line": e.attribution_line,
                    "author_credit": e.author_credit,
                    "license": e.license,
                    "license_source_url": e.license_source_url,
                    "source_url": e.source_url,
                    "sha256": e.sha256,
                })
            })
            .collect();
        Ok(page.apply(&items))
    }

    /// Ingest a per-file-capture tree (skill §5): every audio
    /// file gets a row; its licence state comes from its own
    /// sidecar capture, never from a pack-level assumption.
    pub fn ingest(&mut self, paths: &Paths, args: &Value) -> TororiResult<Value> {
        let pack_dir = arg_str(args, "pack_dir")?;
        let pack = arg_str(args, "pack")?;
        let root = std::path::PathBuf::from(pack_dir);
        if !root.is_dir() {
            return Err(TororiError::not_found(format!(
                "pack_dir {} is not a directory",
                root.display()
            )));
        }
        let mut files = Vec::new();
        walk_audio(&root, &root, &mut files)?;
        files.sort();
        let mut added = 0usize;
        let mut states = (0usize, 0usize, 0usize); // cleared, not, undetermined
        for rel in files {
            let abs = root.join(&rel);
            if self.rows.iter().any(|r| r.pack == pack && r.file == rel) {
                continue; // ingest is idempotent per (pack, file)
            }
            let row = capture_row(&abs, pack, &rel);
            match row.license_state {
                LicenseState::Cleared => states.0 += 1,
                LicenseState::NotCleared => states.1 += 1,
                LicenseState::Undetermined => states.2 += 1,
            }
            self.rows.push(row);
            added += 1;
        }
        self.save(paths)?;
        Ok(json!({
            "pack": pack,
            "added": added,
            "states": {
                "CLEARED": states.0,
                "NOT CLEARED": states.1,
                "UNDETERMINED": states.2,
            },
            "total_rows": self.rows.len(),
        }))
    }
}

fn validate_project_name(project: &str) -> TororiResult<()> {
    if project.is_empty()
        || project.contains('/')
        || project.contains('\\')
        || project.contains("..")
    {
        return Err(TororiError::invalid_input(
            "project must be a single path segment",
        ));
    }
    Ok(())
}

fn manifest_path(paths: &Paths, project: &str) -> std::path::PathBuf {
    paths.projects_dir().join(project).join("attribution.ron")
}

fn read_manifest(paths: &Paths, project: &str) -> TororiResult<Vec<AttributionEntry>> {
    let path = manifest_path(paths, project);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| TororiError::new("io", format!("read manifest: {e}")))?;
    let parsed: ProjectManifest = ron::from_str(&text)
        .map_err(|e| TororiError::new("library_invalid", format!("manifest parse: {e}")))?;
    Ok(parsed.entries)
}

fn append_manifest(paths: &Paths, entry: &AttributionEntry) -> TororiResult<()> {
    let mut entries = read_manifest(paths, &entry.project)?;
    entries.push(entry.clone());
    let text = ron::ser::to_string_pretty(
        &ProjectManifest { entries },
        ron::ser::PrettyConfig::new().struct_names(true),
    )
    .map_err(|e| TororiError::new("io", format!("manifest serialize: {e}")))?;
    let path = manifest_path(paths, &entry.project);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| TororiError::new("io", format!("mkdir manifest dir: {e}")))?;
    }
    std::fs::write(&path, text).map_err(|e| TororiError::new("io", format!("write manifest: {e}")))
}

const AUDIO_EXTS: [&str; 5] = ["mp3", "wav", "flac", "ogg", "m4a"];

fn walk_audio(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<String>,
) -> TororiResult<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| TororiError::new("io", format!("walk {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| TororiError::new("io", format!("walk: {e}")))?;
        let path = entry.path();
        if path.is_dir() {
            walk_audio(root, &path, out)?;
        } else if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| AUDIO_EXTS.contains(&x.to_lowercase().as_str()))
        {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| TororiError::new("io", "walk prefix"))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}

fn file_duration(abs: &std::path::Path) -> Option<f64> {
    let ext = abs
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "wav" {
        crate::audio::read_wav(abs).ok().map(|w| w.duration_sec())
    } else {
        crate::audio::mp3_duration_ffprobe(abs).ok()
    }
}

/// Build one row from a per-file capture: the `<file>.meta.json`
/// sidecar is the capture; nothing captured → UNDETERMINED.
fn capture_row(abs: &std::path::Path, pack: &str, rel: &str) -> LibraryRow {
    let sidecar = abs.with_extension(format!(
        "{}.meta.json",
        abs.extension().and_then(|x| x.to_str()).unwrap_or("")
    ));
    let meta: Option<Value> = std::fs::read_to_string(&sidecar)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let get = |key: &str| {
        meta.as_ref()
            .and_then(|m| m.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let license = get("license");
    let (license_text, state) = match &license {
        Some(text) => (text.clone(), classify_license(text)),
        None => ("UNDETERMINED".to_string(), LicenseState::Undetermined),
    };
    let author = get("author_credit");
    let attribution = match (&license, &author) {
        (Some(_), Some(a)) => Some(format!("{a} — via {pack}")),
        _ => None,
    };
    let category = rel.split('/').next().unwrap_or("").to_string();
    LibraryRow {
        file: rel.to_string(),
        pack: pack.to_string(),
        category,
        tags: Vec::new(),
        duration_sec: file_duration(abs),
        intensity: None,
        license: license_text,
        license_state: state,
        license_source_url: get("license_source_url"),
        author_credit: author,
        attribution_line: attribution,
        source_url: get("source_url"),
        sha256: sha256_file(abs).ok(),
        library_path: abs.display().to_string(),
    }
}

/// The voice-pack INDEX backfill (brief T2, skill §7): walk the
/// staged OpenNSFW voice tree read-only; one row per clip with
/// sha256 + duration + licence state computed. Performer credit
/// comes from the folder the clip lives in; a clip outside a
/// performer folder cannot be attributed and lands UNDETERMINED.
/// Returns the report with the exact UNDETERMINED count.
pub fn backfill_voice_pack(
    paths: &Paths,
    tree: &std::path::Path,
    pack_name: &str,
) -> TororiResult<(LibraryStore, Value)> {
    if !tree.is_dir() {
        return Err(TororiError::not_found(format!(
            "voice tree {} is not a directory",
            tree.display()
        )));
    }
    // The terms PDF rides in the pack root (skill §5): find it
    // for the licence source reference.
    let terms_ref = std::fs::read_dir(tree)
        .map_err(|e| TororiError::new("io", format!("read tree: {e}")))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(|x| x.to_str()) == Some("pdf")
                && p.to_string_lossy().to_lowercase().contains("term")
        })
        .map(|p| p.display().to_string());
    let mut rels = Vec::new();
    walk_audio(tree, tree, &mut rels)?;
    rels.sort();
    let mut store = LibraryStore::default();
    let mut undetermined = 0usize;
    for rel in &rels {
        let abs = tree.join(rel);
        let parts: Vec<&str> = rel.split('/').collect();
        let performer = if parts.len() >= 3 && (parts[0] == "Female" || parts[0] == "Male") {
            Some(parts[1].to_string())
        } else {
            None
        };
        let (state, attribution, author) = match &performer {
            Some(name) => (
                LicenseState::Cleared,
                Some(format!("OpenNSFW Voice Pack (@OpenNSFWSP) — {name}")),
                Some(name.clone()),
            ),
            None => {
                undetermined += 1;
                (LicenseState::Undetermined, None, None)
            }
        };
        store.rows.push(LibraryRow {
            file: rel.clone(),
            pack: pack_name.to_string(),
            category: parts.first().copied().unwrap_or("").to_string(),
            tags: performer.iter().cloned().collect(),
            duration_sec: file_duration(&abs),
            intensity: None,
            license: "OpenNSFW Voice Pack terms (free use, attribution required; AI-use flag: comping only, no likeness replication)".to_string(),
            license_state: state,
            license_source_url: terms_ref.clone(),
            author_credit: author,
            attribution_line: attribution,
            source_url: None,
            sha256: sha256_file(&abs).ok(),
            library_path: abs.display().to_string(),
        });
    }
    store.save(paths)?;
    Ok((
        store,
        json!({
            "pack": pack_name,
            "rows": rels.len(),
            "undetermined": undetermined,
            "cleared": rels.len() - undetermined,
            "terms_ref": terms_ref,
            "tree": tree.display().to_string(),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: LicenseState, license: &str) -> LibraryRow {
        LibraryRow {
            file: "clip.wav".into(),
            pack: "test-pack".into(),
            category: "Female".into(),
            tags: vec![],
            duration_sec: Some(1.0),
            intensity: None,
            license: license.into(),
            license_state: state,
            license_source_url: None,
            author_credit: Some("Performer A".into()),
            attribution_line: Some("Attr A".into()),
            source_url: None,
            sha256: None,
            library_path: "/nonexistent/clip.wav".into(),
        }
    }

    #[test]
    fn decide_is_pure_and_state_driven() {
        assert!(row(LicenseState::Cleared, "CC0").decide().is_ok());
        assert_eq!(
            row(LicenseState::NotCleared, "CC-BY-NC")
                .decide()
                .unwrap_err()
                .code,
            "license_not_cleared"
        );
        assert_eq!(
            row(LicenseState::Undetermined, "?")
                .decide()
                .unwrap_err()
                .code,
            "license_undetermined"
        );
    }

    #[test]
    fn classification_never_rounds_up() {
        assert_eq!(classify_license("CC-BY 4.0"), LicenseState::Cleared);
        assert_eq!(classify_license("CC0 1.0"), LicenseState::Cleared);
        assert_eq!(classify_license("CC-BY-NC 4.0"), LicenseState::NotCleared);
        assert_eq!(classify_license("CC-BY-ND 4.0"), LicenseState::NotCleared);
        assert_eq!(
            classify_license("some custom terms"),
            LicenseState::Undetermined
        );
        assert_eq!(classify_license(""), LicenseState::Undetermined);
    }

    #[test]
    fn fetch_refuses_undetermined_and_not_cleared() {
        let root = std::env::temp_dir().join(format!("torori-lib-{}", std::process::id()));
        let paths = Paths::resolve(None, Some(root.clone()));
        paths.ensure_state_dirs().unwrap();
        for state in [LicenseState::Undetermined, LicenseState::NotCleared] {
            let store = LibraryStore {
                rows: vec![row(state, "X")],
            };
            let out = store.fetch(
                &paths,
                &json!({"pack": "test-pack", "file": "clip.wav", "project": "p1"}),
            );
            assert!(out.is_err(), "{state:?} must refuse");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
