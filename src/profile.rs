//! Launch-time tool profiles (D-934).
//!
//! A profile is deterministic launch configuration: it scopes
//! which of the catalog a launch serves, so a client's tools/list
//! handshake carries only the surface its task needs. The default
//! profile (`core`) serves the standing operator surface — server
//! status plus the catalogue lane; named packs add one family
//! each and combine freely; the `full` profile serves the
//! complete catalog unfiltered.
//!
//! The scoping grain is the descriptor's own lane label: every
//! [`crate::server::ToolSpec`] carries a `group` (the trigger-
//! scoped loading group of brief §3), and packs are sets of
//! groups. Filtering happens at composition time — the server is
//! built with a profile and serves only that profile's specs, so
//! a tool outside the profile is unknown to the process (a
//! protocol-level `unknown tool` refusal), not merely unlisted.
//! The `full` profile applies no filter, so its served surface is
//! the complete registry by construction.
//!
//! Profiles decide presence only. Domain gates inside the tools
//! (licence states, ear verdicts, lane configuration) are
//! unchanged, and no tool is reachable *only* through a profile:
//! the full surface is always one explicit `full` launch away.
//! Skill or trigger layers may recommend a pack, but they are
//! advisory — this module is the deterministic gate they
//! recommend into.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::error::{TororiError, TororiResult};

/// Group labels every profile serves: server status plus the
/// catalogue lane — the standing operator surface (catalogue
/// search/get/stats and the governed status transition).
pub const CORE_GROUPS: &[&str] = &["server", "catalogue"];

/// The profile keyword that serves only [`CORE_GROUPS`].
pub const PROFILE_CORE: &str = "core";

/// The profile keyword that serves the complete catalog.
pub const PROFILE_FULL: &str = "full";

/// Environment variable consulted between the `--profile`
/// argument and the config file's `tool_profile` field.
pub const PROFILE_ENV_VAR: &str = "TORORI_TOOL_PROFILE";

/// Environment variable naming the config file, consulted
/// between the `--config` argument and the default config path.
pub const CONFIG_ENV_VAR: &str = "TORORI_CONFIG";

/// The typed startup error code for profile/config failures.
const CONFIGURATION_INVALID: &str = "configuration_invalid";

fn invalid(message: impl Into<String>) -> TororiError {
    TororiError::new(CONFIGURATION_INVALID, message)
}

/// One named pack: the group labels it adds beyond
/// [`CORE_GROUPS`]. Packs are disjoint by group (asserted in this
/// module's tests), so a group belongs to exactly one pack and
/// the union of all packs plus core is the catalog.
struct Pack {
    name: &'static str,
    groups: &'static [&'static str],
}

/// The pack table, alphabetical by pack name, named after
/// torori's real surface families (brief §1/§3): the OpenNSFW
/// library lane, GAP5 pair validation + cue-bundle ingest, the
/// Qwen3-TTS voice lane, the deterministic DSP SFX lane, and the
/// Route C comp lane. A new group joins exactly one pack here in
/// the same change that registers it; the server tests fail
/// until it does.
const PACKS: &[Pack] = &[
    Pack {
        name: "bundles",
        groups: &["bundles", "pairs"],
    },
    Pack {
        name: "comp",
        groups: &["comp"],
    },
    Pack {
        name: "library",
        groups: &["library"],
    },
    Pack {
        name: "sfx",
        groups: &["sfx"],
    },
    Pack {
        name: "voice",
        groups: &["voice"],
    },
];

/// A resolved launch profile: the full catalog, or core plus a
/// set of packs. Construct via [`ToolProfile::parse`],
/// [`ToolProfile::resolve`], or [`Default`]; the resolved form
/// cannot name an unknown pack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolProfile {
    full: bool,
    packs: BTreeSet<&'static str>,
}

impl Default for ToolProfile {
    /// The default launch profile: `core`.
    fn default() -> Self {
        ToolProfile {
            full: false,
            packs: BTreeSet::new(),
        }
    }
}

impl ToolProfile {
    /// The full profile: the complete catalog, no filter.
    pub fn full() -> ToolProfile {
        ToolProfile {
            full: true,
            packs: BTreeSet::new(),
        }
    }

    /// Parse a profile spec: `core`, `full`, or a comma-separated
    /// list of pack names (optionally including `core`, which is
    /// always served anyway). Matching is exact and lowercase.
    /// Fail-closed: an empty spec, an empty segment, an unknown
    /// pack, or `full` combined with anything else is a
    /// `configuration_invalid` error naming the valid choices.
    pub fn parse(spec: &str) -> TororiResult<ToolProfile> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Err(invalid(
                "tool profile spec is empty (name core, full, or a pack)",
            ));
        }
        let mut profile = ToolProfile::default();
        let mut saw_full = false;
        let mut segments = 0usize;
        for segment in trimmed.split(',') {
            segments += 1;
            let name = segment.trim();
            if name.is_empty() {
                return Err(invalid(format!(
                    "tool profile spec '{spec}' contains an empty segment"
                )));
            }
            match name {
                PROFILE_FULL => saw_full = true,
                PROFILE_CORE => {}
                _ => {
                    let pack = PACKS.iter().find(|pack| pack.name == name).ok_or_else(|| {
                        invalid(format!(
                            "tool profile names unknown pack '{name}' (valid: core, full, {})",
                            Self::pack_names().join(", ")
                        ))
                    })?;
                    profile.packs.insert(pack.name);
                }
            }
        }
        if saw_full {
            if segments > 1 {
                return Err(invalid(
                    "tool profile 'full' stands alone; combine packs instead (e.g. bundles,voice)",
                ));
            }
            return Ok(ToolProfile::full());
        }
        Ok(profile)
    }

    /// Resolve the launch profile by precedence (D-934): the
    /// `--profile` argument spec wins over the
    /// `TORORI_TOOL_PROFILE` environment spec, which wins over
    /// the config file's `tool_profile` spec; with none present
    /// the default is `core`. A present spec is always parsed —
    /// a malformed one fails closed rather than falling through
    /// to a lower-precedence source.
    pub fn resolve(
        arg_spec: Option<&str>,
        env_spec: Option<&str>,
        config_spec: Option<&str>,
    ) -> TororiResult<ToolProfile> {
        match arg_spec.or(env_spec).or(config_spec) {
            Some(spec) => Self::parse(spec),
            None => Ok(ToolProfile::default()),
        }
    }

    /// Whether this profile serves the complete catalog.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Whether a tool with this group label is served.
    pub fn serves_group(&self, group: &str) -> bool {
        if self.full || CORE_GROUPS.contains(&group) {
            return true;
        }
        self.packs
            .iter()
            .filter_map(|name| Self::pack(name))
            .any(|pack| pack.groups.contains(&group))
    }

    /// The canonical spec string (`core`, `core+bundles,voice`,
    /// `full`): what `torori_status` reports.
    pub fn spec_string(&self) -> String {
        if self.full {
            return PROFILE_FULL.to_string();
        }
        if self.packs.is_empty() {
            return PROFILE_CORE.to_string();
        }
        let joined: Vec<&str> = self.packs.iter().copied().collect();
        format!("core+{}", joined.join(","))
    }

    /// Every pack name, alphabetical (the table's own order).
    pub fn pack_names() -> Vec<&'static str> {
        PACKS.iter().map(|pack| pack.name).collect()
    }

    /// The group labels one pack adds, if the pack exists.
    pub fn pack_groups(name: &str) -> Option<&'static [&'static str]> {
        Self::pack(name).map(|pack| pack.groups)
    }

    fn pack(name: &str) -> Option<&'static Pack> {
        PACKS.iter().find(|pack| pack.name == name)
    }
}

/// The launch config file's interpreted contents. The file is a
/// TOML-shaped assignment list; `tool_profile` is the only key
/// this server reads. Other keys are ignored (a shared file may
/// carry settings for other tools), but every non-blank,
/// non-comment line must be a `key = value` assignment — a
/// malformed file fails closed rather than being half-read.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct LaunchConfig {
    pub tool_profile: Option<String>,
}

/// Parse config file text (see [`LaunchConfig`]). The
/// `tool_profile` value must be a quoted string; a duplicate or
/// malformed `tool_profile` line is a `configuration_invalid`
/// error.
pub fn parse_config(text: &str) -> TororiResult<LaunchConfig> {
    let mut config = LaunchConfig::default();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid(format!(
                "config line {} is not a key = value assignment: '{line}'",
                index + 1
            )));
        };
        if key.trim() != "tool_profile" {
            continue;
        }
        if config.tool_profile.is_some() {
            return Err(invalid("config names tool_profile twice"));
        }
        let value = value.trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .ok_or_else(|| {
                invalid(format!(
                    "config tool_profile value must be a quoted string, got '{value}'"
                ))
            })?;
        config.tool_profile = Some(unquoted.to_string());
    }
    Ok(config)
}

/// Load and parse the config file at `path`. A named file that
/// cannot be read is a `configuration_invalid` error — an
/// explicit config that fails to load never silently degrades
/// to defaults.
pub fn load_config(path: &std::path::Path) -> TororiResult<LaunchConfig> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| invalid(format!("cannot read config file {}: {e}", path.display())))?;
    parse_config(&text)
}

/// The default config path: `$XDG_CONFIG_HOME/torori/torori.toml`,
/// or `$HOME/.config/torori/torori.toml`. Consulted only when it
/// exists and no `--config`/`TORORI_CONFIG` names another file.
pub fn default_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("torori").join("torori.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_default_is_core_only() {
        let profile = ToolProfile::default();
        assert!(!profile.is_full());
        assert!(profile.serves_group("server"));
        assert!(profile.serves_group("catalogue"));
        assert!(!profile.serves_group("library"));
        assert!(!profile.serves_group("voice"));
        assert_eq!(profile.spec_string(), "core");
    }

    #[test]
    fn validation_full_serves_every_group() {
        let profile = ToolProfile::parse("full").unwrap();
        assert!(profile.is_full());
        assert!(profile.serves_group("voice"));
        assert!(profile.serves_group("anything-at-all"));
        assert_eq!(profile.spec_string(), "full");
    }

    #[test]
    fn validation_packs_combine_dedupe_and_keep_core() {
        let profile = ToolProfile::parse(" voice, core ,bundles,voice ").unwrap();
        assert!(profile.serves_group("server"));
        assert!(profile.serves_group("voice"));
        assert!(profile.serves_group("bundles"));
        assert!(profile.serves_group("pairs"));
        assert!(!profile.serves_group("sfx"));
        assert_eq!(profile.spec_string(), "core+bundles,voice");
    }

    #[test]
    fn validation_resolve_precedence_arg_over_env_over_config() {
        let profile = ToolProfile::resolve(Some("sfx"), Some("voice"), Some("library")).unwrap();
        assert_eq!(profile.spec_string(), "core+sfx");
        let profile = ToolProfile::resolve(None, Some("voice"), Some("library")).unwrap();
        assert_eq!(profile.spec_string(), "core+voice");
        let profile = ToolProfile::resolve(None, None, Some("library")).unwrap();
        assert_eq!(profile.spec_string(), "core+library");
        let profile = ToolProfile::resolve(None, None, None).unwrap();
        assert_eq!(profile.spec_string(), "core");
    }

    #[test]
    fn validation_config_reads_tool_profile_and_ignores_other_keys() {
        let config =
            parse_config("# torori launch config\nother = \"x\"\ntool_profile = \"voice\"\n")
                .unwrap();
        assert_eq!(config.tool_profile.as_deref(), Some("voice"));
        let config = parse_config("other = \"x\"\n").unwrap();
        assert_eq!(config.tool_profile, None);
    }

    #[test]
    fn adversarial_unknown_pack_is_a_typed_error() {
        let err = ToolProfile::parse("voic").unwrap_err();
        assert_eq!(err.code, "configuration_invalid");
        assert!(
            err.message.contains("unknown pack 'voic'"),
            "{}",
            err.message
        );
        assert!(err.message.contains("library"), "{}", err.message);
    }

    #[test]
    fn adversarial_full_combined_is_an_error() {
        let err = ToolProfile::parse("full,voice").unwrap_err();
        assert_eq!(err.code, "configuration_invalid");
    }

    #[test]
    fn adversarial_empty_and_blank_segments_are_errors() {
        for spec in ["", "   ", "voice,,sfx", "voice,"] {
            let err = ToolProfile::parse(spec).unwrap_err();
            assert_eq!(err.code, "configuration_invalid", "spec: {spec}");
        }
    }

    #[test]
    fn adversarial_pack_names_are_case_exact() {
        assert!(ToolProfile::parse("FULL").is_err());
        assert!(ToolProfile::parse("Voice").is_err());
    }

    #[test]
    fn adversarial_present_but_invalid_spec_never_falls_through() {
        // An invalid argument spec must not degrade to the valid
        // env/config specs below it in precedence.
        assert!(ToolProfile::resolve(Some("nope"), Some("voice"), None).is_err());
        assert!(ToolProfile::resolve(None, Some(""), Some("voice")).is_err());
    }

    #[test]
    fn adversarial_config_malformed_tool_profile_is_an_error() {
        for text in [
            "tool_profile = voice\n",
            "tool_profile = \"voice\"\ntool_profile = \"sfx\"\n",
            "tool_profile \"voice\"\n",
        ] {
            let err = parse_config(text).unwrap_err();
            assert_eq!(err.code, "configuration_invalid", "text: {text}");
        }
    }

    #[test]
    fn validation_pack_table_is_alphabetical_disjoint_and_core_free() {
        let names = ToolProfile::pack_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "pack table stays alphabetical");
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for name in &names {
            for group in ToolProfile::pack_groups(name).unwrap() {
                assert!(
                    !CORE_GROUPS.contains(group),
                    "pack {name} re-lists core group {group}"
                );
                assert!(seen.insert(group), "group {group} appears in two packs");
            }
        }
    }
}
