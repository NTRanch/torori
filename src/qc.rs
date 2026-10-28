//! The §5.3 QC metric set: computed for 100% of generated
//! assets, metric-only (silence rule enforcement point 3 —
//! nothing here listens to anything; these are numbers over
//! sample buffers).
//!
//! LUFS here is an energy-based BS.1770-style estimate without
//! K-weighting (labelled `lufs_integrated_est`); the comp lane's
//! finish pass uses ffmpeg loudnorm when the pinned binary is
//! present and records which path produced the shipped numbers.

use serde::{Deserialize, Serialize};

use crate::audio::{self, WavData};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreathBracket {
    pub lead_in_present: bool,
    pub lead_out_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcRecord {
    pub duration_sec: f64,
    pub duration_delta_sec: Option<f64>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Quiet-frame RMS floor, dBFS.
    pub noise_floor_dbfs: f64,
    /// Spectral flatness of the floor frames (0..1).
    pub floor_flatness: f64,
    pub crest_factor_db: f64,
    /// Voiced-frame envelope dynamic range, dB.
    pub envelope_dynamic_range_db: f64,
    pub lufs_integrated_est: f64,
    pub true_peak_dbtp_est: f64,
    pub f0_median_hz: Option<f64>,
    pub f0_range_semitones: Option<f64>,
    /// Fraction of frames classified voiced (0..1).
    pub voiced_fraction: f64,
    /// Fraction of voiced frames within ±0.5 semitone of the
    /// voiced median (track stability).
    pub f0_track_stability: Option<f64>,
    pub clipped_samples: usize,
    pub zero_run_max_ms: f64,
    pub dc_offset: f64,
    pub breath_bracket: Option<BreathBracket>,
    /// Inter-channel onset offset for paired/stereo assets, ms.
    pub onset_offset_ms: Option<f64>,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Cubic-interpolated 4× oversampled peak (true-peak estimate).
fn true_peak_est(samples: &[f32]) -> f64 {
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
            // Catmull-Rom.
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

/// Compute the full QC record for one asset.
pub fn compute(
    wav: &WavData,
    target_duration_sec: Option<f64>,
    want_breath_bracket: bool,
) -> QcRecord {
    let mono = wav.mono();
    let rate = wav.sample_rate;
    // Frame grid: 25 ms frames, 10 ms hop (shared with f0_track).
    let frame = (rate as usize / 40).max(64);
    let hop = (rate as usize / 100).max(1);
    let mut frame_rms: Vec<f64> = Vec::new();
    let mut start = 0usize;
    while start + frame <= mono.len() {
        frame_rms.push(audio::rms(&mono[start..start + frame]));
        start += hop;
    }
    // Floor = quietest decile of frames.
    let mut sorted = frame_rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let decile = ((sorted.len() as f64 * 0.1).ceil() as usize)
        .max(1)
        .min(sorted.len());
    let floor_frames: Vec<f64> = sorted.iter().take(decile).copied().collect();
    let floor_rms = if floor_frames.is_empty() {
        0.0
    } else {
        floor_frames.iter().sum::<f64>() / floor_frames.len() as f64
    };
    let floor_db = audio::dbfs(floor_rms);
    // Flatness over the actual quietest frame's samples.
    let flatness = if frame_rms.is_empty() {
        0.0
    } else {
        let quietest = frame_rms
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let at = quietest * hop;
        let seg = &mono[at..(at + frame).min(mono.len())];
        audio::spectral_flatness(seg, rate)
    };
    // Voiced frames: RMS above floor + 6 dB.
    let voiced_threshold = floor_rms * 2.0; // +6 dB in amplitude ≈ ×2
    let voiced_env: Vec<f64> = frame_rms
        .iter()
        .filter(|r| **r > voiced_threshold && **r > 1e-5)
        .map(|r| audio::dbfs(*r))
        .collect();
    let env_range = if voiced_env.len() >= 2 {
        let hi = voiced_env.iter().copied().fold(f64::MIN, f64::max);
        let lo = voiced_env.iter().copied().fold(f64::MAX, f64::min);
        hi - lo
    } else {
        0.0
    };
    let track = audio::f0_track(&mono, rate);
    let voiced_f0: Vec<f64> = track
        .iter()
        .filter(|(_, v)| *v > 0.5)
        .map(|(f, _)| *f)
        .filter(|f| *f > 0.0)
        .collect();
    let (f0_median, f0_range, f0_stability) = if voiced_f0.len() >= 3 {
        let mut v = voiced_f0.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = v[v.len() / 2];
        let p10 = v[(v.len() as f64 * 0.1) as usize];
        let p90 = v[((v.len() as f64 * 0.9) as usize).min(v.len() - 1)];
        let range = if p10 > 0.0 && p90 > 0.0 {
            12.0 * (p90 / p10).log2()
        } else {
            0.0
        };
        let stable = voiced_f0
            .iter()
            .filter(|f| (12.0 * (*f / median).log2()).abs() <= 0.5)
            .count() as f64
            / voiced_f0.len() as f64;
        (
            Some(round3(median)),
            Some(round3(range)),
            Some(round3(stable)),
        )
    } else {
        (None, None, None)
    };
    let breath_bracket = if want_breath_bracket {
        let band = |r: f64| r > floor_rms * 1.4 && r < floor_rms * 8.0 && r > 1e-5;
        let head = frame_rms.iter().take(20).any(|r| band(*r));
        let tail = frame_rms.iter().rev().take(20).any(|r| band(*r));
        Some(BreathBracket {
            lead_in_present: head,
            lead_out_present: tail,
        })
    } else {
        None
    };
    let onset_offset_ms = if wav.channels >= 2 {
        let l = wav.channel(0);
        let r = wav.channel(1);
        let onset = |s: &[f32]| -> Option<usize> {
            let mut st = 0usize;
            while st + frame <= s.len() {
                if audio::rms(&s[st..st + frame]) > voiced_threshold {
                    return Some(st);
                }
                st += hop;
            }
            None
        };
        match (onset(&l), onset(&r)) {
            (Some(a), Some(b)) => Some(round3((a as f64 - b as f64).abs() * 1000.0 / rate as f64)),
            _ => None,
        }
    } else {
        None
    };
    let duration = wav.duration_sec();
    QcRecord {
        duration_sec: round3(duration),
        duration_delta_sec: target_duration_sec.map(|t| round3(duration - t)),
        sample_rate: rate,
        channels: wav.channels,
        noise_floor_dbfs: round3(floor_db),
        floor_flatness: round3(flatness),
        crest_factor_db: round3(audio::dbfs(audio::peak(&mono)) - audio::dbfs(audio::rms(&mono))),
        envelope_dynamic_range_db: round3(env_range),
        lufs_integrated_est: round3(audio::dbfs(audio::rms(&mono)) - 0.691),
        true_peak_dbtp_est: round3(audio::dbfs(true_peak_est(&mono))),
        f0_median_hz: f0_median,
        f0_range_semitones: f0_range,
        voiced_fraction: round3(if frame_rms.is_empty() {
            0.0
        } else {
            voiced_env.len() as f64 / frame_rms.len() as f64
        }),
        f0_track_stability: f0_stability,
        clipped_samples: audio::clipped_count(&mono),
        zero_run_max_ms: round3(
            audio::longest_zero_run(&mono) as f64 * 1000.0 / rate.max(1) as f64,
        ),
        dc_offset: round3(audio::dc_offset(&mono)),
        breath_bracket,
        onset_offset_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qc_on_silence_and_tone() {
        let silent = WavData {
            sample_rate: 48_000,
            channels: 1,
            samples: vec![0.0; 48_000],
        };
        let qc = compute(&silent, Some(1.0), false);
        assert_eq!(qc.duration_sec, 1.0);
        assert_eq!(qc.noise_floor_dbfs, -120.0);
        assert_eq!(qc.voiced_fraction, 0.0);
        assert_eq!(qc.zero_run_max_ms, 1000.0);

        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.3 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / 48_000.0).sin() as f32)
            .collect();
        let wav = WavData {
            sample_rate: 48_000,
            channels: 1,
            samples: tone,
        };
        let qc = compute(&wav, None, false);
        assert!(qc.f0_median_hz.is_some_and(|f| (f - 220.0).abs() < 15.0));
        assert_eq!(qc.clipped_samples, 0);
    }
}
