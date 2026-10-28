//! torori — the NTRanch estate sound MCP server.
//!
//! One domain: adult-lane audio assets and their production
//! discipline. Four functions (build brief §1): catalogue serving
//! over the 114-entry TORORI catalogue, the OpenNSFW library lane
//! under its licence-state decide(), GAP5 pair validation +
//! file-bundle cue ingest (neemu's hand-off — files only, never a
//! server call), and the generation lanes (Qwen3-TTS voice,
//! deterministic DSP SFX, Route C comp).
//!
//! Standing law, enforced in code (brief §2/§5): the AUDIO
//! SILENCE RULE is absolute — this codebase contains no audio
//! output path of any kind; every output is a file plus metrics.
//! Rights law (no real-person likeness, the OpenNSFW conjunction
//! ban, per-file licence states) is code, not documentation.

pub mod audio;
pub mod bundles;
pub mod catalogue;
pub mod comp;
pub mod conditioning;
pub mod error;
pub mod guard;
pub mod library;
pub mod manifest;
pub mod pairs;
pub mod qc;
pub mod server;
pub mod sfx;
pub mod state;
pub mod util;
pub mod voice;

/// The server's runtime state: filesystem roots plus the loaded
/// stores. Held behind a mutex by the server (calls are
/// dispatched on blocking threads; the surface is operator-scale,
/// not a hot path).
pub struct TororiState {
    pub paths: state::Paths,
    pub catalogue: catalogue::Catalogue,
    pub library: library::LibraryStore,
    pub bundles: bundles::BundleStore,
    pub voices: conditioning::VoiceRegistry,
}

impl TororiState {
    /// Load every store from disk. Missing optional stores load
    /// as empty (first run); a present-but-invalid catalogue data
    /// file is a hard error — load-time validation never serves
    /// drifted data silently (brief §4).
    pub fn load(paths: state::Paths) -> error::TororiResult<TororiState> {
        paths.ensure_state_dirs()?;
        let mut catalogue = catalogue::Catalogue::load(&paths)?;
        catalogue.load_overlay(&paths)?;
        let library = library::LibraryStore::load(&paths)?;
        let bundles = bundles::BundleStore::load(&paths)?;
        // GAP5 §6.1 lint (batch 1): PLANNED entries naming pairs
        // must name Armed-grade pairs. Hard error, never served.
        catalogue::lint_named_pairs(&catalogue, &bundles.program_docs)?;
        let voices = conditioning::VoiceRegistry::load(&paths)?;
        Ok(TororiState {
            paths,
            catalogue,
            library,
            bundles,
            voices,
        })
    }

    /// `torori_status` payload: server-computed counts and lane
    /// posture. Counts are computed here, never by a caller.
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "name": "torori",
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": guard::MODERN_VERSION,
            "counts": {
                "catalogue_entries": self.catalogue.entries.len(),
                "library_rows": self.library.rows.len(),
                "bundles_ingested": self.bundles.ingested.len(),
                "pairs": self.bundles.pair_count(),
                "voice_identities": self.voices.identities.len(),
            },
            "lanes": {
                "qwen_tts": voice::lane_state(),
                "dsp_sfx": "available",
                "route_c_comp": "available",
            },
            "silence_rule": "enforced: no playback path exists in this codebase; outputs are files + metrics only",
        })
    }
}
