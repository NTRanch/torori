//! Cue-bundle ingest (brief §8, GAP5 §6, neemu SPEC §3.5).
//!
//! Bundles are FILES — the only torori↔neemu transport. A bundle
//! is a directory holding `bundle.ron` plus the asset files it
//! names. Ingest validation runs in the brief's order, each
//! failure a typed refusal, and partial ingest never happens:
//!
//! The order: schema version, then content hashes, then
//! provenance-chain completeness, then tier-stamp consistency,
//! then `target_voice` resolution, then pairs V1–V8.
//!
//! Staging: sources are copied to quarantine first; only a fully
//! validated bundle is promoted into the ingested store, and
//! unvalidated bundles are never visible to generation tools.
//! Ingest is idempotent on bundle content hash.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::conditioning::{ConditioningParams, VoiceRegistry};
use crate::error::{TororiError, TororiResult};
use crate::manifest::{AssetManifest, is_sha256_hex};
use crate::pairs::{ProgramDoc, TriggerPair, ValidateCtx, validate_doc};
use crate::state::Paths;
use crate::util::{Page, arg_str, sha256_file, sha256_hex};

pub const BUNDLE_SCHEMA_VERSION: u32 = 1;
pub const BUNDLE_SCHEMA_JSON: &str = include_str!("../data/schemas/bundle-v1.json");

/// Honesty tier stamped on cues by the producer (neemu §3.5);
/// torori re-checks it before any training-class use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    Measured,
    Authored,
    Inferred,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Measured => "MEASURED",
            Tier::Authored => "AUTHORED",
            Tier::Inferred => "INFERRED",
        }
    }
}

/// One neemu→torori cue (neemu SPEC §3.5, field for field).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TororiCue {
    pub cue_id: String,
    /// From the DialogueLine.
    pub text: String,
    /// The measured delivery targets, as a conditioning record.
    pub conditioning: ConditioningParams,
    /// scene_function.
    pub intent: String,
    /// Estate identity ref — REQUIRED, never a source performer.
    pub target_voice: String,
    /// Torori catalogue refs.
    pub sfx_pairs: Vec<String>,
    /// Inherited tier; torori re-checks before training use.
    pub tier: Tier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleAsset {
    /// Path inside the bundle directory.
    pub asset_ref: String,
    pub sha256: String,
    pub manifest: AssetManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceLink {
    pub step: String,
    pub actor: String,
    pub artifact_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueBundle {
    pub schema_version: u32,
    pub bundle_id: String,
    /// Producer identity (name@version+build).
    pub producer: String,
    pub cues: Vec<TororiCue>,
    pub pairs: Vec<TriggerPair>,
    /// Shared release forms the bundle's pairs reference.
    #[serde(default)]
    pub shared_releases: Vec<crate::pairs::ReleaseForm>,
    pub assets: Vec<BundleAsset>,
    pub provenance: Vec<ProvenanceLink>,
}

impl CueBundle {
    /// The bundle content hash: sha256 over the canonical RON of
    /// (cues, pairs, asset hashes) — the ingest identity.
    pub fn content_hash(&self) -> TororiResult<String> {
        let canonical = json!({
            "cues": serde_json::to_value(&self.cues).unwrap_or(Value::Null),
            "pairs": serde_json::to_value(&self.pairs).unwrap_or(Value::Null),
            "asset_sha256s": self.assets.iter().map(|a| a.sha256.clone()).collect::<Vec<_>>(),
        });
        let text = serde_json::to_string(&canonical)
            .map_err(|e| TororiError::new("io", format!("canonical form: {e}")))?;
        Ok(sha256_hex(text.as_bytes()))
    }
}

/// The full validation battery, in the brief's order. Returns
/// the typed refusal for the first failing stage.
pub fn validate_bundle(
    bundle: &CueBundle,
    bundle_dir: &std::path::Path,
    voices: &VoiceRegistry,
    disclosures: &[String],
) -> TororiResult<()> {
    // 1. Schema version.
    if bundle.schema_version != BUNDLE_SCHEMA_VERSION {
        return Err(TororiError::new(
            "bundle_schema_version",
            format!(
                "bundle schema {} is not supported (this build reads v{})",
                bundle.schema_version, BUNDLE_SCHEMA_VERSION
            ),
        ));
    }
    // 2. Content hashes: every asset file exists and hashes to
    //    its declared value; manifests validate complete.
    for asset in &bundle.assets {
        if !is_sha256_hex(&asset.sha256) {
            return Err(TororiError::new(
                "bundle_hash",
                format!("asset {} declares a malformed sha256", asset.asset_ref),
            ));
        }
        let path = bundle_dir.join(&asset.asset_ref);
        let actual = sha256_file(&path).map_err(|_| {
            TororiError::new(
                "bundle_hash",
                format!(
                    "asset {} is missing from the bundle directory",
                    asset.asset_ref
                ),
            )
        })?;
        if actual != asset.sha256 {
            return Err(TororiError::new(
                "bundle_hash",
                format!(
                    "asset {} hashes to {actual}, bundle declares {}",
                    asset.asset_ref, asset.sha256
                ),
            ));
        }
        let problems = asset.manifest.validate();
        if !problems.is_empty() {
            return Err(TororiError::new(
                "bundle_manifest",
                format!(
                    "asset {} manifest incomplete: {}",
                    asset.asset_ref,
                    problems.join("; ")
                ),
            ));
        }
        if asset.manifest.output_sha256 != asset.sha256 {
            return Err(TororiError::new(
                "bundle_manifest",
                format!(
                    "asset {} manifest output hash does not match the bundle hash",
                    asset.asset_ref
                ),
            ));
        }
    }
    // 3. Provenance chain complete: non-empty, well-formed, and
    //    the final link carries the bundle content hash.
    if bundle.provenance.is_empty() {
        return Err(TororiError::new(
            "bundle_provenance",
            "provenance chain is empty",
        ));
    }
    for link in &bundle.provenance {
        if link.step.is_empty() || link.actor.is_empty() || !is_sha256_hex(&link.artifact_sha256) {
            return Err(TororiError::new(
                "bundle_provenance",
                "provenance link is incomplete (step, actor, artifact hash required)",
            ));
        }
    }
    let content_hash = bundle.content_hash()?;
    if bundle.provenance.last().unwrap().artifact_sha256 != content_hash {
        return Err(TororiError::new(
            "bundle_provenance",
            "final provenance link does not carry the bundle content hash — chain incomplete",
        ));
    }
    // 4. Tier stamps consistent: cue tier matches the honesty
    //    label of what it will be used as — a MEASURED-stamped
    //    cue must carry measured delivery targets (an F0 median),
    //    and a cue's conditioning must not contradict the cue.
    for cue in &bundle.cues {
        if cue.conditioning.text != cue.text {
            return Err(TororiError::new(
                "bundle_tier",
                format!(
                    "cue {}: conditioning text does not match the cue text",
                    cue.cue_id
                ),
            ));
        }
        if cue.conditioning.identity_ref != cue.target_voice {
            return Err(TororiError::new(
                "bundle_tier",
                format!(
                    "cue {}: conditioning identity_ref does not match target_voice",
                    cue.cue_id
                ),
            ));
        }
        if cue.tier == Tier::Measured && cue.conditioning.f0_median_hz.is_none() {
            return Err(TororiError::new(
                "bundle_tier",
                format!(
                    "cue {} is stamped MEASURED but carries no measured F0 target — tier stamp inconsistent",
                    cue.cue_id
                ),
            ));
        }
        let problems = cue.conditioning.validate();
        if !problems.is_empty() {
            return Err(TororiError::new(
                "bundle_conditioning",
                format!("cue {}: {}", cue.cue_id, problems.join("; ")),
            ));
        }
    }
    // 5. target_voice resolves anchored-or-better.
    for cue in &bundle.cues {
        voices.resolve_anchored(&cue.target_voice)?;
    }
    // 6. Pairs pass V1–V8 — the whole battery, against the
    //    bundle's own program doc (its pairs + the shared forms
    //    it carries).
    if !bundle.pairs.is_empty() {
        let program_id = bundle.pairs[0].program.program_id.clone();
        let doc = ProgramDoc {
            program_id,
            pairs: bundle.pairs.clone(),
            shared_releases: bundle.shared_releases.clone(),
        };
        let ctx = ValidateCtx {
            asset_index: bundle
                .assets
                .iter()
                .map(|a| (a.asset_ref.clone(), a.sha256.clone()))
                .collect(),
            disclosures: disclosures.to_vec(),
            ship_templates: Vec::new(),
            pair_templates: Vec::new(),
        };
        let violations = validate_doc(&doc, &ctx);
        if !violations.is_empty() {
            let first = &violations[0];
            return Err(TororiError::new(
                "bundle_pairs",
                format!(
                    "pair battery {} on {}: {} ({} violation(s) total)",
                    first.check,
                    first.pair_id,
                    first.message,
                    violations.len()
                ),
            ));
        }
    }
    Ok(())
}

/// Read + parse a bundle directory's `bundle.ron`.
pub fn read_bundle(dir: &std::path::Path) -> TororiResult<CueBundle> {
    let file = dir.join("bundle.ron");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| TororiError::new("io", format!("cannot read {}: {e}", file.display())))?;
    ron::from_str(&text)
        .map_err(|e| TororiError::new("bundle_invalid", format!("bundle.ron does not parse: {e}")))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestedRecord {
    pub bundle_id: String,
    pub content_hash: String,
    pub producer: String,
    pub cue_count: usize,
    pub pair_count: usize,
    pub stored_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexFile {
    records: Vec<IngestedRecord>,
}

#[derive(Debug, Default)]
pub struct BundleStore {
    pub ingested: Vec<IngestedRecord>,
    /// Program docs loaded from data/pairs (pair_list source).
    pub program_docs: Vec<ProgramDoc>,
}

impl BundleStore {
    pub fn load(paths: &Paths) -> TororiResult<BundleStore> {
        let mut store = BundleStore::default();
        let index = paths.bundles_store().join("index.ron");
        if index.exists() {
            let text = std::fs::read_to_string(&index)
                .map_err(|e| TororiError::new("io", format!("read index: {e}")))?;
            let parsed: IndexFile = ron::from_str(&text)
                .map_err(|e| TororiError::new("bundle_invalid", format!("index parse: {e}")))?;
            store.ingested = parsed.records;
        }
        let pairs_dir = paths.pairs_dir();
        if pairs_dir.is_dir() {
            let mut names: Vec<_> = std::fs::read_dir(&pairs_dir)
                .map_err(|e| TororiError::new("io", format!("read pairs dir: {e}")))?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("ron"))
                .collect();
            names.sort();
            for path in names {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| TororiError::new("io", format!("read {}: {e}", path.display())))?;
                let doc: ProgramDoc = ron::from_str(&text).map_err(|e| {
                    TororiError::new(
                        "bundle_invalid",
                        format!("{} does not parse as a program doc: {e}", path.display()),
                    )
                })?;
                store.program_docs.push(doc);
            }
        }
        Ok(store)
    }

    fn save_index(&self, paths: &Paths) -> TororiResult<()> {
        let file = IndexFile {
            records: self.ingested.clone(),
        };
        let text =
            ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::new().struct_names(true))
                .map_err(|e| TororiError::new("io", format!("index serialize: {e}")))?;
        std::fs::write(paths.bundles_store().join("index.ron"), text)
            .map_err(|e| TororiError::new("io", format!("write index: {e}")))
    }

    pub fn pair_count(&self) -> usize {
        let from_docs: usize = self.program_docs.iter().map(|d| d.pairs.len()).sum();
        let from_bundles: usize = self.ingested.iter().map(|r| r.pair_count).sum();
        from_docs + from_bundles
    }

    /// Validate a bundle directory WITHOUT ingesting (the file
    /// stays where it is; nothing enters the store).
    pub fn validate_tool(
        &self,
        paths: &Paths,
        voices: &VoiceRegistry,
        args: &Value,
    ) -> TororiResult<Value> {
        let dir = arg_str(args, "bundle_dir")?;
        let bundle = read_bundle(std::path::Path::new(dir))?;
        let disclosures = disclosure_set(args);
        validate_bundle(&bundle, std::path::Path::new(dir), voices, &disclosures)?;
        let _ = paths;
        Ok(json!({
            "bundle_id": bundle.bundle_id,
            "valid": true,
            "content_hash": bundle.content_hash()?,
            "cues": bundle.cues.len(),
            "pairs": bundle.pairs.len(),
            "assets": bundle.assets.len(),
        }))
    }

    /// Ingest: quarantine-copy → validate → promote. Idempotent
    /// on content hash.
    pub fn ingest(
        &mut self,
        paths: &Paths,
        voices: &VoiceRegistry,
        args: &Value,
    ) -> TororiResult<Value> {
        let src_dir = std::path::PathBuf::from(arg_str(args, "bundle_dir")?);
        let bundle = read_bundle(&src_dir)?;
        let content_hash = bundle.content_hash()?;
        if let Some(existing) = self
            .ingested
            .iter()
            .find(|r| r.content_hash == content_hash)
        {
            return Ok(json!({
                "bundle_id": existing.bundle_id,
                "content_hash": existing.content_hash,
                "idempotent": true,
                "cues": existing.cue_count,
                "pairs": existing.pair_count,
                "stored_dir": existing.stored_dir,
            }));
        }
        // Quarantine copy: validation runs against the copy, so
        // what was validated is byte-for-byte what gets stored.
        let quarantine = paths.bundles_quarantine().join(&content_hash);
        if quarantine.exists() {
            std::fs::remove_dir_all(&quarantine)
                .map_err(|e| TororiError::new("io", format!("clean quarantine: {e}")))?;
        }
        copy_dir(&src_dir, &quarantine)?;
        let disclosures = disclosure_set(args);
        // Quarantine keeps the evidence on failure; the store
        // never sees an unvalidated bundle.
        validate_bundle(&bundle, &quarantine, voices, &disclosures)?;
        let stored =
            paths
                .bundles_store()
                .join(format!("{}@{}", bundle.bundle_id, &content_hash[..12]));
        if stored.exists() {
            std::fs::remove_dir_all(&stored)
                .map_err(|e| TororiError::new("io", format!("clean store dir: {e}")))?;
        }
        std::fs::rename(&quarantine, &stored)
            .map_err(|e| TororiError::new("io", format!("promote bundle: {e}")))?;
        let record = IngestedRecord {
            bundle_id: bundle.bundle_id.clone(),
            content_hash: content_hash.clone(),
            producer: bundle.producer.clone(),
            cue_count: bundle.cues.len(),
            pair_count: bundle.pairs.len(),
            stored_dir: stored.display().to_string(),
        };
        self.ingested.push(record.clone());
        self.save_index(paths)?;
        Ok(json!({
            "bundle_id": record.bundle_id,
            "content_hash": record.content_hash,
            "idempotent": false,
            "cues": record.cue_count,
            "pairs": record.pair_count,
            "stored_dir": record.stored_dir,
        }))
    }

    /// `torori_pair_validate`: run the V1–V8 battery over one
    /// pair record (plus caller-supplied shared forms, asset
    /// index, and disclosure set).
    pub fn pair_validate(&self, args: &Value) -> TororiResult<Value> {
        let pair_value = crate::util::arg_value(args, "pair")?;
        let pair: TriggerPair = serde_json::from_value(pair_value).map_err(|e| {
            TororiError::invalid_input(format!("pair does not parse as a TriggerPair: {e}"))
        })?;
        let shared: Vec<crate::pairs::ReleaseForm> = match args.get("shared_releases") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                TororiError::invalid_input(format!("shared_releases do not parse: {e}"))
            })?,
        };
        let doc = ProgramDoc {
            program_id: pair.program.program_id.clone(),
            pairs: vec![pair],
            shared_releases: shared,
        };
        let asset_index: Vec<(String, String)> = match args.get("asset_index") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|i| {
                    Some((
                        i.get("asset_ref")?.as_str()?.to_string(),
                        i.get("sha256")?.as_str()?.to_string(),
                    ))
                })
                .collect(),
            Some(_) => {
                return Err(TororiError::invalid_input(
                    "asset_index must be an array of {asset_ref, sha256}",
                ));
            }
        };
        let ctx = ValidateCtx {
            asset_index,
            disclosures: disclosure_set(args),
            ship_templates: Vec::new(),
            pair_templates: Vec::new(),
        };
        let violations = validate_doc(&doc, &ctx);
        Ok(crate::pairs::violations_json(&violations))
    }

    /// `torori_pair_list`: every pair visible to the server —
    /// data/pairs program docs plus ingested bundles.
    pub fn pair_list(&self, args: &Value) -> TororiResult<Value> {
        let page = Page::parse(args)?;
        let mut items: Vec<Value> = Vec::new();
        let mut push = |pair: &TriggerPair, origin: &str| {
            items.push(json!({
                "pair_id": pair.pair_id,
                "program_id": pair.program.program_id,
                "consumer_class": format!("{:?}", pair.program.consumer_class),
                "subject_form": format!("{:?}", pair.subject.form),
                "cue_kind": format!("{:?}", pair.install.cue.kind),
                "cue_sha256": pair.install.cue.asset_sha256,
                "release": match &pair.release {
                    crate::pairs::ReleaseHalf::Embedded(_) => "embedded".to_string(),
                    crate::pairs::ReleaseHalf::Shared(id) => format!("shared:{id}"),
                },
                "origin": origin,
            }));
        };
        for doc in &self.program_docs {
            for pair in &doc.pairs {
                push(pair, "data/pairs");
            }
        }
        for record in &self.ingested {
            if let Ok(bundle) = read_bundle(std::path::Path::new(&record.stored_dir)) {
                for pair in &bundle.pairs {
                    push(pair, &record.bundle_id);
                }
            }
        }
        Ok(page.apply(&items))
    }

    pub fn list_bundles(&self, args: &Value) -> TororiResult<Value> {
        let page = Page::parse(args)?;
        let items: Vec<Value> = self
            .ingested
            .iter()
            .map(|r| {
                json!({
                    "bundle_id": r.bundle_id,
                    "content_hash": r.content_hash,
                    "producer": r.producer,
                    "cues": r.cue_count,
                    "pairs": r.pair_count,
                    "stored_dir": r.stored_dir,
                })
            })
            .collect();
        Ok(page.apply(&items))
    }
}

fn disclosure_set(args: &Value) -> Vec<String> {
    args.get("disclosures")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> TororiResult<()> {
    std::fs::create_dir_all(dst)
        .map_err(|e| TororiError::new("io", format!("mkdir {}: {e}", dst.display())))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| TororiError::new("io", format!("read {}: {e}", src.display())))?
    {
        let entry = entry.map_err(|e| TororiError::new("io", format!("copy: {e}")))?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)
                .map_err(|e| TororiError::new("io", format!("copy {}: {e}", path.display())))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_schema_descriptor_is_v1() {
        let descriptor: Value = serde_json::from_str(BUNDLE_SCHEMA_JSON).unwrap();
        assert_eq!(descriptor["version"], BUNDLE_SCHEMA_VERSION);
        assert_eq!(
            descriptor["validation_order"],
            json!([
                "schema_version",
                "content_hashes",
                "provenance_chain",
                "tier_stamps",
                "target_voice",
                "pairs_v1_v8"
            ])
        );
        // The frozen descriptor must name every CueBundle
        // field — a code field missing here is freeze drift.
        let props = descriptor["properties"]
            .as_object()
            .expect("descriptor properties");
        for field in [
            "schema_version",
            "bundle_id",
            "producer",
            "cues",
            "pairs",
            "shared_releases",
            "assets",
            "provenance",
        ] {
            assert!(props.contains_key(field), "descriptor missing {field}");
        }
    }
}
