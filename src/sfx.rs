//! Deterministic DSP SFX instruments (brief §7 T5, STUDY §3).
//!
//! Every instrument is a pure function of (parameters, seed):
//! same inputs → same samples → same sha256 (BIT-EXACT class).
//! PRNG is SplitMix64 — no platform entropy anywhere. The
//! spike's three contract checks are this module's unit tests:
//! impact peak monotonic in force; Minnaert centroid falls with
//! depth; friction envelope recovers the commanded stick-slip
//! rate.

use serde_json::{Value, json};

use crate::audio::{self, WavData};
use crate::error::{TororiError, TororiResult};
use crate::manifest::{AssetManifest, DeterminismClass, FinishRecord, HonestyLabel};
use crate::qc;
use crate::state::Paths;
use crate::util::{arg_str, sha256_hex};

pub const RATE: u32 = 48_000;

/// SplitMix64: tiny, portable, deterministic.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Approximately standard normal (Irwin–Hall, 12 uniforms).
    pub fn next_gauss(&mut self) -> f64 {
        let mut sum = 0.0;
        for _ in 0..12 {
            sum += self.next_f64();
        }
        sum - 6.0
    }
}

fn clamp_duration(d: f64) -> TororiResult<usize> {
    if !(0.05..=30.0).contains(&d) {
        return Err(TororiError::invalid_input(
            "duration_sec out of range 0.05..=30",
        ));
    }
    Ok((d * RATE as f64) as usize)
}

/// Modal impact bank (STUDY §3): a damped resonator bank whose
/// excitation spectrum and low-mode gain are set by the preset;
/// force scales burst energy and low-mode gain — peak amplitude
/// is monotonic in force by construction.
pub fn impact(preset: &str, force: f64, seed: u64, duration_sec: f64) -> TororiResult<WavData> {
    if !(0.0..=1.0).contains(&force) {
        return Err(TororiError::invalid_input("force out of range 0..=1"));
    }
    // (base Hz, mode ratios, decay seconds, transient brightness)
    let (base, ratios, decay, bright): (f64, &[f64], f64, f64) = match preset {
        "hand" => (180.0, &[1.0, 2.32, 3.91, 5.4], 0.22, 0.4),
        "paddle" => (240.0, &[1.0, 2.1, 3.4, 4.9], 0.16, 0.55),
        "crop" => (420.0, &[1.0, 1.9, 3.1, 4.4], 0.09, 0.9),
        "flogger" => (140.0, &[1.0, 2.6, 4.2], 0.3, 0.25),
        other => {
            return Err(TororiError::invalid_input(format!(
                "unknown impact preset {other:?} (hand|paddle|crop|flogger)"
            )));
        }
    };
    let n = clamp_duration(duration_sec)?;
    let mut rng = SplitMix64::new(seed);
    let mut out = vec![0.0f32; n];
    // Modal body: gain of mode k falls with k; force lifts the
    // low modes most (low-mode gain ∝ force).
    for (k, ratio) in ratios.iter().enumerate() {
        let freq = base * ratio;
        let mode_gain =
            (0.9 / (1.0 + k as f64 * 1.3)) * (0.35 + 0.65 * force / (1.0 + k as f64 * 0.2));
        let phase = rng.next_f64() * std::f64::consts::TAU;
        let tau = decay / (1.0 + k as f64 * 0.35);
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f64 / RATE as f64;
            let env = (-t / tau).exp();
            *s += (mode_gain * env * (std::f64::consts::TAU * freq * t + phase).sin()) as f32;
        }
    }
    // Excitation transient: seeded noise burst, brightness by
    // preset, length ∝ 12 ms; energy ∝ force.
    let burst = (0.012 * RATE as f64) as usize;
    let mut lp = 0.0f64;
    let alpha = 0.15 + 0.6 * bright;
    for (i, s) in out.iter_mut().enumerate().take(burst.min(n)) {
        let white = rng.next_gauss();
        lp += alpha * (white - lp);
        let hp = white - lp;
        let env = 1.0 - i as f64 / burst as f64;
        *s += (0.8 * force * env * hp) as f32;
    }
    // Peak target monotonic in force by construction.
    let peak_now = audio::peak(&out);
    let target = 0.15 + 0.8 * force;
    if peak_now > 0.0 {
        let g = (target / peak_now) as f32;
        for s in &mut out {
            *s *= g;
        }
    }
    Ok(WavData {
        sample_rate: RATE,
        channels: 1,
        samples: out,
    })
}

/// Minnaert wet layer (STUDY §3): a bubble population where
/// bubble rate ∝ speed and the radius distribution shifts with
/// depth — and since f·r ≈ 3.26 m/s, deeper (larger bubbles)
/// means a lower spectral centroid.
pub fn wet_layer(speed: f64, depth: f64, seed: u64, duration_sec: f64) -> TororiResult<WavData> {
    if !(0.0..=1.0).contains(&speed) || !(0.0..=1.0).contains(&depth) {
        return Err(TororiError::invalid_input("speed/depth out of range 0..=1"));
    }
    let n = clamp_duration(duration_sec)?;
    let mut rng = SplitMix64::new(seed);
    let mut out = vec![0.0f32; n];
    let rate = 15.0 + 165.0 * speed; // bubbles per second
    let count = (rate * duration_sec) as usize;
    // Mean bubble radius (mm) grows with depth: 0.6 → 2.8 mm.
    let mean_r = 0.6 + 2.2 * depth;
    for _ in 0..count {
        let at = (rng.next_f64() * n as f64) as usize;
        // Log-normal-ish radius around the depth mean.
        let r_mm = mean_r * (0.7 + 0.6 * rng.next_f64());
        let freq = 3260.0 / r_mm; // f·r ≈ 3.26 m/s, r in mm → Hz
        let len = ((0.004 + 0.004 * rng.next_f64()) * RATE as f64) as usize;
        let amp = 0.05 + 0.2 * speed * rng.next_f64();
        let phase = rng.next_f64() * std::f64::consts::TAU;
        for j in 0..len {
            let idx = at + j;
            if idx >= n {
                break;
            }
            let t = j as f64 / RATE as f64;
            let env = (-t / 0.0022).exp();
            out[idx] += (amp * env * (std::f64::consts::TAU * freq * t + phase).sin()) as f32;
        }
    }
    Ok(WavData {
        sample_rate: RATE,
        channels: 1,
        samples: out,
    })
}

/// Stick-slip friction (STUDY §3): seeded noise through a
/// one-pole lowpass (the material), amplitude-gated at the
/// stick-slip rate — the envelope spectrum peaks at the
/// commanded rate.
pub fn friction(
    material: &str,
    rate_hz: f64,
    seed: u64,
    duration_sec: f64,
) -> TororiResult<WavData> {
    if !(0.1..=8.0).contains(&rate_hz) {
        return Err(TororiError::invalid_input("rate_hz out of range 0.1..=8"));
    }
    let (alpha, gain): (f64, f64) = match material {
        "leather" => (0.05, 0.7),
        "rope" => (0.1, 0.8),
        "skin" => (0.25, 0.5),
        "fabric" => (0.4, 0.35),
        other => {
            return Err(TororiError::invalid_input(format!(
                "unknown friction material {other:?} (leather|rope|skin|fabric)"
            )));
        }
    };
    let n = clamp_duration(duration_sec)?;
    let mut rng = SplitMix64::new(seed);
    let mut out = vec![0.0f32; n];
    let mut lp = 0.0f64;
    for (i, s) in out.iter_mut().enumerate() {
        let white = rng.next_gauss();
        lp += alpha * (white - lp);
        // Stick-slip gate: pulses at rate_hz with a sharp attack.
        let phase = (i as f64 * rate_hz / RATE as f64).fract();
        let gate = if phase < 0.25 {
            (1.0 - phase / 0.25) * 0.9 + 0.1
        } else {
            0.1
        };
        *s = (gain * gate * lp) as f32;
    }
    Ok(WavData {
        sample_rate: RATE,
        channels: 1,
        samples: out,
    })
}

/// Oscillator / motor bed: the setting is an authored step
/// (0..=10), not a continuum (STUDY §3).
pub fn bed(kind: &str, setting: u32, duration_sec: f64) -> TororiResult<WavData> {
    if setting > 10 {
        return Err(TororiError::invalid_input("setting must be 0..=10"));
    }
    let (fundamental, harmonics): (f64, &[f64]) = match kind {
        "motor" => (28.0, &[1.0, 0.5, 0.22]),
        "oscillator" => (55.0, &[1.0, 0.3, 0.12, 0.05]),
        other => {
            return Err(TororiError::invalid_input(format!(
                "unknown bed kind {other:?} (motor|oscillator)"
            )));
        }
    };
    let n = clamp_duration(duration_sec)?;
    let freq = fundamental * (1.0 + setting as f64 * 0.22);
    let mut out = vec![0.0f32; n];
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f64 / RATE as f64;
        let lfo = 0.85 + 0.15 * (std::f64::consts::TAU * 0.4 * t).sin();
        let mut v = 0.0f64;
        for (k, g) in harmonics.iter().enumerate() {
            v += g * (std::f64::consts::TAU * freq * (k + 1) as f64 * t).sin();
        }
        *s = (0.3 * lfo * v) as f32;
    }
    Ok(WavData {
        sample_rate: RATE,
        channels: 1,
        samples: out,
    })
}

/// Room tone: filtered noise + slow LFO, low level.
pub fn room_tone(seed: u64, duration_sec: f64) -> TororiResult<WavData> {
    let n = clamp_duration(duration_sec)?;
    let mut rng = SplitMix64::new(seed);
    let mut out = Vec::with_capacity(n);
    let mut lp = 0.0f64;
    for i in 0..n {
        let white = rng.next_gauss();
        lp += 0.03 * (white - lp);
        let lfo = 0.9 + 0.1 * (std::f64::consts::TAU * 0.1 * i as f64 / RATE as f64).sin();
        out.push((0.05 * lfo * lp) as f32);
    }
    Ok(WavData {
        sample_rate: RATE,
        channels: 1,
        samples: out,
    })
}

/// `torori_sfx_render`: render one instrument to a master WAV
/// with its §5.2 manifest. BIT-EXACT class only — a request in
/// any other class refuses rather than substituting a lane
/// (brief T5 adversarial).
pub fn render(paths: &Paths, args: &Value) -> TororiResult<Value> {
    let instrument = arg_str(args, "instrument")?;
    let class = args
        .get("determinism_class")
        .and_then(Value::as_str)
        .unwrap_or("BIT-EXACT");
    if class != "BIT-EXACT" {
        return Err(TororiError::new(
            "determinism_class",
            format!(
                "SFX render is BIT-EXACT only; requested {class} — refusing rather than substituting an ML lane"
            ),
        ));
    }
    let seed = args.get("seed").and_then(Value::as_u64).unwrap_or(1);
    let duration = args
        .get("duration_sec")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let params = args.get("params").cloned().unwrap_or(json!({}));
    let (wav, lane, summary) = match instrument {
        "impact" => {
            let preset = params
                .get("preset")
                .and_then(Value::as_str)
                .unwrap_or("hand");
            let force = params.get("force").and_then(Value::as_f64).unwrap_or(0.5);
            (
                impact(preset, force, seed, duration)?,
                format!("dsp-impact-{preset}"),
                format!("modal impact bank, preset {preset}, force {force}"),
            )
        }
        "wet_layer" => {
            let speed = params.get("speed").and_then(Value::as_f64).unwrap_or(0.5);
            let depth = params.get("depth").and_then(Value::as_f64).unwrap_or(0.5);
            (
                wet_layer(speed, depth, seed, duration)?,
                "dsp-wet-minnaert".to_string(),
                format!("Minnaert bubble population, speed {speed}, depth {depth}"),
            )
        }
        "friction" => {
            let material = params
                .get("material")
                .and_then(Value::as_str)
                .unwrap_or("leather");
            let rate = params.get("rate_hz").and_then(Value::as_f64).unwrap_or(0.8);
            (
                friction(material, rate, seed, duration)?,
                format!("dsp-friction-{material}"),
                format!(
                    "stick-slip friction, {material}, {rate_hz} Hz",
                    rate_hz = rate
                ),
            )
        }
        "bed" => {
            let kind = params
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("motor");
            let setting = params.get("setting").and_then(Value::as_u64).unwrap_or(3) as u32;
            (
                bed(kind, setting, duration)?,
                format!("dsp-bed-{kind}"),
                format!("{kind} bed at authored setting step {setting}"),
            )
        }
        "room_tone" => (
            room_tone(seed, duration)?,
            "dsp-room-tone".to_string(),
            "room tone: filtered noise + slow LFO".to_string(),
        ),
        other => {
            return Err(TororiError::invalid_input(format!(
                "unknown instrument {other:?} (impact|wet_layer|friction|bed|room_tone)"
            )));
        }
    };
    let bytes = audio::encode_wav(&wav);
    let sha = sha256_hex(&bytes);
    let asset_id = format!("sfx.{instrument}-{}-{seed}", &sha[..8]);
    let dir = paths.assets_dir().join(&asset_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| TororiError::new("io", format!("mkdir asset: {e}")))?;
    let file_path = dir.join("render.wav");
    std::fs::write(&file_path, &bytes)
        .map_err(|e| TororiError::new("io", format!("write render: {e}")))?;
    let qc_record = qc::compute(&wav, Some(duration), false);
    let mut manifest = AssetManifest::new(&asset_id, &lane);
    manifest.character_id = "class".into();
    manifest.prompt = summary;
    manifest.parameters = json!({
        "instrument": instrument, "params": params, "seed": seed, "duration_sec": duration,
    });
    manifest.seed = Some(seed);
    manifest.determinism_class = DeterminismClass::BitExact;
    manifest.qc = Some(qc_record.clone());
    manifest.finish = Some(FinishRecord {
        lufs: qc_record.lufs_integrated_est,
        true_peak_dbtp: qc_record.true_peak_dbtp_est,
        sample_rate: RATE,
        channels: 1,
    });
    manifest.honesty_label = HonestyLabel::Authored;
    manifest.rights =
        "estate-authored deterministic DSP; no third-party material, no pack content".into();
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
        "determinism_class": "BIT-EXACT",
        "qc": serde_json::to_value(&qc_record).unwrap_or(Value::Null),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_sha(wav: &WavData) -> String {
        sha256_hex(&audio::encode_wav(wav))
    }

    #[test]
    fn double_render_is_bit_exact() {
        for (a, b) in [
            (
                render_sha(&impact("paddle", 0.6, 7, 0.5).unwrap()),
                render_sha(&impact("paddle", 0.6, 7, 0.5).unwrap()),
            ),
            (
                render_sha(&wet_layer(0.5, 0.5, 3, 1.0).unwrap()),
                render_sha(&wet_layer(0.5, 0.5, 3, 1.0).unwrap()),
            ),
            (
                render_sha(&friction("rope", 0.8, 9, 1.0).unwrap()),
                render_sha(&friction("rope", 0.8, 9, 1.0).unwrap()),
            ),
        ] {
            assert_eq!(a, b, "same params + seed → same bytes");
        }
        // A different seed is a different render.
        assert_ne!(
            render_sha(&impact("hand", 0.5, 1, 0.5).unwrap()),
            render_sha(&impact("hand", 0.5, 2, 0.5).unwrap())
        );
    }

    #[test]
    fn spike_contract_impact_peak_monotonic_in_force() {
        let mut last = 0.0;
        for force in [0.1, 0.35, 0.6, 0.85, 1.0] {
            let wav = impact("hand", force, 5, 0.5).unwrap();
            let p = audio::peak(&wav.samples);
            assert!(p > last, "peak {p} must exceed {last} at force {force}");
            last = p;
        }
    }

    #[test]
    fn spike_contract_minnaert_centroid_falls_with_depth() {
        let shallow =
            audio::spectral_centroid(&wet_layer(0.6, 0.1, 11, 2.0).unwrap().samples, RATE);
        let deep = audio::spectral_centroid(&wet_layer(0.6, 0.9, 11, 2.0).unwrap().samples, RATE);
        assert!(
            deep < shallow,
            "centroid must fall with depth: shallow {shallow:.0} Hz vs deep {deep:.0} Hz"
        );
    }

    #[test]
    fn spike_contract_friction_envelope_recovers_rate() {
        for rate in [0.3, 0.8, 1.4] {
            let wav = friction("leather", rate, 13, 4.0).unwrap();
            // Envelope via frame RMS, then its spectrum peak.
            let hop = (RATE / 100) as usize;
            let frame = (RATE / 40) as usize;
            let mut env = Vec::new();
            let mut start = 0usize;
            while start + frame <= wav.samples.len() {
                env.push(audio::rms(&wav.samples[start..start + frame]) as f32);
                start += hop;
            }
            // Remove DC, then find the spectral peak below 3 Hz.
            let mean = env.iter().map(|v| *v as f64).sum::<f64>() / env.len() as f64;
            let centered: Vec<f32> = env.iter().map(|v| (*v as f64 - mean) as f32).collect();
            let (freqs, mags) = audio::magnitude_spectrum(&centered, 100);
            let mut best = (0.0f64, 0.0f64);
            for (f, m) in freqs.iter().zip(&mags) {
                if *f > 0.05 && *f < 3.0 && *m > best.1 {
                    best = (*f, *m);
                }
            }
            assert!(
                (best.0 - rate).abs() < 0.2,
                "commanded {rate} Hz, envelope peak {:.2} Hz",
                best.0
            );
        }
    }
}
