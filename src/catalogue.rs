//! The catalogue core: the TORORI catalogue as governed data.
//!
//! Data of record: `data/catalogue/entries.ron`, derived
//! mechanically from `TORORI-CATALOGUE.md` (catalogue of record)
//! by `tools/gen_catalogue.py`. Loading validates shape (unique
//! slugs, the B-01..=B-80 index complete) — drift fails startup
//! loudly rather than being served (brief §4, T1 gate).
//!
//! The status state machine (brief §4): HAVE-LIBRARY /
//! MEASURED-ONLY / PLANNED / GAP are provenance classes from the
//! source doc's triage; GENERATED is reachable only through
//! `transition` with produced assets + complete §5.2 manifests,
//! and GENERATED→produced form only on an Owner ear-admission
//! verdict recorded in the transition call. GENERATED never
//! overwrites a provenance class — the class an entry arrived
//! with stays on the record.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{TororiError, TororiResult};
use crate::state::Paths;
use crate::util::{Page, arg_str, opt_str};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Kink,
    Act,
    Reaction,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Kink => "KINK",
            Kind::Act => "ACT",
            Kind::Reaction => "REACTION",
        }
    }

    fn parse(s: &str) -> TororiResult<Kind> {
        match s {
            "KINK" => Ok(Kind::Kink),
            "ACT" => Ok(Kind::Act),
            "REACTION" => Ok(Kind::Reaction),
            other => Err(TororiError::invalid_input(format!(
                "unknown kind {other:?} (expected KINK|ACT|REACTION)"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    HaveLibrary,
    MeasuredOnly,
    Planned,
    Gap,
    Generated,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::HaveLibrary => "HAVE-LIBRARY",
            Status::MeasuredOnly => "MEASURED-ONLY",
            Status::Planned => "PLANNED",
            Status::Gap => "GAP",
            Status::Generated => "GENERATED",
        }
    }

    fn parse(s: &str) -> TororiResult<Status> {
        match s {
            "HAVE-LIBRARY" => Ok(Status::HaveLibrary),
            "MEASURED-ONLY" => Ok(Status::MeasuredOnly),
            "PLANNED" => Ok(Status::Planned),
            "GAP" => Ok(Status::Gap),
            "GENERATED" => Ok(Status::Generated),
            other => Err(TororiError::invalid_input(format!(
                "unknown status {other:?}"
            ))),
        }
    }

    /// True for the four provenance classes of the source
    /// catalogue (GENERATED is a production state, not a class).
    fn is_provenance(&self) -> bool {
        !matches!(self, Status::Generated)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub slug: String,
    pub title: String,
    pub kind: Kind,
    pub brief: Option<String>,
    pub description: String,
    pub assets: Option<String>,
    pub coupling: Option<String>,
    pub source: Option<String>,
    pub status: Status,
    pub status_note: Option<String>,
    /// GAP5 §6.1 delta line (`**Pair:**`), migration batches.
    pub pair: Option<String>,
    /// GAP5 §6.1 delta line (`**Form:**`), migration batches.
    pub form: Option<String>,
    /// The provenance class the entry arrived with; GENERATED
    /// transitions record production without overwriting it.
    #[serde(default)]
    pub origin_status: Option<Status>,
    /// Production record, present once GENERATED: asset refs +
    /// manifest refs (asset_id → manifest asset_id).
    #[serde(default)]
    pub generated: Option<GeneratedRecord>,
    /// Owner ear-admission verdict, recorded as a gate event.
    #[serde(default)]
    pub ear_admission: Option<EarAdmission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedRecord {
    pub asset_refs: Vec<String>,
    pub manifest_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarAdmission {
    pub verdict: String,
    pub admitted_by: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CatalogueFile {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub entries: Vec<Entry>,
}

impl Catalogue {
    pub fn load(paths: &Paths) -> TororiResult<Catalogue> {
        let file = paths.catalogue_file();
        if !file.exists() {
            // First run without shipped data: an empty catalogue
            // is honest; drift checks apply to present data.
            return Ok(Catalogue::default());
        }
        let text = std::fs::read_to_string(&file)
            .map_err(|e| TororiError::new("io", format!("cannot read {}: {e}", file.display())))?;
        let parsed: CatalogueFile = ron::from_str(&text).map_err(|e| {
            TororiError::new(
                "catalogue_invalid",
                format!("{} does not parse: {e}", file.display()),
            )
        })?;
        let catalogue = Catalogue {
            entries: parsed.entries,
        };
        catalogue.validate()?;
        Ok(catalogue)
    }

    /// Load-time drift validation (brief §4 / T1 gate): unique
    /// slugs, non-empty titles/descriptions, and the B-01..=B-80
    /// index spans complete with no duplicates.
    pub fn validate(&self) -> TororiResult<()> {
        let mut slugs = std::collections::BTreeSet::new();
        let mut briefs = std::collections::BTreeSet::new();
        for entry in &self.entries {
            if entry.slug.is_empty() || entry.title.is_empty() || entry.description.is_empty() {
                return Err(TororiError::new(
                    "catalogue_invalid",
                    format!("entry {:?} has an empty slug/title/description", entry.slug),
                ));
            }
            if !slugs.insert(&entry.slug) {
                return Err(TororiError::new(
                    "catalogue_invalid",
                    format!("duplicate slug {}", entry.slug),
                ));
            }
            if !entry.status.is_provenance() && entry.origin_status.is_none() {
                return Err(TororiError::new(
                    "catalogue_invalid",
                    format!(
                        "entry {} is GENERATED without a recorded provenance class",
                        entry.slug
                    ),
                ));
            }
            if let Some(brief) = &entry.brief {
                let n: u32 = brief
                    .strip_prefix("B-")
                    .and_then(|s| s.parse().ok())
                    .filter(|n| (1..=80).contains(n))
                    .ok_or_else(|| {
                        TororiError::new(
                            "catalogue_invalid",
                            format!("entry {} has malformed brief ref {brief}", entry.slug),
                        )
                    })?;
                if !briefs.insert(n) {
                    return Err(TororiError::new(
                        "catalogue_invalid",
                        format!("duplicate brief ref {brief}"),
                    ));
                }
            }
        }
        for n in 1..=80u32 {
            if !briefs.contains(&n) {
                return Err(TororiError::new(
                    "catalogue_invalid",
                    format!("B-index incomplete: B-{n:02} missing (drift)"),
                ));
            }
        }
        Ok(())
    }

    fn overlay_file(paths: &Paths) -> std::path::PathBuf {
        paths.state_dir.join("catalogue-transitions.ron")
    }

    /// Apply the persisted transition overlay (GENERATED records
    /// and ear admissions live in the state dir; the shipped
    /// data file stays read-only).
    pub fn load_overlay(&mut self, paths: &Paths) -> TororiResult<()> {
        let file = Self::overlay_file(paths);
        if !file.exists() {
            return Ok(());
        }
        let text = std::fs::read_to_string(&file)
            .map_err(|e| TororiError::new("io", format!("cannot read {}: {e}", file.display())))?;
        let parsed: CatalogueFile = ron::from_str(&text).map_err(|e| {
            TororiError::new("catalogue_invalid", format!("overlay does not parse: {e}"))
        })?;
        for overlay in parsed.entries {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.slug == overlay.slug) {
                entry.status = overlay.status;
                entry.origin_status = overlay.origin_status;
                entry.generated = overlay.generated;
                entry.ear_admission = overlay.ear_admission;
            }
        }
        Ok(())
    }

    /// Persist the overlay: only entries carrying production
    /// state are written.
    pub fn save_overlay(&self, paths: &Paths) -> TororiResult<()> {
        let changed: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| e.generated.is_some() || e.ear_admission.is_some())
            .cloned()
            .collect();
        let file = CatalogueFile { entries: changed };
        let text =
            ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::new().struct_names(true))
                .map_err(|e| TororiError::new("io", format!("overlay serialize: {e}")))?;
        std::fs::write(Self::overlay_file(paths), text)
            .map_err(|e| TororiError::new("io", format!("cannot write overlay: {e}")))
    }

    pub fn get(&self, slug: &str) -> TororiResult<&Entry> {
        self.entries
            .iter()
            .find(|e| e.slug == slug)
            .ok_or_else(|| TororiError::not_found(format!("no catalogue entry {slug}")))
    }

    fn get_mut(&mut self, slug: &str) -> TororiResult<&mut Entry> {
        self.entries
            .iter_mut()
            .find(|e| e.slug == slug)
            .ok_or_else(|| TororiError::not_found(format!("no catalogue entry {slug}")))
    }

    /// Server-computed counts (brief §4: computed here, never by
    /// a caller).
    pub fn stats(&self) -> Value {
        let mut by_kind = serde_json::Map::new();
        let mut by_status = serde_json::Map::new();
        for kind in [Kind::Kink, Kind::Act, Kind::Reaction] {
            let n = self.entries.iter().filter(|e| e.kind == kind).count();
            by_kind.insert(kind.as_str().to_string(), json!(n));
        }
        for status in [
            Status::HaveLibrary,
            Status::MeasuredOnly,
            Status::Planned,
            Status::Gap,
            Status::Generated,
        ] {
            let n = self.entries.iter().filter(|e| e.status == status).count();
            if n > 0 || status.is_provenance() {
                by_status.insert(status.as_str().to_string(), json!(n));
            }
        }
        json!({
            "total": self.entries.len(),
            "by_kind": by_kind,
            "by_status": by_status,
            "with_pair": self.entries.iter().filter(|e| e.pair.is_some()).count(),
            "with_form": self.entries.iter().filter(|e| e.form.is_some()).count(),
        })
    }

    fn entry_json(&self, entry: &Entry) -> Value {
        json!({
            "slug": entry.slug,
            "title": entry.title,
            "kind": entry.kind.as_str(),
            "brief": entry.brief,
            "description": entry.description,
            "assets": entry.assets,
            "coupling": entry.coupling,
            "source": entry.source,
            "status": entry.status.as_str(),
            "status_note": entry.status_note,
            "pair": entry.pair,
            "form": entry.form,
            "origin_status": entry.origin_status.map(|s| s.as_str()),
            "generated": entry.generated.as_ref().map(|g| json!({
                "asset_refs": g.asset_refs,
                "manifest_refs": g.manifest_refs,
            })),
            "ear_admission": entry.ear_admission.as_ref().map(|a| json!({
                "verdict": a.verdict,
                "admitted_by": a.admitted_by,
                "note": a.note,
            })),
        })
    }

    pub fn search(&self, args: &Value) -> TororiResult<Value> {
        let query = opt_str(args, "query").map(str::to_lowercase);
        let kind = match opt_str(args, "kind") {
            Some(k) => Some(Kind::parse(k)?),
            None => None,
        };
        let status = match opt_str(args, "status") {
            Some(s) => Some(Status::parse(s)?),
            None => None,
        };
        let page = Page::parse(args)?;
        let hits: Vec<Value> = self
            .entries
            .iter()
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .filter(|e| status.is_none_or(|s| e.status == s))
            .filter(|e| {
                query.as_ref().is_none_or(|q| {
                    e.title.to_lowercase().contains(q)
                        || e.description.to_lowercase().contains(q)
                        || e.slug.contains(q.as_str())
                })
            })
            .map(|e| self.entry_json(e))
            .collect();
        Ok(page.apply(&hits))
    }

    pub fn get_tool(&self, args: &Value) -> TororiResult<Value> {
        let slug = arg_str(args, "slug")?;
        Ok(self.entry_json(self.get(slug)?))
    }

    /// The governed status transition (brief §4). Writes; every
    /// refusal is typed.
    pub fn transition(&mut self, args: &Value) -> TororiResult<Value> {
        let slug = arg_str(args, "slug")?.to_string();
        let to = Status::parse(arg_str(args, "to_status")?)?;
        let entry = self.get_mut(&slug)?;
        match to {
            Status::Generated => {
                if entry.status == Status::Generated {
                    return Err(TororiError::new(
                        "invalid_transition",
                        format!("{slug} is already GENERATED"),
                    ));
                }
                let asset_refs = string_list(args, "asset_refs")?;
                let manifest_refs = string_list(args, "manifest_refs")?;
                if asset_refs.is_empty() || manifest_refs.is_empty() {
                    return Err(TororiError::new(
                        "manifest_required",
                        "GENERATED requires produced asset refs and §5.2 manifest refs",
                    ));
                }
                if entry.origin_status.is_none() {
                    entry.origin_status = Some(entry.status);
                }
                entry.generated = Some(GeneratedRecord {
                    asset_refs,
                    manifest_refs,
                });
                entry.status = Status::Generated;
            }
            _ => {
                // GENERATED → produced form: the Owner's ear
                // admission, recorded as a gate event.
                if entry.status != Status::Generated {
                    return Err(TororiError::new(
                        "invalid_transition",
                        format!(
                            "{} is {}; only a GENERATED entry can be ear-admitted to a produced form",
                            slug,
                            entry.status.as_str()
                        ),
                    ));
                }
                let verdict = args
                    .get("ear_verdict")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        TororiError::new(
                            "ear_verdict_required",
                            "leaving GENERATED requires an ear_verdict record {verdict, admitted_by}",
                        )
                    })?;
                let verdict_text = verdict
                    .get("verdict")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        TororiError::new("ear_verdict_required", "ear_verdict.verdict is required")
                    })?;
                let admitted_by = verdict
                    .get("admitted_by")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        TororiError::new(
                            "ear_verdict_required",
                            "ear_verdict.admitted_by is required (the Owner's recorded verdict)",
                        )
                    })?;
                entry.ear_admission = Some(EarAdmission {
                    verdict: verdict_text.to_string(),
                    admitted_by: admitted_by.to_string(),
                    note: verdict
                        .get("note")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });
                entry.status = to;
            }
        }
        Ok(self.entry_json(self.get(&slug)?))
    }
}

/// The GAP5 §6.1 PLANNED-requires-Armed-grade lint, switched on
/// for migration batch 1: any catalogue entry that names a pair
/// while holding PLANNED must name a pair that exists in the
/// loaded program docs and validates Armed-grade there. Runs at
/// state load; a violation is a hard error, never served.
pub fn lint_named_pairs(
    catalogue: &Catalogue,
    docs: &[crate::pairs::ProgramDoc],
) -> TororiResult<()> {
    for entry in &catalogue.entries {
        let Some(pair_line) = &entry.pair else {
            continue;
        };
        if entry.status != Status::Planned {
            continue;
        }
        let pair_id = pair_line
            .split(" — ")
            .next()
            .unwrap_or(pair_line.as_str())
            .trim();
        let Some(doc) = docs
            .iter()
            .find(|d| d.pairs.iter().any(|p| p.pair_id == pair_id))
        else {
            return Err(TororiError::new(
                "catalogue_lint",
                format!(
                    "entry {} is PLANNED and names pair {pair_id}, which is not in any loaded program doc (§6.1 lint)",
                    entry.slug
                ),
            ));
        };
        let ctx = crate::pairs::ValidateCtx {
            asset_index: doc_internal_assets(doc),
            disclosures: doc
                .pairs
                .iter()
                .map(|p| p.install.effect.disclosure_ref.clone())
                .collect(),
            ship_templates: vec![
                ("T-RESET".into(), "reset".into()),
                ("T-INSTALL".into(), "install".into()),
            ],
            pair_templates: Vec::new(),
        };
        let violations = crate::pairs::validate_doc(doc, &ctx);
        if !violations.is_empty() {
            return Err(TororiError::new(
                "catalogue_lint",
                format!(
                    "entry {} is PLANNED but its program doc fails the pair battery: {} on {}",
                    entry.slug, violations[0].check, violations[0].pair_id
                ),
            ));
        }
    }
    Ok(())
}

/// Asset index built from a doc's own records (cue + release
/// refs) — for validating shipped data docs standalone.
pub fn doc_internal_assets(doc: &crate::pairs::ProgramDoc) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in &doc.pairs {
        out.push((
            pair.install.cue.asset_ref.clone(),
            pair.install.cue.asset_sha256.clone(),
        ));
        if let crate::pairs::ReleaseHalf::Embedded(body) = &pair.release {
            for r in &body.asset_refs {
                out.push((r.clone(), String::new()));
            }
            if let Some(cue) = &body.cue {
                out.push((cue.asset_ref.clone(), cue.asset_sha256.clone()));
            }
        }
    }
    for form in &doc.shared_releases {
        for r in &form.asset_refs {
            out.push((r.clone(), String::new()));
        }
        if let Some(cue) = &form.cue {
            out.push((cue.asset_ref.clone(), cue.asset_sha256.clone()));
        }
    }
    out
}

fn string_list(args: &Value, field: &str) -> TororiResult<Vec<String>> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let s = item.as_str().ok_or_else(|| {
                    TororiError::invalid_input(format!("{field} entries must be strings"))
                })?;
                out.push(s.to_string());
            }
            Ok(out)
        }
        Some(_) => Err(TororiError::invalid_input(format!(
            "{field} must be an array of strings"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Catalogue {
        let mut entries = Vec::new();
        for n in 1..=80u32 {
            entries.push(Entry {
                slug: format!("kink-e{n}"),
                title: format!("Entry {n}"),
                kind: Kind::Kink,
                brief: Some(format!("B-{n:02}")),
                description: "d".into(),
                assets: None,
                coupling: None,
                source: None,
                status: Status::Gap,
                status_note: None,
                pair: None,
                form: None,
                origin_status: None,
                generated: None,
                ear_admission: None,
            });
        }
        Catalogue { entries }
    }

    #[test]
    fn validation_adversarial_drift_fails() {
        let mut c = fixture();
        c.validate().expect("complete fixture validates");
        c.entries.pop();
        assert!(c.validate().is_err(), "missing B-80 must fail");
        let mut c = fixture();
        c.entries[3].slug = c.entries[4].slug.clone();
        assert!(c.validate().is_err(), "duplicate slug must fail");
        let mut c = fixture();
        c.entries[0].brief = Some("B-02".into());
        assert!(c.validate().is_err(), "duplicate brief must fail");
    }

    #[test]
    fn transition_adversarial_generated_without_manifests_refuses() {
        let mut c = fixture();
        let out = c.transition(&json!({"slug": "kink-e1", "to_status": "GENERATED"}));
        assert_eq!(out.unwrap_err().code, "manifest_required");
        let out = c.transition(&json!({
            "slug": "kink-e1", "to_status": "GENERATED",
            "asset_refs": ["assets/x.wav"], "manifest_refs": ["x"]
        }));
        assert!(out.is_ok());
        let entry = c.get("kink-e1").unwrap();
        assert_eq!(entry.status, Status::Generated);
        assert_eq!(entry.origin_status, Some(Status::Gap));
    }

    #[test]
    fn transition_adversarial_ear_admission_without_verdict_refuses() {
        let mut c = fixture();
        c.transition(&json!({
            "slug": "kink-e1", "to_status": "GENERATED",
            "asset_refs": ["assets/x.wav"], "manifest_refs": ["x"]
        }))
        .unwrap();
        let out = c.transition(&json!({"slug": "kink-e1", "to_status": "PLANNED"}));
        assert_eq!(out.unwrap_err().code, "ear_verdict_required");
        let out = c.transition(&json!({
            "slug": "kink-e1", "to_status": "PLANNED",
            "ear_verdict": {"verdict": "admitted", "admitted_by": "Owner"}
        }));
        assert!(out.is_ok());
        assert_eq!(c.get("kink-e1").unwrap().status, Status::Planned);
    }
}
