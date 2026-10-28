//! Audio file core: a self-contained WAV codec and deterministic
//! analysis primitives.
//!
//! No audio crate is used anywhere in this workspace: nothing
//! here can open an output device, by construction (silence rule
//! enforcement point 1). Samples are `f32` in memory; on disk,
//! PCM 16/24-bit and float32 WAV are read, and masters are
//! written as 24-bit PCM WAV (STUDY §4.4) with a fixed chunk
//! layout — writing the same samples twice is byte-identical.

use crate::error::{TororiError, TororiResult};

#[derive(Debug, Clone, PartialEq)]
pub struct WavData {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples, nominal range [-1, 1].
    pub samples: Vec<f32>,
}

impl WavData {
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }

    pub fn duration_sec(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f64 / self.sample_rate as f64
        }
    }

    /// One channel's samples (channel index beyond range returns
    /// the whole buffer, which for mono is the channel itself).
    pub fn channel(&self, index: usize) -> Vec<f32> {
        let ch = self.channels.max(1) as usize;
        if self.channels <= 1 || index >= ch {
            return self.samples.clone();
        }
        self.samples
            .iter()
            .skip(index)
            .step_by(ch)
            .copied()
            .collect()
    }

    pub fn mono(&self) -> Vec<f32> {
        if self.channels <= 1 {
            return self.samples.clone();
        }
        let ch = self.channels as usize;
        self.samples
            .chunks_exact(ch)
            .map(|frame| frame.iter().sum::<f32>() / ch as f32)
            .collect()
    }
}

fn io_err(context: &str, e: std::io::Error) -> TororiError {
    TororiError::new("io", format!("{context}: {e}"))
}

/// Read a WAV file (PCM 16/24/32 int, or 32-bit float).
pub fn read_wav(path: &std::path::Path) -> TororiResult<WavData> {
    let bytes = std::fs::read(path).map_err(|e| io_err(&format!("read {}", path.display()), e))?;
    parse_wav(&bytes)
}

pub fn parse_wav(bytes: &[u8]) -> TororiResult<WavData> {
    let bad = |m: &str| TororiError::new("invalid_audio", format!("wav: {m}"));
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("missing RIFF/WAVE header"));
    }
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(bytes.len());
        match id {
            b"fmt " => {
                if body_end - body_start < 16 {
                    return Err(bad("fmt chunk too small"));
                }
                let b = &bytes[body_start..body_end];
                fmt = Some((
                    u16::from_le_bytes(b[0..2].try_into().unwrap()),
                    u16::from_le_bytes(b[2..4].try_into().unwrap()),
                    u32::from_le_bytes(b[4..8].try_into().unwrap()),
                    u16::from_le_bytes(b[14..16].try_into().unwrap()),
                ));
            }
            b"data" => data = Some(&bytes[body_start..body_end]),
            _ => {}
        }
        pos = body_start + size + (size % 2);
    }
    let (format, channels, rate, bits) = fmt.ok_or_else(|| bad("no fmt chunk"))?;
    let data = data.ok_or_else(|| bad("no data chunk"))?;
    if channels == 0 || rate == 0 {
        return Err(bad("zero channels or sample rate"));
    }
    let samples: Vec<f32> = match (format, bits) {
        (1, 16) => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
            .collect(),
        (1, 24) => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| {
                let v =
                    i32::from_le_bytes([c[0], c[1], c[2], if c[2] & 0x80 != 0 { 0xFF } else { 0 }]);
                v as f32 / 8_388_608.0
            })
            .collect(),
        (1, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
        _ => return Err(bad(&format!("unsupported format {format}/{bits} bits"))),
    };
    Ok(WavData {
        sample_rate: rate,
        channels,
        samples,
    })
}

/// Encode samples as a 24-bit PCM WAV with a fixed chunk layout
/// (deterministic: same input → same bytes).
pub fn encode_wav(wav: &WavData) -> Vec<u8> {
    let data_len = wav.samples.len() * 3;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&wav.channels.to_le_bytes());
    out.extend_from_slice(&wav.sample_rate.to_le_bytes());
    let byte_rate = wav.sample_rate * wav.channels as u32 * 3;
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&(wav.channels * 3).to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for &s in &wav.samples {
        let clamped = s.clamp(-1.0, 1.0 - 1.0 / 8_388_608.0);
        let v = (clamped * 8_388_608.0).round() as i32;
        out.extend_from_slice(&v.to_le_bytes()[0..3]);
    }
    out
}

pub fn write_wav(path: &std::path::Path, wav: &WavData) -> TororiResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| io_err(&format!("mkdir {}", parent.display()), e))?;
    }
    std::fs::write(path, encode_wav(wav))
        .map_err(|e| io_err(&format!("write {}", path.display()), e))
}

/// Read an MP3's duration through the pinned ffprobe binary
/// (library-lane backfill only — the file is never modified).
/// ffprobe's absence is a typed refusal the caller records as
/// UNDETERMINED for that row.
pub fn mp3_duration_ffprobe(path: &std::path::Path) -> TororiResult<f64> {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|e| TororiError::new("tool_missing", format!("ffprobe unavailable: {e}")))?;
    if !out.status.success() {
        return Err(TororiError::new(
            "invalid_audio",
            format!("ffprobe failed on {}", path.display()),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .parse::<f64>()
        .map_err(|_| TororiError::new("invalid_audio", "ffprobe duration unparsable"))
}

// ── Analysis primitives (all deterministic) ────────────────────

pub fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / samples.len() as f64).sqrt()
}

pub fn peak(samples: &[f32]) -> f64 {
    samples
        .iter()
        .map(|s| (*s as f64).abs())
        .fold(0.0, f64::max)
}

pub fn dbfs(v: f64) -> f64 {
    if v <= 0.0 { -120.0 } else { 20.0 * v.log10() }
}

pub fn dc_offset(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        0.0
    } else {
        samples.iter().map(|s| *s as f64).sum::<f64>() / samples.len() as f64
    }
}

/// Longest run of exact-zero samples (digital silence census).
pub fn longest_zero_run(samples: &[f32]) -> usize {
    let mut best = 0usize;
    let mut run = 0usize;
    for &s in samples {
        if s == 0.0 {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

/// Count of samples at clipping level.
pub fn clipped_count(samples: &[f32]) -> usize {
    samples.iter().filter(|s| s.abs() >= 0.999_969).count()
}

/// In-place radix-2 FFT (deterministic; length a power of two).
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for base in (0..n).step_by(len) {
            let (mut cwr, mut cwi) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (ur, ui) = (re[base + k], im[base + k]);
                let (vr, vi) = (re[base + k + len / 2], im[base + k + len / 2]);
                let (tr, ti) = (vr * cwr - vi * cwi, vr * cwi + vi * cwr);
                re[base + k] = ur + tr;
                im[base + k] = ui + ti;
                re[base + k + len / 2] = ur - tr;
                im[base + k + len / 2] = ui - ti;
                let nwr = cwr * wr - cwi * wi;
                cwi = cwr * wi + cwi * wr;
                cwr = nwr;
            }
        }
        len <<= 1;
    }
}

/// Magnitude spectrum of one Hann-windowed frame, zero-padded
/// to the next power of two ≥ frame length.
pub fn magnitude_spectrum(samples: &[f32], sample_rate: u32) -> (Vec<f64>, Vec<f64>) {
    let n = samples.len().next_power_of_two().max(2);
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];
    let m = samples.len();
    for (i, &s) in samples.iter().enumerate() {
        let w = if m > 1 {
            0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (m - 1) as f64).cos()
        } else {
            1.0
        };
        re[i] = s as f64 * w;
    }
    fft(&mut re, &mut im);
    let half = n / 2;
    let freqs: Vec<f64> = (0..half)
        .map(|k| k as f64 * sample_rate as f64 / n as f64)
        .collect();
    let mags: Vec<f64> = (0..half)
        .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
        .collect();
    (freqs, mags)
}

/// Spectral centroid in Hz (0 for silence).
pub fn spectral_centroid(samples: &[f32], sample_rate: u32) -> f64 {
    let (freqs, mags) = magnitude_spectrum(samples, sample_rate);
    let total: f64 = mags.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    freqs.iter().zip(&mags).map(|(f, m)| f * m).sum::<f64>() / total
}

/// Spectral flatness (geometric/arithmetic mean of magnitudes).
pub fn spectral_flatness(samples: &[f32], sample_rate: u32) -> f64 {
    let (_, mags) = magnitude_spectrum(samples, sample_rate);
    let usable: Vec<f64> = mags.iter().copied().filter(|m| *m > 0.0).collect();
    if usable.is_empty() {
        return 0.0;
    }
    let arith = usable.iter().sum::<f64>() / usable.len() as f64;
    if arith <= 0.0 {
        return 0.0;
    }
    let geo = (usable.iter().map(|m| m.ln()).sum::<f64>() / usable.len() as f64).exp();
    geo / arith
}

/// Frame-wise F0 by normalized autocorrelation; one
/// (f0_hz, voicing 0..1) per frame. Deterministic.
pub fn f0_track(samples: &[f32], sample_rate: u32) -> Vec<(f64, f64)> {
    let frame = (sample_rate as usize / 40).max(64); // 25 ms
    let hop = (sample_rate as usize / 100).max(1); // 10 ms
    let min_lag = ((sample_rate as f64 / 500.0) as usize).max(1); // 500 Hz ceiling
    let max_lag = ((sample_rate as f64 / 50.0) as usize).min(frame.saturating_sub(1));
    let mut out = Vec::new();
    if samples.len() < frame || max_lag <= min_lag {
        return out;
    }
    let mut start = 0usize;
    while start + frame <= samples.len() {
        let window = &samples[start..start + frame];
        let energy: f64 = window.iter().map(|s| (*s as f64) * (*s as f64)).sum();
        if energy <= 1e-9 {
            out.push((0.0, 0.0));
        } else {
            let mut best_lag = 0usize;
            let mut best_corr = 0.0f64;
            for lag in min_lag..=max_lag {
                let mut num = 0.0f64;
                let mut den = 0.0f64;
                for i in 0..frame - lag {
                    num += window[i] as f64 * window[i + lag] as f64;
                    den += window[i] as f64 * window[i] as f64;
                }
                let corr = if den > 0.0 { num / den } else { 0.0 };
                if corr > best_corr {
                    best_corr = corr;
                    best_lag = lag;
                }
            }
            let f0 = if best_lag > 0 {
                sample_rate as f64 / best_lag as f64
            } else {
                0.0
            };
            out.push((f0, best_corr.clamp(0.0, 1.0)));
        }
        start += hop;
    }
    out
}

/// Pearson correlation of two equal-length series.
pub fn pearson(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.len() < 2 {
        return 0.0;
    }
    let n = a.len() as f64;
    let ma = a.iter().sum::<f64>() / n;
    let mb = b.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for (x, y) in a.iter().zip(b) {
        num += (x - ma) * (y - mb);
        da += (x - ma) * (x - ma);
        db += (y - mb) * (y - mb);
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da.sqrt() * db.sqrt())
    }
}

/// Short-term envelope (frame RMS on the 10 ms hop grid), dBFS.
pub fn envelope_db(samples: &[f32], sample_rate: u32) -> Vec<f64> {
    let hop = (sample_rate as usize / 100).max(1);
    let frame = (sample_rate as usize / 40).max(64);
    let mut out = Vec::new();
    let mut start = 0usize;
    while start + frame <= samples.len() {
        out.push(dbfs(rms(&samples[start..start + frame])));
        start += hop;
    }
    out
}

/// Linear-interpolation resample (deterministic; the comp lane
/// brings sources to the 48 kHz master rate with it).
pub fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let out_len = (samples.len() as f64 * to_rate as f64 / from_rate as f64).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * from_rate as f64 / to_rate as f64;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;
        let a = samples[idx.min(samples.len() - 1)];
        let b = samples[(idx + 1).min(samples.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, seconds: f64, rate: u32, amp: f32) -> Vec<f32> {
        let n = (seconds * rate as f64) as usize;
        (0..n)
            .map(|i| {
                amp * (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin() as f32
            })
            .collect()
    }

    #[test]
    fn wav_round_trip_is_stable() {
        let wav = WavData {
            sample_rate: 48_000,
            channels: 1,
            samples: sine(440.0, 0.1, 48_000, 0.5),
        };
        let bytes = encode_wav(&wav);
        let back = parse_wav(&bytes).expect("parse");
        assert_eq!(back.sample_rate, 48_000);
        assert_eq!(back.samples.len(), wav.samples.len());
        for (a, b) in wav.samples.iter().zip(&back.samples) {
            assert!((*a as f64 - *b as f64).abs() < 1e-6);
        }
        assert_eq!(encode_wav(&back), bytes, "re-encode is byte-identical");
    }

    #[test]
    fn metrics_on_a_known_sine() {
        let s = sine(440.0, 0.5, 48_000, 0.5);
        assert!((rms(&s) - 0.3536).abs() < 0.01);
        assert!((peak(&s) - 0.5).abs() < 0.01);
        let centroid = spectral_centroid(&s, 48_000);
        assert!((centroid - 440.0).abs() < 30.0, "centroid {centroid}");
        let track = f0_track(&s, 48_000);
        assert!(!track.is_empty());
        let voiced: Vec<f64> = track
            .iter()
            .filter(|(_, v)| *v > 0.5)
            .map(|(f, _)| *f)
            .collect();
        assert!(!voiced.is_empty());
        let median = voiced[voiced.len() / 2];
        assert!((median - 440.0).abs() < 25.0, "f0 median {median}");
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_wav(b"not a wav").is_err());
        assert!(parse_wav(&[0u8; 64]).is_err());
    }
}
