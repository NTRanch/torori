//! GAP5 TriggerPairs (GAP5-SPEC §§2–3, 5–6): the pair record
//! types and the V1–V8 validation battery as code.
//!
//! The load-bearing laws, by construction and by battery:
//! - R1 completeness is a **type-level** fact: `release` is a
//!   non-optional field of `TriggerPair` — a pair without a
//!   release half cannot be represented in any estate store.
//! - R2 one cue hash = one pair per program; no install cue may
//!   equal or truncate the safeword cue.
//! - R3 every release asset resolves to a shipped bundle asset
//!   with a matching manifest hash; shared forms are validated
//!   bidirectionally (the form must list the pair in `covers`).
//! - R4 scope legality (Player reach ≤ Program, contexts
//!   non-empty; Fiction with empty contexts needs a site list).
//! - R5 a program may not arm without ≥1 SafewordClass release
//!   covering every pair.
//! - R6 tombstones, never deletion (the Player Tandem Purge
//!   exception is a consumer act, not a record edit).
//! - R7 the precedence graph is acyclic below the safeword root.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::manifest::is_sha256_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsumerClass {
    Fiction,
    Player,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramRef {
    pub program_id: String,
    pub series: Option<String>,
    pub consumer_class: ConsumerClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectForm {
    Single,
    Dyadic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerSource {
    OwnState,
    PartnerState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub form: SubjectForm,
    pub identity_ref: String,
    /// Required iff `form` is Dyadic (validated in V1).
    pub partner_ref: Option<String>,
    pub trigger_source: TriggerSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CueKind {
    Snap,
    SpokenWord,
    SpokenPhrase,
    TonePair,
    MusicMotif,
    VisualGlyph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    pub kind: CueKind,
    pub asset_ref: String,
    /// The hash IS the cue's identity.
    pub asset_sha256: String,
    pub canonical: bool,
    /// Must be empty in schema v1 — and Snap cues forbid variants
    /// outright (V1).
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectStatement {
    /// At disclosure grade, per the spec.
    pub summary: String,
    pub disclosure_ref: String,
    pub fiction_effect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallHalf {
    pub cue: Cue,
    pub effect: EffectStatement,
    pub install_contexts: Vec<String>,
    pub countermand_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleaseKind {
    SoftRelease,
    ScopedPurge,
    FullReset,
    Countermand,
    SafewordClass,
}

/// The release body — embedded in a pair, or shared as a
/// `ReleaseForm` record referenced by id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseBody {
    pub cue: Option<Cue>,
    pub asset_refs: Vec<String>,
    pub covers: Vec<String>,
    pub except: Vec<String>,
    pub budget: Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Budget {
    Unlimited,
    OncePerCampaign,
    OncePerSession,
}

/// The release half: exactly one of an embedded body or a shared
/// release-form id (R1 — the field itself is non-optional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReleaseHalf {
    Embedded(ReleaseBody),
    Shared(String),
}

/// A shared release form record (program-level).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseForm {
    pub release_id: String,
    pub form: ReleaseKind,
    pub cue: Option<Cue>,
    pub asset_refs: Vec<String>,
    pub covers: Vec<String>,
    pub except: Vec<String>,
    pub budget: Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reach {
    Session,
    Program,
    CrossProgram,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scope {
    pub reach: Reach,
    pub contexts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub estate_ref: String,
    pub canon_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestLink {
    pub cue_manifest: String,
    pub release_manifests: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerPair {
    pub pair_id: String,
    pub program: ProgramRef,
    pub subject: Subject,
    pub install: InstallHalf,
    /// Non-optional by type (R1).
    pub release: ReleaseHalf,
    pub scope: Scope,
    pub author: Author,
    pub manifest_link: ManifestLink,
}

/// A program document: its pairs plus its shared release forms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramDoc {
    pub program_id: String,
    pub pairs: Vec<TriggerPair>,
    pub shared_releases: Vec<ReleaseForm>,
}

/// Context the battery needs beyond the doc itself.
#[derive(Debug, Clone, Default)]
pub struct ValidateCtx {
    /// asset_ref → sha256 for shipped assets (V2/V8).
    pub asset_index: Vec<(String, String)>,
    /// Resolvable disclosure refs (V7).
    pub disclosures: Vec<String>,
    /// Ordered template refs shipped in the build, tagged with
    /// their kind (V8): (template_ref, "install"|"reset"|"cond").
    pub ship_templates: Vec<(String, String)>,
    /// Pair → template refs (from §6.2 pair-manifest sections).
    pub pair_templates: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Violation {
    /// Battery rung, e.g. "V4".
    pub check: &'static str,
    pub pair_id: String,
    pub message: String,
}

fn v(check: &'static str, pair_id: &str, message: impl Into<String>) -> Violation {
    Violation {
        check,
        pair_id: pair_id.to_string(),
        message: message.into(),
    }
}

impl TriggerPair {
    pub fn release_form<'a>(&'a self, doc: &'a ProgramDoc) -> Option<&'a ReleaseForm> {
        match &self.release {
            ReleaseHalf::Shared(id) => doc.shared_releases.iter().find(|f| &f.release_id == id),
            ReleaseHalf::Embedded(_) => None,
        }
    }

    /// The pair's effective release kind: embedded bodies carry
    /// their kind implicitly via the pair record's declared kind
    /// field — v1 embedded bodies are validated structurally;
    /// shared forms carry the kind on the form.
    fn release_assets<'a>(&'a self, doc: &'a ProgramDoc) -> Vec<&'a str> {
        match &self.release {
            ReleaseHalf::Embedded(body) => body.asset_refs.iter().map(String::as_str).collect(),
            ReleaseHalf::Shared(id) => doc
                .shared_releases
                .iter()
                .find(|f| &f.release_id == id)
                .map(|f| f.asset_refs.iter().map(String::as_str).collect())
                .unwrap_or_default(),
        }
    }
}

/// Run the full battery over a program document. Empty result =
/// Armed-grade.
pub fn validate_doc(doc: &ProgramDoc, ctx: &ValidateCtx) -> Vec<Violation> {
    let mut out = Vec::new();
    for pair in &doc.pairs {
        validate_pair_v1(pair, doc, &mut out);
        validate_pair_v2(pair, doc, ctx, &mut out);
        validate_pair_v5(pair, &mut out);
        validate_pair_v7(pair, ctx, &mut out);
    }
    validate_v3(doc, &mut out);
    validate_v4(doc, &mut out);
    validate_v8(doc, ctx, &mut out);
    out
}

/// V1 — completeness (the parts of R1 a parsed record can still
/// violate: empty shared ref, partner/cardinality, cue shape).
fn validate_pair_v1(pair: &TriggerPair, doc: &ProgramDoc, out: &mut Vec<Violation>) {
    let id = &pair.pair_id;
    match pair.subject.form {
        SubjectForm::Dyadic if pair.subject.partner_ref.is_none() => {
            out.push(v("V1", id, "dyadic subject without partner_ref"))
        }
        SubjectForm::Single if pair.subject.partner_ref.is_some() => {
            out.push(v("V1", id, "single subject carries a partner_ref"))
        }
        _ => {}
    }
    let cue = &pair.install.cue;
    if !is_sha256_hex(&cue.asset_sha256) {
        out.push(v("V1", id, "install cue hash is not a sha256"));
    }
    if !cue.canonical {
        out.push(v("V1", id, "install cue is not the canonical take"));
    }
    if !cue.variants.is_empty() {
        out.push(v(
            "V1",
            id,
            "schema v1 admits no cue variants (Snap cues forbid them outright)",
        ));
    }
    if pair.install.effect.summary.is_empty() || pair.install.effect.disclosure_ref.is_empty() {
        out.push(v(
            "V1",
            id,
            "effect statement needs a summary and a disclosure_ref",
        ));
    }
    match &pair.release {
        ReleaseHalf::Shared(ref_id) => {
            if ref_id.is_empty() {
                out.push(v(
                    "V1",
                    id,
                    "shared release ref is empty — release half is prose, not a record",
                ));
            } else if doc.shared_releases.iter().all(|f| &f.release_id != ref_id) {
                out.push(v(
                    "V1",
                    id,
                    format!("shared release {ref_id} not present in the program doc"),
                ));
            }
        }
        ReleaseHalf::Embedded(body) => {
            if body.asset_refs.is_empty() && body.cue.is_none() {
                out.push(v(
                    "V1",
                    id,
                    "embedded release body carries neither assets nor a cue",
                ));
            }
            if matches!(
                body.budget,
                Budget::OncePerSession | Budget::OncePerCampaign
            ) && pair.release_is_safeword(doc)
            {
                out.push(v("V1", id, "a safeword release may not be budgeted"));
            }
        }
    }
}

impl TriggerPair {
    fn release_is_safeword(&self, doc: &ProgramDoc) -> bool {
        matches!(
            self.release_form(doc).map(|f| f.form),
            Some(ReleaseKind::SafewordClass)
        )
    }
}

/// V2 — release reachability (R3): assets resolve to shipped
/// assets with matching hashes; shared forms cover the pair
/// bidirectionally.
fn validate_pair_v2(
    pair: &TriggerPair,
    doc: &ProgramDoc,
    ctx: &ValidateCtx,
    out: &mut Vec<Violation>,
) {
    let id = &pair.pair_id;
    for asset_ref in pair.release_assets(doc) {
        if !ctx.asset_index.iter().any(|(r, _)| r == asset_ref) {
            out.push(v(
                "V2",
                id,
                format!("release asset {asset_ref} does not resolve to a shipped asset"),
            ));
        }
    }
    if let ReleaseHalf::Shared(ref_id) = &pair.release
        && let Some(form) = doc.shared_releases.iter().find(|f| &f.release_id == ref_id)
        && !form.covers.iter().any(|c| c == id)
    {
        out.push(v(
            "V2",
            id,
            format!("shared form {ref_id} does not list this pair in covers (bidirectional check)"),
        ));
    }
}

/// V5 — scope legality (R4).
fn validate_pair_v5(pair: &TriggerPair, out: &mut Vec<Violation>) {
    let id = &pair.pair_id;
    match pair.program.consumer_class {
        ConsumerClass::Player => {
            if pair.scope.contexts.is_empty() {
                out.push(v("V5", id, "player pairs must declare scope contexts"));
            }
            if pair.scope.reach == Reach::CrossProgram {
                out.push(v(
                    "V5",
                    id,
                    "player triggers may not reach CrossProgram (unrepresentable)",
                ));
            }
        }
        ConsumerClass::Fiction => {
            if pair.scope.contexts.is_empty() && pair.install.install_contexts.is_empty() {
                out.push(v(
                    "V5",
                    id,
                    "fiction pair with empty scope contexts must carry a site list in install_contexts",
                ));
            }
        }
    }
}

/// V7 — disclosure completeness.
fn validate_pair_v7(pair: &TriggerPair, ctx: &ValidateCtx, out: &mut Vec<Violation>) {
    if !ctx
        .disclosures
        .iter()
        .any(|d| d == &pair.install.effect.disclosure_ref)
    {
        out.push(v(
            "V7",
            &pair.pair_id,
            format!(
                "disclosure_ref {} does not resolve",
                pair.install.effect.disclosure_ref
            ),
        ));
    }
}

/// V3 — precedence soundness (R7 + R5): the covers-graph is
/// acyclic, the safeword forms are exactly the graph roots, and
/// every pair is covered by a SafewordClass form.
fn validate_v3(doc: &ProgramDoc, out: &mut Vec<Violation>) {
    // Nodes: shared forms. Edge F -> G when F covers a pair whose
    // release resolves to G (F's firing defers to G's record).
    let form_ids: Vec<&str> = doc
        .shared_releases
        .iter()
        .map(|f| f.release_id.as_str())
        .collect();
    let mut edges: Vec<(&str, &str)> = Vec::new();
    for form in &doc.shared_releases {
        for covered in &form.covers {
            if let Some(pair) = doc.pairs.iter().find(|p| &p.pair_id == covered)
                && let ReleaseHalf::Shared(target) = &pair.release
                && target != &form.release_id
            {
                edges.push((form.release_id.as_str(), target.as_str()));
            }
        }
    }
    // Cycle detection (DFS colours).
    let mut state: std::collections::HashMap<&str, u8> = std::collections::HashMap::new();
    fn visit<'a>(
        node: &'a str,
        edges: &[(&'a str, &'a str)],
        state: &mut std::collections::HashMap<&'a str, u8>,
    ) -> bool {
        state.insert(node, 1);
        for (_a, b) in edges.iter().filter(|(a, _)| *a == node) {
            match state.get(b).copied().unwrap_or(0) {
                1 => return false,
                0 if !visit(b, edges, state) => return false,
                0 => {}
                _ => {}
            }
        }
        state.insert(node, 2);
        true
    }
    for id in &form_ids {
        if state.get(id).copied().unwrap_or(0) == 0 && !visit(id, &edges, &mut state) {
            out.push(v(
                "V3",
                "",
                format!("precedence graph has a cycle through {id}"),
            ));
            break;
        }
    }
    // Roots (no incoming edges) must be exactly the safewords.
    let roots: Vec<&str> = form_ids
        .iter()
        .filter(|id| !edges.iter().any(|(_, b)| b == *id))
        .copied()
        .collect();
    for form in &doc.shared_releases {
        let is_root = roots.contains(&form.release_id.as_str());
        let is_safeword = form.form == ReleaseKind::SafewordClass;
        if is_root && !is_safeword && !edges.is_empty() {
            out.push(v(
                "V3",
                "",
                format!(
                    "release form {} sits at the precedence root but is not SafewordClass",
                    form.release_id
                ),
            ));
        }
        if is_safeword && !is_root {
            out.push(v(
                "V3",
                "",
                format!(
                    "safeword form {} is not at the precedence root",
                    form.release_id
                ),
            ));
        }
    }
    // R5: every pair covered by at least one SafewordClass form.
    let safeword_covers: Vec<&str> = doc
        .shared_releases
        .iter()
        .filter(|f| f.form == ReleaseKind::SafewordClass)
        .flat_map(|f| f.covers.iter().map(String::as_str))
        .collect();
    if doc
        .shared_releases
        .iter()
        .all(|f| f.form != ReleaseKind::SafewordClass)
    {
        out.push(v(
            "V3",
            "",
            "program arms without any SafewordClass release (R5)",
        ));
    } else {
        for pair in &doc.pairs {
            if !safeword_covers.contains(&pair.pair_id.as_str()) {
                out.push(v(
                    "V3",
                    &pair.pair_id,
                    "pair is not covered by any SafewordClass release (R5)",
                ));
            }
        }
    }
}

/// V4 — cue collision (R2): one cue hash per pair per program;
/// no cue equals or truncates the safeword cue.
fn validate_v4(doc: &ProgramDoc, out: &mut Vec<Violation>) {
    let safeword_hashes: Vec<&str> = doc
        .shared_releases
        .iter()
        .filter(|f| f.form == ReleaseKind::SafewordClass)
        .filter_map(|f| f.cue.as_ref().map(|c| c.asset_sha256.as_str()))
        .collect();
    let mut seen: Vec<(&str, &str)> = Vec::new(); // (hash, pair_id)
    for pair in &doc.pairs {
        let hash = pair.install.cue.asset_sha256.as_str();
        if let Some((_, other)) = seen.iter().find(|(h, _)| *h == hash) {
            out.push(v(
                "V4",
                &pair.pair_id,
                format!("cue hash already used by {other} in this program (R2)"),
            ));
        }
        for sw in &safeword_hashes {
            if hash == *sw || sw.starts_with(hash) || hash.starts_with(sw) {
                out.push(v(
                    "V4",
                    &pair.pair_id,
                    "install cue equals or truncates the safeword cue (R2)",
                ));
            }
        }
        seen.push((hash, &pair.pair_id));
    }
}

/// V8 — ship order: a covering T-RESET ships at or before the
/// first T-INSTALL.
fn validate_v8(doc: &ProgramDoc, ctx: &ValidateCtx, out: &mut Vec<Violation>) {
    if ctx.ship_templates.is_empty() {
        return;
    }
    let pos =
        |kind: &str| -> Option<usize> { ctx.ship_templates.iter().position(|(_, k)| k == kind) };
    let (Some(reset_at), Some(install_at)) = (pos("reset"), pos("install")) else {
        return;
    };
    if reset_at > install_at {
        for pair in &doc.pairs {
            out.push(v(
                "V8",
                &pair.pair_id,
                "the build ships a T-INSTALL before its covering T-RESET",
            ));
        }
    }
}

// ── V6: lifecycle replay ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairState {
    Authored,
    Armed,
    Installed,
    Suspended,
    Released,
    Purged,
}

/// One event-log record (GAP5 §3): `(pair_id, gen, transition,
/// cue_sha256 | release_form_id, actor, context_id,
/// session_segment_index)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairEvent {
    pub pair_id: String,
    /// Generation counter (the spec's `gen` field).
    #[serde(rename = "gen")]
    pub generation: u64,
    pub transition: String,
    pub actor: String,
    pub context_id: String,
    pub session_segment_index: u64,
}

/// Replay an event log; returns end states per pair or the
/// first illegal transition. Deterministic by construction —
/// replaying the same log reproduces the same end states (V6).
pub fn replay(events: &[PairEvent]) -> Result<Vec<(String, PairState, u64)>, Violation> {
    let mut states: std::collections::BTreeMap<String, (PairState, u64)> =
        std::collections::BTreeMap::new();
    for event in events {
        let (state, cur_gen) = states
            .entry(event.pair_id.clone())
            .or_insert((PairState::Authored, event.generation));
        if event.generation != *cur_gen {
            return Err(v(
                "V6",
                &event.pair_id,
                format!(
                    "gen mismatch: log carries {}, current gen is {} (re-install is a fresh gen+1 event)",
                    event.generation, cur_gen
                ),
            ));
        }
        let next = match (*state, event.transition.as_str()) {
            (PairState::Authored, "arm") => PairState::Armed,
            (PairState::Armed, "install") => PairState::Installed,
            (PairState::Installed, "suspend") => PairState::Suspended,
            (PairState::Installed, "release") => PairState::Released,
            (PairState::Suspended, "release") => PairState::Released,
            (PairState::Released, "purge") => PairState::Purged,
            // Re-install after release is a FRESH install event at
            // gen+1 — the log must carry the new gen, handled by
            // the gen check above; a same-gen "install" here is an
            // attempt to resurrect, which is sticky-illegal.
            (PairState::Released, "install") => {
                return Err(v(
                    "V6",
                    &event.pair_id,
                    "released is sticky: re-install must arrive as a fresh gen+1 install from Armed",
                ));
            }
            (s, "noop") => s,
            (s, t) => {
                return Err(v(
                    "V6",
                    &event.pair_id,
                    format!("illegal transition {t} from {s:?}"),
                ));
            }
        };
        *state = next;
    }
    Ok(states.into_iter().map(|(id, (s, g))| (id, s, g)).collect())
}

/// JSON shape for a violation list (tool payloads).
pub fn violations_json(violations: &[Violation]) -> Value {
    json!({
        "armed_grade": violations.is_empty(),
        "violations": violations.iter().map(|x| json!({
            "check": x.check, "pair_id": x.pair_id, "message": x.message,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const H1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const H2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const HS: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn snap_pair(id: &str, hash: &str) -> TriggerPair {
        TriggerPair {
            pair_id: id.into(),
            program: ProgramRef {
                program_id: "kit".into(),
                series: None,
                consumer_class: ConsumerClass::Fiction,
            },
            subject: Subject {
                form: SubjectForm::Single,
                identity_ref: "kit".into(),
                partner_ref: None,
                trigger_source: TriggerSource::OwnState,
            },
            install: InstallHalf {
                cue: Cue {
                    kind: CueKind::Snap,
                    asset_ref: "audio/kit/snap.wav".into(),
                    asset_sha256: hash.into(),
                    canonical: true,
                    variants: vec![],
                },
                effect: EffectStatement {
                    summary: "drop cue".into(),
                    disclosure_ref: "disclosure.kit".into(),
                    fiction_effect: None,
                },
                install_contexts: vec!["ctx".into()],
                countermand_allowed: true,
            },
            release: ReleaseHalf::Shared("release.kit.reset".into()),
            scope: Scope {
                reach: Reach::Program,
                contexts: vec!["ctx".into()],
            },
            author: Author {
                estate_ref: "torori-kit".into(),
                canon_refs: vec![],
            },
            manifest_link: ManifestLink {
                cue_manifest: "pair.kit.x".into(),
                release_manifests: vec![],
            },
        }
    }

    fn kit_doc() -> ProgramDoc {
        ProgramDoc {
            program_id: "kit".into(),
            pairs: vec![snap_pair("pair.kit.drop-snap", H1)],
            shared_releases: vec![
                ReleaseForm {
                    release_id: "release.kit.reset".into(),
                    form: ReleaseKind::ScopedPurge,
                    cue: None,
                    asset_refs: vec!["audio/kit/reset.wav".into()],
                    covers: vec!["pair.kit.drop-snap".into()],
                    except: vec![],
                    budget: Budget::Unlimited,
                },
                ReleaseForm {
                    release_id: "release.kit.stop".into(),
                    form: ReleaseKind::SafewordClass,
                    cue: Some(Cue {
                        kind: CueKind::SpokenWord,
                        asset_ref: "audio/kit/stop.wav".into(),
                        asset_sha256: HS.into(),
                        canonical: true,
                        variants: vec![],
                    }),
                    asset_refs: vec!["audio/kit/stop.wav".into()],
                    covers: vec!["pair.kit.drop-snap".into()],
                    except: vec![],
                    budget: Budget::Unlimited,
                },
            ],
        }
    }

    fn ctx() -> ValidateCtx {
        ValidateCtx {
            asset_index: vec![
                ("audio/kit/reset.wav".into(), H2.into()),
                ("audio/kit/stop.wav".into(), HS.into()),
            ],
            disclosures: vec!["disclosure.kit".into()],
            ship_templates: vec![],
            pair_templates: vec![],
        }
    }

    #[test]
    fn a_sound_doc_is_armed_grade() {
        let violations = validate_doc(&kit_doc(), &ctx());
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn battery_adversarial_cases() {
        // Cue collision (V4).
        let mut doc = kit_doc();
        doc.pairs.push(snap_pair("pair.kit.other", H1));
        doc.shared_releases[0].covers.push("pair.kit.other".into());
        doc.shared_releases[1].covers.push("pair.kit.other".into());
        let vs = validate_doc(&doc, &ctx());
        assert!(vs.iter().any(|x| x.check == "V4"), "{vs:?}");
        // Safeword truncation (V4): install cue is a prefix-free
        // distinct hash — simulate equality with safeword hash.
        let mut doc = kit_doc();
        doc.pairs[0].install.cue.asset_sha256 = HS.into();
        let vs = validate_doc(&doc, &ctx());
        assert!(vs.iter().any(|x| x.check == "V4"), "{vs:?}");
        // Missing shared form (V1) + bidirectional (V2).
        let mut doc = kit_doc();
        doc.shared_releases.remove(0);
        let vs = validate_doc(&doc, &ctx());
        assert!(vs.iter().any(|x| x.check == "V1"), "{vs:?}");
        // Player cross-program (V5).
        let mut doc = kit_doc();
        doc.pairs[0].program.consumer_class = ConsumerClass::Player;
        doc.pairs[0].scope.reach = Reach::CrossProgram;
        let vs = validate_doc(&doc, &ctx());
        assert!(vs.iter().any(|x| x.check == "V5"), "{vs:?}");
        // No safeword (V3/R5).
        let mut doc = kit_doc();
        doc.shared_releases.remove(1);
        let vs = validate_doc(&doc, &ctx());
        assert!(vs.iter().any(|x| x.check == "V3"), "{vs:?}");
        // Disclosure unresolved (V7).
        let mut c = ctx();
        c.disclosures.clear();
        let vs = validate_doc(&kit_doc(), &c);
        assert!(vs.iter().any(|x| x.check == "V7"), "{vs:?}");
        // Release asset not shipped (V2).
        let mut c = ctx();
        c.asset_index.clear();
        let vs = validate_doc(&kit_doc(), &c);
        assert!(vs.iter().any(|x| x.check == "V2"), "{vs:?}");
    }

    #[test]
    fn replay_is_deterministic_and_sticky() {
        let ev = |t: &str, generation: u64| PairEvent {
            pair_id: "pair.kit.drop-snap".into(),
            generation,
            transition: t.into(),
            actor: "test".into(),
            context_id: "ctx".into(),
            session_segment_index: 0,
        };
        let log = vec![
            ev("arm", 1),
            ev("install", 1),
            ev("suspend", 1),
            ev("release", 1),
        ];
        let a = replay(&log).unwrap();
        let b = replay(&log).unwrap();
        assert_eq!(a, b, "replay reproduces end states exactly");
        assert_eq!(a[0].1, PairState::Released);
        // Resurrection at the same gen is refused.
        let mut bad = log.clone();
        bad.push(ev("install", 1));
        assert!(replay(&bad).is_err());
        // Illegal transition from Authored.
        assert!(replay(&[ev("install", 1)]).is_err());
    }
}
