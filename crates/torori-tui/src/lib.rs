//! torori-tui — the sound-catalogue console for the torori MCP.
//!
//! Fleet TUI pattern (programme D-1562, DESIGN §D2): a narrow
//! [`facade::TororiFacade`] trait, pure panels, a screen state
//! machine, a typed trigger engine (§D4), a full-screen ratatui
//! mode, and a non-TTY line mode sharing the same execute path.
//!
//! AUDIO SILENCE RULE: this crate never plays audio, never
//! decodes audio for playback, and never renders waveforms from
//! audio. Audio files are catalogue rows — text metadata — and
//! nothing more. Torori's catalogue references no image
//! artifacts (entries carry text asset descriptions only), so
//! there is deliberately no Media screen (§D5: absent, not
//! stubbed, where the member has no image use case).
#![forbid(unsafe_code)]

pub mod app;
pub mod facade;
pub mod panels;
pub mod screens;
pub mod triggers;

/// Maximum lines retained in the Log / Live buffer.
pub const LOG_LINES_MAX: usize = 512;
/// Maximum trigger audit lines retained in memory.
pub const AUDIT_LINES_MAX: usize = 512;
/// Maximum bytes accepted on one line-mode input line.
pub const INPUT_LINE_BYTES_MAX: usize = 4096;
/// Maximum catalogue rows requested in one search page (the
/// server's own page ceiling is 200; the TUI never asks for more).
pub const SEARCH_LIMIT_MAX: u64 = 200;
/// Maximum files counted in one cue bundle before ingest is
/// refused as oversized (bounded work; the count walk stops here).
pub const BUNDLE_FILES_MAX: usize = 4096;
/// Maximum bundle bytes walked for the pre-ingest size report.
pub const BUNDLE_BYTES_MAX: u64 = 4 * 1024 * 1024 * 1024;
/// Maximum voice takes staged in one voice-lane job (the server's
/// own ceiling for `torori_voice_generate`).
pub const VOICE_TAKES_MAX: u64 = 8;
/// Hard ceiling for trigger chain depth (§D4: default 1, ceiling 2).
pub const TRIGGER_DEPTH_HARD_MAX: u8 = 2;
