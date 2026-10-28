//! Route C comp lane (brief §7 T5, STUDY §4): deterministic
//! assembly of voice + SFX + library comping material, the §4
//! cleanup chain, and the §4.3 numeric ship gate.
//!
//! The gate is a hard gate: a NO-SHIP result returns the
//! untreated comp plus the failing numbers — never a silently
//! cleaned file. Provenance law is enforced in assembly:
//! pack-derived material is comping material only, and
//! generated material is never comped INTO a pack-derived
//! asset (that direction is laundering, and it refuses).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::audio::{self, WavData};
use crate::error::{TororiError, TororiResult};
use crate::library::LibraryStore;
use crate::manifest::{AssetManifest, DeterminismClass, FinishRecord, HonestyLabel};
use crate::qc;
use crate::state::Paths;
use crate::util::{arg_str, sha256_hex};

pub const RATE: u32 = 48_000;

/// The §4.3 ship-gate measurements (STUDY §4.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateNumbers {
    /// Hiss floor reduction vs untreated, dB (need ≥ 6, or the
    /// treated floor ≤ −60 dBFS absolute).
    pub floor_reduction_db: f64,
    pub treated_floor_dbfs: f64,
    /// Largest residual spectral peak over the treated floor in
    /// any 1/3-octave band, dB (need ≤ 6).
    pub max_residual_peak_db: f64,
    /// Envelope dynamic-range delta vs untreated, dB (±1.5).
    pub envelope_range_delta_db: f64,
    /// Crest-factor delta vs untreated, dB (±1).
    pub crest_delta_db: f64,
    /// F0 track correlation on voiced frames (need ≥ 0.95).
    pub f0_correlation: f64,
    /// Finished master loudness, LUFS (need −16 ± 0.5).
    pub master_lufs: f64,
    /// Finished master true peak, dBTP (need ≤ −1).
    pub master_true_peak_dbtp: f64,
    /// Loudest phonation-bearing frame in the removed signal,
    /// dBFS (need ≤ −40).
    pub removed_phonation_dbfs: f64,
    pub verdict: String,
    pub failing_rungs: Vec<String>,
}

// ── §4 cleanup chain ──────────────────────────────────────────

fn dc_remove(samples: &mut [f32]) {
    let mean = audio::dc_offset(samples) as f32;
    for s in samples.iter_mut() {
        *s -= mean;
    }
}

fn highpass_25hz(samples: &mut [f32], rate: u32) {
    // One-pole high-pass, fc = 25 Hz (STUDY §4.4 chain step).
    let rc = 1.0 / (2.0 * std::f64::consts::PI * 25.0);
    let dt = 1.0 / rate as f64;
    let a = (rc / (rc + dt)) as f32;
    let mut y = 0.0f32;
    let mut x_prev = 0.0f32;
    for s in samples.iter_mut() {
        let x = *s;
        y = a * (y + x - x_prev);
        x_prev = x;
        *s = y;
    }
}

/// Spectral-gate denoise: per-bin floor estimated from the
/// quietest frames; gain floor 0.05; deterministic throughout.
fn spectral_gate(samples: &[f32], rate: u32) -> Vec<f32> {
    let frame = 2048usize;
    let hop = 1024usize;
    if samples.len() < frame {
        return samples.to_vec();
    }
    let frames = (samples.len() - frame) / hop + 1;
    // Pass 1: spectra + frame energies.
    let mut mags: Vec<Vec<f64>> = Vec::with_capacity(frames);
    let mut phases: Vec<Vec<(f64, f64)>> = Vec::with_capacity(frames);
    let mut energies: Vec<f64> = Vec::with_capacity(frames);
    for f in 0..frames {
        let seg = &samples[f * hop..f * hop + frame];
        energies.push(audio::rms(seg));
        let n = frame;
        let mut re = vec![0.0f64; n];
        let mut im = vec![0.0f64; n];
        for (i, &s) in seg.iter().enumerate() {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
            re[i] = s as f64 * w;
        }
        audio::fft(&mut re, &mut im);
        mags.push(
            re.iter()
                .zip(&im)
                .map(|(r, i)| (r * r + i * i).sqrt())
                .collect(),
        );
        phases.push(re.iter().zip(&im).map(|(r, i)| (*r, *i)).collect());
    }
    // Floor per bin: 20th-percentile magnitude across frames.
    let mut floor = vec![0.0f64; frame];
    let mut column: Vec<f64> = Vec::with_capacity(frames);
    for bin in 0..frame {
        column.clear();
        column.extend(mags.iter().map(|m| m[bin]));
        column.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        floor[bin] = column[((frames as f64 * 0.2) as usize).min(frames - 1)];
    }
    // Pass 2: gate + inverse FFT + overlap-add.
    let mut out = vec![0.0f64; samples.len()];
    let mut norm = vec![0.0f64; samples.len()];
    for f in 0..frames {
        let mut re = vec![0.0f64; frame];
        let mut im = vec![0.0f64; frame];
        for bin in 0..frame {
            let mag = mags[f][bin];
            let g = if mag > 1e-12 {
                let ratio = floor[bin] / mag;
                (1.0 - ratio * ratio).max(0.05)
            } else {
                0.05
            };
            re[bin] = phases[f][bin].0 * g;
            im[bin] = phases[f][bin].1 * g;
        }
        // Inverse FFT via conjugate trick.
        for v in im.iter_mut() {
            *v = -*v;
        }
        audio::fft(&mut re, &mut im);
        let scale = 1.0 / frame as f64;
        let base = f * hop;
        for (i, rv) in re.iter().enumerate() {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (frame - 1) as f64).cos();
            out[base + i] += rv * scale * w;
            norm[base + i] += w * w;
        }
    }
    let _ = rate;
    out.iter()
        .zip(&norm)
        .map(|(v, n)| if *n > 1e-6 { (v / n) as f32 } else { 0.0 })
        .collect()
}

/// The §4 cleanup chain, in order. Returns (treated, chain).
pub fn cleanup(samples: &[f32], rate: u32) -> (Vec<f32>, Vec<String>) {
    let mut s = samples.to_vec();
    dc_remove(&mut s);
    highpass_25hz(&mut s, rate);
    let s = spectral_gate(&s, rate);
    (
        s,
        vec![
            format!("dc-remove torori-{}", env!("CARGO_PKG_VERSION")),
            format!("highpass-25hz torori-{}", env!("CARGO_PKG_VERSION")),
            format!("spectral-gate torori-{}", env!("CARGO_PKG_VERSION")),
        ],
    )
}

// ── §4.4 finish ───────────────────────────────────────────────

/// Two-pass loudness finish to −16 LUFS (est) with a −1 dBTP
/// ceiling. Returns (samples, lufs_est, true_peak_est).
pub fn finish(samples: &[f32]) -> (Vec<f32>, f64, f64) {
    let lufs0 = audio::dbfs(audio::rms(samples)) - 0.691;
    let gain_db = -16.0 - lufs0;
    let g = 10f64.powf(gain_db / 20.0) as f32;
    let mut out: Vec<f32> = samples.iter().map(|s| s * g).collect();
    // Ceiling pass: if the true-peak estimate exceeds −1 dBTP,
    // scale to it (the re-measured LUFS is reported honestly).
    let tp = audio::dbfs(peak_est(&out));
    if tp > -1.0 {
        let scale = 10f64.powf((-1.0 - tp) / 20.0) as f32;
        for s in &mut out {
            *s *= scale;
        }
    }
    let lufs = audio::dbfs(audio::rms(&out)) - 0.691;
    let tp = audio::dbfs(peak_est(&out));
    (out, round3(lufs), round3(tp))
}

fn peak_est(samples: &[f32]) -> f64 {
    // 4x Catmull-Rom oversampled peak (shared shape with qc).
    let mut best = audio::peak(samples);
    if samples.len() < 4 {
        return best;
    }
    for i in 1..samples.len() - 2 {
        let (y0, y1, y2, y3) = (
            samples[i - 1] as f64,
            samples[i] as f64,
            samples[i + 1] as f64,
            samples[i + 2] as f64,
        );
        for t_i in 1..4 {
            let t = t_i as f64 / 4.0;
            let v = 0.5
                * ((2.0 * y1)
                    + (-y0 + y2) * t
                    + (2.0 * y0 - 5.0 * y1 + 4.0 * y2 - y3) * t * t
                    + (-y0 + 3.0 * y1 - 3.0 * y2 + y3) * t * t * t);
            best = best.max(v.abs());
        }
    }
    best
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

// ── §4.3 gate ─────────────────────────────────────────────────

fn floor_dbfs(samples: &[f32], rate: u32) -> f64 {
    let qc = qc::compute(
        &WavData {
            sample_rate: rate,
            channels: 1,
            samples: samples.to_vec(),
        },
        None,
        false,
    );
    qc.noise_floor_dbfs
}

fn residual_peak_db(samples: &[f32], rate: u32) -> f64 {
    // Treated quietest 1-second window's 1/3-octave band peaks
    // over the band-median level.
    let win = rate as usize;
    if samples.len() < win {
        return 0.0;
    }
    let frame = (rate as usize / 40).max(64);
    let hop = (rate as usize / 100).max(1);
    let mut quietest = (0usize, f64::MAX);
    let mut start = 0usize;
    while start + frame <= samples.len() {
        let r = audio::rms(&samples[start..start + frame]);
        if r < quietest.1 {
            quietest = (start, r);
        }
        start += hop;
    }
    let seg = &samples[quietest.0..(quietest.0 + win).min(samples.len())];
    let (freqs, mags) = audio::magnitude_spectrum(seg, rate);
    // 1/3-octave bands from 50 Hz to 16 kHz.
    let mut bands: Vec<f64> = Vec::new();
    let mut center = 50.0f64;
    while center < 16_000.0 {
        let lo = center / 2f64.powf(1.0 / 6.0);
        let hi = center * 2f64.powf(1.0 / 6.0);
        let mut sum = 0.0;
        let mut count = 0usize;
        for (f, m) in freqs.iter().zip(&mags) {
            if *f >= lo && *f < hi {
                sum += m * m;
                count += 1;
            }
        }
        if count > 0 {
            bands.push(audio::dbfs((sum / count as f64).sqrt()));
        }
        center *= 2f64.powf(1.0 / 3.0);
    }
    if bands.len() < 3 {
        return 0.0;
    }
    let mut sorted = bands.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    bands.iter().map(|b| b - median).fold(0.0, f64::max)
}

/// Measure the §4.3 gate for (untreated, treated) and the
/// finished master of whichever form would ship.
pub fn measure_gate(untreated: &[f32], treated: &[f32], rate: u32) -> GateNumbers {
    let untreated_floor = floor_dbfs(untreated, rate);
    let treated_floor = floor_dbfs(treated, rate);
    let floor_reduction = untreated_floor - treated_floor;
    let u_qc = qc::compute(
        &WavData {
            sample_rate: rate,
            channels: 1,
            samples: untreated.to_vec(),
        },
        None,
        false,
    );
    let t_qc = qc::compute(
        &WavData {
            sample_rate: rate,
            channels: 1,
            samples: treated.to_vec(),
        },
        None,
        false,
    );
    // F0 correlation on frames voiced in both.
    let u_track = audio::f0_track(untreated, rate);
    let t_track = audio::f0_track(treated, rate);
    let (mut ua, mut ta): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    for ((uf, uv), (tf, tv)) in u_track.iter().zip(&t_track) {
        if *uv > 0.5 && *tv > 0.5 && *uf > 0.0 && *tf > 0.0 {
            ua.push(uf.ln());
            ta.push(tf.ln());
        }
    }
    let f0_corr = if ua.len() >= 8 {
        audio::pearson(&ua, &ta)
    } else {
        0.0
    };
    // Removed-signal phonation: frames voiced in the untreated
    // comp, measured in (untreated − treated).
    let removed: Vec<f32> = untreated.iter().zip(treated).map(|(u, t)| u - t).collect();
    let frame = (rate as usize / 40).max(64);
    let hop = (rate as usize / 100).max(1);
    let voice_threshold = audio::dbfs(audio::rms(untreated)) - 30.0;
    let mut phonation = -120.0f64;
    let mut start = 0usize;
    while start + frame <= untreated.len() {
        let u_rms = audio::rms(&untreated[start..start + frame]);
        if audio::dbfs(u_rms) > voice_threshold {
            let r_rms = audio::rms(&removed[start..start + frame]);
            phonation = phonation.max(audio::dbfs(r_rms));
        }
        start += hop;
    }
    let (_, master_lufs, master_tp) = finish(treated);
    let mut failing: Vec<String> = Vec::new();
    if !(floor_reduction >= 6.0 || treated_floor <= -60.0) {
        failing.push(format!(
            "floor: reduction {floor_reduction:.2} dB < 6 and treated floor {treated_floor:.2} dBFS > −60"
        ));
    }
    let residual = residual_peak_db(treated, rate);
    if residual > 6.0 {
        failing.push(format!(
            "residual peaks: {residual:.2} dB over the treated floor in a 1/3-octave band > 6"
        ));
    }
    let env_delta = (t_qc.envelope_dynamic_range_db - u_qc.envelope_dynamic_range_db).abs();
    if env_delta > 1.5 {
        failing.push(format!("envelope range delta {env_delta:.2} dB > ±1.5"));
    }
    let crest_delta = (t_qc.crest_factor_db - u_qc.crest_factor_db).abs();
    if crest_delta > 1.0 {
        failing.push(format!("crest delta {crest_delta:.2} dB > ±1"));
    }
    if f0_corr < 0.95 {
        failing.push(format!(
            "F0 correlation {f0_corr:.3} < 0.95 (or too few voiced frames to verify)"
        ));
    }
    if (master_lufs - -16.0).abs() > 0.5 {
        failing.push(format!("master LUFS {master_lufs:.2} outside −16 ±0.5"));
    }
    if master_tp > -1.0 {
        failing.push(format!("master true peak {master_tp:.2} dBTP > −1"));
    }
    if phonation > -40.0 {
        failing.push(format!(
            "removed signal carries phonation at {phonation:.2} dBFS > −40"
        ));
    }
    GateNumbers {
        floor_reduction_db: round3(floor_reduction),
        treated_floor_dbfs: round3(treated_floor),
        max_residual_peak_db: round3(residual),
        envelope_range_delta_db: round3(env_delta),
        crest_delta_db: round3(crest_delta),
        f0_correlation: round3(f0_corr),
        master_lufs: round3(master_lufs),
        master_true_peak_dbtp: round3(master_tp),
        removed_phonation_dbfs: round3(phonation),
        verdict: if failing.is_empty() {
            "SHIP".into()
        } else {
            "NO-SHIP".into()
        },
        failing_rungs: failing,
    }
}

// ── Assembly ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provenance {
    Generated,
    Library,
    File,
    Sfx,
}

struct ResolvedLayer {
    samples: Vec<f32>,
    provenance: Provenance,
    description: String,
}

fn resolve_layer(
    paths: &Paths,
    library: &LibraryStore,
    layer: &Value,
) -> TororiResult<ResolvedLayer> {
    let kind = layer
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| TororiError::invalid_input("layer.kind is required"))?;
    let to48 = |wav: WavData| -> Vec<f32> {
        let mono = wav.mono();
        if wav.sample_rate == RATE {
            mono
        } else {
            audio::resample_linear(&mono, wav.sample_rate, RATE)
        }
    };
    match kind {
        "file" => {
            let path = layer
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| TororiError::invalid_input("file layer needs path"))?;
            let p = std::path::PathBuf::from(path);
            // Provenance honesty: a path inside the generated
            // assets dir IS generated material, whatever the
            // caller declared.
            let provenance = if p.starts_with(paths.assets_dir()) {
                Provenance::Generated
            } else {
                Provenance::File
            };
            Ok(ResolvedLayer {
                samples: to48(audio::read_wav(&p)?),
                provenance,
                description: format!("file {path}"),
            })
        }
        "generated" => {
            let asset_id = layer
                .get("asset_id")
                .and_then(Value::as_str)
                .ok_or_else(|| TororiError::invalid_input("generated layer needs asset_id"))?;
            let file = paths.assets_dir().join(asset_id).join("take.wav");
            let file = if file.exists() {
                file
            } else {
                paths.assets_dir().join(asset_id).join("render.wav")
            };
            Ok(ResolvedLayer {
                samples: to48(audio::read_wav(&file)?),
                provenance: Provenance::Generated,
                description: format!("generated asset {asset_id}"),
            })
        }
        "library" => {
            let pack = layer
                .get("pack")
                .and_then(Value::as_str)
                .ok_or_else(|| TororiError::invalid_input("library layer needs pack"))?;
            let file = layer
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| TororiError::invalid_input("library layer needs file"))?;
            let row = library.find(pack, file)?;
            row.decide()?;
            Ok(ResolvedLayer {
                samples: to48(audio::read_wav(std::path::Path::new(&row.library_path))?),
                provenance: Provenance::Library,
                description: format!(
                    "library {pack}/{file} ({})",
                    row.attribution_line.clone().unwrap_or_default()
                ),
            })
        }
        "sfx" => {
            let instrument = layer
                .get("instrument")
                .and_then(Value::as_str)
                .ok_or_else(|| TororiError::invalid_input("sfx layer needs instrument"))?;
            let params = layer.get("params").cloned().unwrap_or(json!({}));
            let seed = layer.get("seed").and_then(Value::as_u64).unwrap_or(1);
            let duration = layer
                .get("duration_sec")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let wav = match instrument {
                "room_tone" => crate::sfx::room_tone(seed, duration)?,
                "impact" => crate::sfx::impact(
                    params
                        .get("preset")
                        .and_then(Value::as_str)
                        .unwrap_or("hand"),
                    params.get("force").and_then(Value::as_f64).unwrap_or(0.5),
                    seed,
                    duration,
                )?,
                "wet_layer" => crate::sfx::wet_layer(
                    params.get("speed").and_then(Value::as_f64).unwrap_or(0.5),
                    params.get("depth").and_then(Value::as_f64).unwrap_or(0.5),
                    seed,
                    duration,
                )?,
                "friction" => crate::sfx::friction(
                    params
                        .get("material")
                        .and_then(Value::as_str)
                        .unwrap_or("leather"),
                    params.get("rate_hz").and_then(Value::as_f64).unwrap_or(0.8),
                    seed,
                    duration,
                )?,
                "bed" => crate::sfx::bed(
                    params
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("motor"),
                    params.get("setting").and_then(Value::as_u64).unwrap_or(3) as u32,
                    duration,
                )?,
                other => {
                    return Err(TororiError::invalid_input(format!(
                        "unknown sfx instrument {other:?}"
                    )));
                }
            };
            Ok(ResolvedLayer {
                samples: wav.samples,
                provenance: Provenance::Sfx,
                description: format!("sfx {instrument}"),
            })
        }
        other => Err(TororiError::invalid_input(format!(
            "unknown layer kind {other:?} (file|generated|library|sfx)"
        ))),
    }
}

/// `torori_comp_assemble`.
pub fn assemble(paths: &Paths, library: &LibraryStore, args: &Value) -> TororiResult<Value> {
    let asset_id = arg_str(args, "asset_id")?;
    let base = crate::util::arg_value(args, "base")?;
    let layers = args
        .get("layers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let want_cleanup = args.get("cleanup").and_then(Value::as_bool).unwrap_or(true);
    let base_resolved = resolve_layer(paths, library, &base)?;
    let mut resolved: Vec<(ResolvedLayer, f64, usize)> = Vec::new();
    for layer in &layers {
        let gain_db = layer.get("gain_db").and_then(Value::as_f64).unwrap_or(0.0);
        let offset = ((layer
            .get("offset_ms")
            .and_then(Value::as_f64)
            .unwrap_or(0.0))
            * RATE as f64
            / 1000.0) as usize;
        resolved.push((resolve_layer(paths, library, layer)?, gain_db, offset));
    }
    // Provenance law (conjunction ban, assembly direction):
    // generated material never enters a pack-derived asset.
    if base_resolved.provenance == Provenance::Library
        && resolved
            .iter()
            .any(|(l, _, _)| l.provenance == Provenance::Generated)
    {
        return Err(TororiError::new(
            "provenance_laundering",
            "refusing: a generated layer may not be comped into a pack-derived asset (the conjunction ban's assembly direction)",
        ));
    }
    // Mix.
    let total = resolved
        .iter()
        .map(|(l, _, off)| off + l.samples.len())
        .max()
        .unwrap_or(0)
        .max(base_resolved.samples.len());
    let mut mix = vec![0.0f32; total];
    for (i, s) in base_resolved.samples.iter().enumerate() {
        mix[i] += s;
    }
    let mut source_notes = vec![format!("base: {}", base_resolved.description)];
    for (layer, gain_db, offset) in &resolved {
        let g = 10f64.powf(gain_db / 20.0) as f32;
        for (i, s) in layer.samples.iter().enumerate() {
            mix[offset + i] += s * g;
        }
        source_notes.push(format!("layer: {} at {gain_db} dB", layer.description));
    }
    // Cleanup + §4.3 gate.
    let (treated, chain) = if want_cleanup {
        cleanup(&mix, RATE)
    } else {
        (mix.clone(), Vec::new())
    };
    let numbers = measure_gate(&mix, &treated, RATE);
    let (ship_samples, shipped_form) = if want_cleanup && numbers.verdict == "SHIP" {
        (treated.clone(), "cleaned (gate SHIP)")
    } else if want_cleanup {
        (mix.clone(), "untreated (gate NO-SHIP)")
    } else {
        (mix.clone(), "untreated (cleanup not requested)")
    };
    let (master, lufs, tp) = finish(&ship_samples);
    let master_wav = WavData {
        sample_rate: RATE,
        channels: 1,
        samples: master,
    };
    let bytes = audio::encode_wav(&master_wav);
    let sha = sha256_hex(&bytes);
    let dir = paths.assets_dir().join(asset_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| TororiError::new("io", format!("mkdir asset: {e}")))?;
    let file_path = dir.join("comp.wav");
    std::fs::write(&file_path, &bytes)
        .map_err(|e| TororiError::new("io", format!("write comp: {e}")))?;
    let qc_record = qc::compute(&master_wav, None, false);
    let mut manifest = AssetManifest::new(asset_id, "route-c-comp");
    manifest.character_id = "class".into();
    manifest.prompt = format!("Route C comp: {}", source_notes.join("; "));
    manifest.parameters = json!({
        "base": base, "layers": layers, "cleanup": want_cleanup,
    });
    manifest.determinism_class = DeterminismClass::BitExact;
    manifest.cleanup = Some(crate::manifest::CleanupRecord {
        chain,
        gate: Some(numbers.clone()),
    });
    manifest.finish = Some(FinishRecord {
        lufs,
        true_peak_dbtp: tp,
        sample_rate: RATE,
        channels: 1,
    });
    manifest.qc = Some(qc_record.clone());
    manifest.honesty_label = HonestyLabel::Measured;
    manifest.rights = format!(
        "Route C comp of declared sources only: {}. Library material is comping material under its attribution; no generated material enters a pack-derived asset; no pack material conditions any AI lane.",
        source_notes.join("; ")
    );
    manifest.stamp_output(&bytes);
    let problems = manifest.validate();
    if !problems.is_empty() {
        return Err(TororiError::new("manifest_incomplete", problems.join("; ")));
    }
    std::fs::write(dir.join("manifest.ron"), manifest.to_ron()?)
        .map_err(|e| TororiError::new("io", format!("write manifest: {e}")))?;
    Ok(json!({
        "asset_id": asset_id,
        "file": file_path.display().to_string(),
        "sha256": sha,
        "shipped_form": shipped_form,
        "gate": serde_json::to_value(&numbers).unwrap_or(Value::Null),
        "qc": serde_json::to_value(&qc_record).unwrap_or(Value::Null),
        "note": "a NO-SHIP gate returns the untreated comp plus the failing numbers — never a silently cleaned file (§4.3)",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_hits_the_loudness_window() {
        let tone: Vec<f32> = (0..RATE as usize)
            .map(|i| {
                0.2 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / RATE as f64).sin() as f32
            })
            .collect();
        let (_out, lufs, tp) = finish(&tone);
        assert!((lufs - -16.0).abs() <= 0.5, "lufs {lufs}");
        assert!(tp <= -1.0 + 0.01, "tp {tp}");
    }

    #[test]
    fn gate_rejects_phonation_in_the_removed_bin() {
        // Adversarial §4.3: a "cleanup" that removes the voice
        // itself must fail — simulate by treating with silence
        // on voiced frames.
        let voice: Vec<f32> = (0..RATE as usize)
            .map(|i| {
                0.3 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / RATE as f64).sin() as f32
            })
            .collect();
        let treated = vec![0.0f32; RATE as usize];
        let numbers = measure_gate(&voice, &treated, RATE);
        assert_eq!(numbers.verdict, "NO-SHIP");
        assert!(
            numbers
                .failing_rungs
                .iter()
                .any(|r| r.contains("phonation") || r.contains("F0")),
            "{:?}",
            numbers.failing_rungs
        );
    }
}
