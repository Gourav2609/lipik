//! Takes the call out of the microphone recording. Without headphones the
//! microphone also hears the call from the speakers. system.wav holds what was
//! played, so its echo in mic.wav can be predicted and subtracted before the
//! final transcription.
//!
//! 1. The delay from system.wav to its echo in mic.wav: cross-correlation
//!    (GCC-PHAT) of 4 s windows every 15 s. Each 20 s block uses the delays
//!    found within a minute of it, so clocks that drift apart or an output
//!    device switched mid-call are followed.
//! 2. Per block, a filter per frequency over the last 8 frames (the room's
//!    echo tail) from the call audio to the mic, fitted by least squares over
//!    the block and its neighbours. The user's voice is unrelated to the call
//!    audio, so it averages out of the fit and survives the subtraction, also
//!    while both talk.
//! 3. What the filter misses (speaker distortion) is turned down per frequency
//!    where the predicted echo is strong next to what is left.

use anyhow::{bail, Result};
use hound::{SampleFormat, WavReader};
use rustfft::num_complex::{Complex32, Complex64};
use rustfft::{Fft, FftPlanner};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use super::audio::{create_wav, RATE};

const FFT_LEN: usize = 512;
const HOP: usize = FFT_LEN / 2;
const BINS: usize = FFT_LEN / 2 + 1;
const TAPS: usize = 8;
/// The filter starts this many samples before the measured delay, so a delay
/// a few ms off still fits.
const LEAD: i64 = 128;
const BLOCK_FRAMES: usize = 1250;
/// Echo left after subtraction, as a share of the predicted echo, until it
/// has been measured.
const DEFAULT_LEAK: f32 = 0.01;
/// A frame where what is left after subtraction is at most this share of the
/// predicted echo has nobody talking in the room.
const NO_VOICE: f32 = 0.25;
const OVER_SUBTRACT: f32 = 3.0;
const MIN_GAIN: f32 = 0.03;
const SMOOTH: f32 = 0.6;

const DELAY_WINDOW: usize = 1 << 16;
const DELAY_STEP: u64 = 15 * RATE as u64;
const NEARBY: u64 = 60 * RATE as u64;
/// A delay found further away than this is not used.
const STALE: u64 = 120 * RATE as u64;
/// How far either side of the last delay a window that doesn't match is
/// searched again.
const WIDE: i64 = 2 * RATE as i64;
/// Time from the call audio being tapped to its sound reaching the mic.
const MIN_LATENCY_MS: i64 = -50;
const MAX_LATENCY_MS: i64 = 800;
/// A correlation peak this many times the average is an echo; unrelated audio
/// peaks at about 5.
const MIN_PROMINENCE: f32 = 20.0;

trait Track {
    fn len(&self) -> u64;
    /// Samples from `start` on, zeros before the start and after the end.
    fn read_at(&mut self, start: i64, out: &mut [f32]) -> Result<()>;
}

#[cfg(test)]
impl Track for &[f32] {
    fn len(&self) -> u64 {
        <[f32]>::len(self) as u64
    }

    fn read_at(&mut self, start: i64, out: &mut [f32]) -> Result<()> {
        out.fill(0.0);
        let from = start.max(0);
        let to = (start + out.len() as i64).min(<[f32]>::len(self) as i64);
        if from < to {
            out[(from - start) as usize..(to - start) as usize]
                .copy_from_slice(&self[from as usize..to as usize]);
        }
        Ok(())
    }
}

struct WavTrack {
    reader: WavReader<BufReader<File>>,
    len: u64,
}

impl WavTrack {
    fn open(path: &Path) -> Result<Self> {
        let reader = WavReader::open(path)?;
        let spec = reader.spec();
        if spec.channels != 1
            || spec.bits_per_sample != 16
            || spec.sample_format != SampleFormat::Int
            || spec.sample_rate as usize != RATE
        {
            bail!("{} is not 16 kHz mono 16-bit", path.display());
        }
        let len = reader.duration() as u64;
        Ok(Self { reader, len })
    }
}

impl Track for WavTrack {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, start: i64, out: &mut [f32]) -> Result<()> {
        out.fill(0.0);
        let from = start.max(0);
        let to = (start + out.len() as i64).min(self.len as i64);
        if from >= to {
            return Ok(());
        }
        self.reader.seek(from as u32)?;
        let span = &mut out[(from - start) as usize..(to - start) as usize];
        for (o, s) in span.iter_mut().zip(self.reader.samples::<i16>()) {
            *o = s? as f32 / i16::MAX as f32;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DelayPoint {
    /// Sample (in mic.wav) at the middle of the window.
    at: u64,
    /// mic.wav sample `i` hears system.wav sample `i - lag`.
    lag: i64,
}

pub struct Report {
    /// From the call audio being tapped to its sound reaching the mic.
    pub delay_ms: f32,
    pub windows: usize,
}

/// The lags to search, given how much later than system.wav mic.wav started.
fn lag_range(mic_starts_later_ms: i64) -> (i64, i64) {
    let per_ms = (RATE / 1000) as i64;
    (
        (MIN_LATENCY_MS - mic_starts_later_ms) * per_ms,
        (MAX_LATENCY_MS - mic_starts_later_ms) * per_ms,
    )
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Cross-correlation of one mic window with the call audio over a range of
/// lags.
struct Correlator {
    planner: FftPlanner<f32>,
    heard: Vec<f32>,
    played: Vec<f32>,
    a: Vec<Complex32>,
    b: Vec<Complex32>,
}

enum Heard {
    /// The mic or the call is quiet here.
    Quiet,
    Unclear,
    Lag(i64),
}

impl Correlator {
    fn new() -> Self {
        Self {
            planner: FftPlanner::new(),
            heard: vec![0.0; DELAY_WINDOW],
            played: Vec::new(),
            a: Vec::new(),
            b: Vec::new(),
        }
    }

    fn lag(
        &mut self,
        mic: &mut impl Track,
        system: &mut impl Track,
        start: u64,
        (lo, hi): (i64, i64),
    ) -> Result<Heard> {
        let span = (hi - lo) as usize;
        let n = (DELAY_WINDOW + span + 1).next_power_of_two();
        self.played.resize(DELAY_WINDOW + span, 0.0);
        mic.read_at(start as i64, &mut self.heard)?;
        system.read_at(start as i64 - hi, &mut self.played)?;
        if rms(&self.heard) < 1e-4 || rms(&self.played) < 1e-3 {
            return Ok(Heard::Quiet);
        }
        let fwd = self.planner.plan_fft_forward(n);
        let inv = self.planner.plan_fft_inverse(n);
        for (dst, src) in [(&mut self.a, &self.heard), (&mut self.b, &self.played)] {
            dst.clear();
            dst.resize(n, Complex32::default());
            for (d, s) in dst.iter_mut().zip(src.iter()) {
                d.re = *s;
            }
            fwd.process(dst);
        }
        for (x, y) in self.a.iter_mut().zip(&self.b) {
            let c = x.conj() * y;
            *x = c / (c.norm() + 1e-12);
        }
        inv.process(&mut self.a);
        let corr: Vec<f32> = self.a[..=span].iter().map(|c| c.re.abs()).collect();
        let mean = corr.iter().sum::<f32>() / corr.len() as f32;
        let (best, peak) = corr
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.total_cmp(y.1))
            .map(|(i, v)| (i, *v))
            .unwrap_or_default();
        Ok(if peak >= MIN_PROMINENCE * mean.max(f32::MIN_POSITIVE) {
            Heard::Lag(hi - best as i64)
        } else {
            Heard::Unclear
        })
    }
}

/// Where the mic clearly hears the call, and with what delay. Empty when it
/// never does (headphones, no call audio). A window that doesn't match within
/// `lags` is searched again 2 s either side of the last delay found, since a
/// glitch in either stream can shift the files by up to a second.
fn find_delays(
    mic: &mut impl Track,
    system: &mut impl Track,
    lags: (i64, i64),
) -> Result<Vec<DelayPoint>> {
    let mut correlator = Correlator::new();
    let mut points: Vec<DelayPoint> = Vec::new();
    let mut start = 0u64;
    while start == 0 || start + DELAY_WINDOW as u64 <= mic.len() {
        let mut heard = correlator.lag(mic, system, start, lags)?;
        if matches!(heard, Heard::Unclear) {
            let around = points.last().map_or((lags.0 + lags.1) / 2, |p| p.lag);
            heard = correlator.lag(mic, system, start, (around - WIDE, around + WIDE))?;
        }
        if let Heard::Lag(lag) = heard {
            points.push(DelayPoint {
                at: start + DELAY_WINDOW as u64 / 2,
                lag,
            });
        }
        start += DELAY_STEP;
    }
    Ok(points)
}

/// The delay to use at `at`: the middle delay found within a minute, else the
/// closest one found within two. None when the call was not heard near `at`.
fn lag_near(points: &[DelayPoint], at: u64) -> Option<i64> {
    let mut near: Vec<i64> = points
        .iter()
        .filter(|p| p.at.abs_diff(at) <= NEARBY)
        .map(|p| p.lag)
        .collect();
    if near.is_empty() {
        return points
            .iter()
            .filter(|p| p.at.abs_diff(at) <= STALE)
            .min_by_key(|p| p.at.abs_diff(at))
            .map(|p| p.lag);
    }
    near.sort_unstable();
    Some(near[near.len() / 2])
}

struct Spectra {
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: Vec<Complex32>,
}

impl Spectra {
    fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        Self {
            fwd: planner.plan_fft_forward(FFT_LEN),
            inv: planner.plan_fft_inverse(FFT_LEN),
            window: (0..FFT_LEN)
                .map(|i| (std::f32::consts::PI * i as f32 / FFT_LEN as f32).sin())
                .collect(),
            buf: vec![Complex32::default(); FFT_LEN],
        }
    }

    /// Frames `first..first + count` of `track` read `shift` samples early.
    /// Frame `t` covers samples `(t - 1) * HOP .. (t + 1) * HOP`.
    fn frames(
        &mut self,
        track: &mut impl Track,
        first: i64,
        count: usize,
        shift: i64,
    ) -> Result<Vec<Complex32>> {
        let mut samples = vec![0.0f32; (count + 1) * HOP];
        track.read_at((first - 1) * HOP as i64 - shift, &mut samples)?;
        let mut out = Vec::with_capacity(count * BINS);
        for t in 0..count {
            let piece = &samples[t * HOP..t * HOP + FFT_LEN];
            for ((b, s), w) in self.buf.iter_mut().zip(piece).zip(&self.window) {
                *b = Complex32::new(s * w, 0.0);
            }
            self.fwd.process(&mut self.buf);
            out.extend_from_slice(&self.buf[..BINS]);
        }
        Ok(out)
    }

    fn samples(&mut self, spectrum: &[Complex32], out: &mut [f32]) {
        self.buf[..BINS].copy_from_slice(spectrum);
        for i in BINS..FFT_LEN {
            self.buf[i] = self.buf[FFT_LEN - i].conj();
        }
        self.inv.process(&mut self.buf);
        for ((o, b), w) in out.iter_mut().zip(&self.buf).zip(&self.window) {
            *o = b.re * w / FFT_LEN as f32;
        }
    }
}

/// One block's spectra with the call audio shifted by `lag`, and the sums the
/// least-squares fit needs.
struct Block {
    index: usize,
    lag: i64,
    frames: usize,
    mic: Vec<Complex32>,
    /// `TAPS - 1` frames of history, then one per mic frame.
    call: Vec<Complex32>,
    call_call: Vec<Complex64>,
    call_mic: Vec<Complex64>,
}

impl Block {
    fn read(
        index: usize,
        lag: i64,
        total: usize,
        mic: &mut impl Track,
        system: &mut impl Track,
        spectra: &mut Spectra,
    ) -> Result<Self> {
        let first = index * BLOCK_FRAMES;
        let frames = BLOCK_FRAMES.min(total - first);
        let mic = spectra.frames(mic, first as i64, frames, 0)?;
        let call = spectra.frames(
            system,
            first as i64 - (TAPS as i64 - 1),
            frames + TAPS - 1,
            lag - LEAD,
        )?;
        let mut call_call = vec![Complex64::default(); BINS * TAPS * TAPS];
        let mut call_mic = vec![Complex64::default(); BINS * TAPS];
        let mut x = [Complex64::default(); TAPS];
        for t in 0..frames {
            for f in 0..BINS {
                for (k, v) in x.iter_mut().enumerate() {
                    let c = call[(t + TAPS - 1 - k) * BINS + f];
                    *v = Complex64::new(c.re as f64, c.im as f64);
                }
                let y = mic[t * BINS + f];
                let y = Complex64::new(y.re as f64, y.im as f64);
                for k in 0..TAPS {
                    call_mic[f * TAPS + k] += x[k].conj() * y;
                    for j in k..TAPS {
                        call_call[(f * TAPS + k) * TAPS + j] += x[k].conj() * x[j];
                    }
                }
            }
        }
        Ok(Self {
            index,
            lag,
            frames,
            mic,
            call,
            call_call,
            call_mic,
        })
    }

    fn echo(&self, filter: &[Complex32], t: usize, f: usize) -> Complex32 {
        (0..TAPS)
            .map(|k| filter[f * TAPS + k] * self.call[(t + TAPS - 1 - k) * BINS + f])
            .sum()
    }
}

/// Solves `a x = b` for Hermitian positive definite `a` (only its upper half
/// is read), leaving `x` in `b`.
fn solve(a: &mut [Complex64; TAPS * TAPS], b: &mut [Complex64; TAPS]) -> bool {
    for i in 0..TAPS {
        for j in 0..i {
            a[i * TAPS + j] = a[j * TAPS + i].conj();
        }
    }
    for j in 0..TAPS {
        let d = a[j * TAPS + j].re - (0..j).map(|k| a[j * TAPS + k].norm_sqr()).sum::<f64>();
        if d <= 0.0 || !d.is_finite() {
            return false;
        }
        let d = d.sqrt();
        a[j * TAPS + j] = Complex64::new(d, 0.0);
        for i in j + 1..TAPS {
            let s: Complex64 = a[i * TAPS + j]
                - (0..j)
                    .map(|k| a[i * TAPS + k] * a[j * TAPS + k].conj())
                    .sum::<Complex64>();
            a[i * TAPS + j] = s / d;
        }
    }
    for i in 0..TAPS {
        let s = b[i] - (0..i).map(|k| a[i * TAPS + k] * b[k]).sum::<Complex64>();
        b[i] = s / a[i * TAPS + i].re;
    }
    for i in (0..TAPS).rev() {
        let s = b[i]
            - (i + 1..TAPS)
                .map(|k| a[k * TAPS + i].conj() * b[k])
                .sum::<Complex64>();
        b[i] = s / a[i * TAPS + i].re;
    }
    true
}

fn fit(window: &[&Block]) -> Vec<Complex32> {
    let mut filter = vec![Complex32::default(); BINS * TAPS];
    for f in 0..BINS {
        let mut a = [Complex64::default(); TAPS * TAPS];
        let mut r = [Complex64::default(); TAPS];
        for b in window {
            for k in 0..TAPS {
                r[k] += b.call_mic[f * TAPS + k];
                for j in k..TAPS {
                    a[k * TAPS + j] += b.call_call[(f * TAPS + k) * TAPS + j];
                }
            }
        }
        let trace: f64 = (0..TAPS).map(|k| a[k * TAPS + k].re).sum();
        let load = 1e-3 * trace / TAPS as f64 + 1e-12;
        for k in 0..TAPS {
            a[k * TAPS + k].re += load;
        }
        if solve(&mut a, &mut r) {
            for k in 0..TAPS {
                filter[f * TAPS + k] = Complex32::new(r[k].re as f32, r[k].im as f32);
            }
        }
    }
    filter
}

/// Per frequency, the share of the predicted echo still there after
/// subtraction, updated in `leak`: the middle ratio over the frames where the
/// call is loud and nobody talks in the room (what is left is well under the
/// predicted echo). Frequencies without enough such frames keep their last
/// value, so the user talking over the call is never counted as echo.
fn update_leak(window: &[&Block], filter: &[Complex32], leak: &mut [f32]) {
    let mut echo = Vec::new();
    let mut quiet_room = Vec::new();
    for b in window {
        for t in 0..b.frames {
            let (mut played, mut left) = (0.0f32, 0.0f32);
            for f in 0..BINS {
                let e = b.echo(filter, t, f);
                let z = b.mic[t * BINS + f] - e;
                played += e.norm_sqr();
                left += z.norm_sqr();
                echo.push((e.norm_sqr(), z.norm_sqr()));
            }
            if played > 0.0 && left <= NO_VOICE * played {
                quiet_room.push(echo.len() / BINS - 1);
            }
        }
    }
    let mut loudness = Vec::new();
    let mut ratios = Vec::new();
    for (f, value) in leak.iter_mut().enumerate() {
        let clean = quiet_room.iter().map(|t| echo[t * BINS + f]);
        loudness.clear();
        loudness.extend(clean.clone().map(|(e, _)| e));
        if loudness.len() < 20 {
            continue;
        }
        let k = loudness.len() * 3 / 5;
        let loud = *loudness.select_nth_unstable_by(k, f32::total_cmp).1;
        ratios.clear();
        ratios.extend(
            clean
                .filter(|(e, _)| *e >= loud && *e > 0.0)
                .map(|(e, z)| z / e),
        );
        if ratios.len() >= 20 {
            let mid = ratios.len() / 2;
            *value = ratios
                .select_nth_unstable_by(mid, f32::total_cmp)
                .1
                .min(1.0);
        }
    }
}

/// mic minus the call audio it heard, passed to `write` in order, as many
/// samples as `mic` has.
fn subtract(
    mic: &mut impl Track,
    system: &mut impl Track,
    delays: &[DelayPoint],
    mut write: impl FnMut(&[f32]) -> Result<()>,
) -> Result<()> {
    let len = mic.len();
    let total = (len as usize).div_ceil(HOP) + 1;
    let blocks = total.div_ceil(BLOCK_FRAMES);
    let mut spectra = Spectra::new();
    let mut cache: VecDeque<Block> = VecDeque::new();
    let mut leak = vec![DEFAULT_LEAK; BINS];
    let mut left = vec![0.0f32; BINS];
    let mut residual = vec![0.0f32; BINS];
    let mut spectrum = vec![Complex32::default(); BINS];
    let mut frame = vec![0.0f32; FFT_LEN];
    let mut tail = vec![0.0f32; HOP];
    for b in 0..blocks {
        let middle = |j: usize| ((j * BLOCK_FRAMES + BLOCK_FRAMES / 2) * HOP) as u64;
        let heard = lag_near(delays, middle(b));
        let lag = heard.unwrap_or(0);
        let range = (b.saturating_sub(1)..(b + 2).min(blocks))
            .filter(|&j| j == b || lag_near(delays, middle(j)) == heard)
            .collect::<Vec<_>>();
        cache.retain(|k| k.index + 1 >= b && k.lag == lag);
        for &j in &range {
            if !cache.iter().any(|k| k.index == j) {
                cache.push_back(Block::read(j, lag, total, mic, system, &mut spectra)?);
            }
        }
        let window: Vec<&Block> = cache.iter().filter(|k| range.contains(&k.index)).collect();
        let filter = match heard {
            Some(_) => {
                let filter = fit(&window);
                update_leak(&window, &filter, &mut leak);
                filter
            }
            None => vec![Complex32::default(); BINS * TAPS],
        };
        let block = window.iter().find(|k| k.index == b).expect("block read");
        for t in 0..block.frames {
            for f in 0..BINS {
                let e = block.echo(&filter, t, f);
                let z = block.mic[t * BINS + f] - e;
                left[f] = SMOOTH * left[f] + (1.0 - SMOOTH) * z.norm_sqr();
                residual[f] = SMOOTH * residual[f] + (1.0 - SMOOTH) * leak[f] * e.norm_sqr();
                let gain = 1.0 - OVER_SUBTRACT * residual[f] / (left[f] + 1e-12);
                spectrum[f] = z * gain.clamp(MIN_GAIN, 1.0);
            }
            spectra.samples(&spectrum, &mut frame);
            for (s, x) in tail.iter_mut().zip(&frame) {
                *s += x;
            }
            let index = (b * BLOCK_FRAMES + t) as u64;
            if index > 0 {
                let start = (index - 1) * HOP as u64;
                let n = len.saturating_sub(start).min(HOP as u64) as usize;
                if n > 0 {
                    write(&tail[..n])?;
                }
            }
            tail.copy_from_slice(&frame[HOP..]);
        }
    }
    Ok(())
}

/// Writes mic.wav without the call audio it heard to `out`. None, and no file,
/// when the mic never heard the call.
pub fn remove_call_echo(
    mic: &Path,
    system: &Path,
    mic_starts_later_ms: i64,
    out: &Path,
) -> Result<Option<Report>> {
    let mut heard = WavTrack::open(mic)?;
    let mut played = WavTrack::open(system)?;
    let delays = find_delays(&mut heard, &mut played, lag_range(mic_starts_later_ms))?;
    if delays.is_empty() {
        return Ok(None);
    }
    let mut writer = create_wav(out)?;
    subtract(&mut heard, &mut played, &delays, |samples| {
        for s in samples {
            writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        Ok(())
    })?;
    writer.finalize()?;
    let mut lags: Vec<i64> = delays.iter().map(|p| p.lag).collect();
    lags.sort_unstable();
    Ok(Some(Report {
        delay_ms: lags[lags.len() / 2] as f32 * 1000.0 / RATE as f32 + mic_starts_later_ms as f32,
        windows: delays.len(),
    }))
}

/// An echo-likeness at least this high means a mic piece is the call heard
/// from the speakers. On two recorded calls the mic during call lines scored
/// 0.37 to 0.78 for 9 in 10 lines (median 0.5), the user talking alone 0 to
/// 0.12.
pub const ECHO_LIKENESS: f32 = 0.3;

/// Taking the call out of a piece of echo leaves at most this share of its
/// energy; with the user talking over the call much more is left.
pub const ECHO_REMOVED_SHARE: f32 = 0.25;

/// Whether a mic piece is the call heard from the speakers: taking the call
/// out left little of it (`cleaned` against `raw`, see
/// [`ECHO_REMOVED_SHARE`]) and `raw` looks like `call` ([`echo_likeness`]),
/// so the user talking over the call is kept.
pub fn is_echo_piece(raw: &[f32], cleaned: &[f32], call: &[f32]) -> bool {
    let energy = |x: &[f32]| x.iter().map(|&v| v as f64 * v as f64).sum::<f64>();
    energy(cleaned) <= ECHO_REMOVED_SHARE as f64 * energy(raw)
        && echo_likeness(raw, call) >= ECHO_LIKENESS
}

/// How much `mic` looks like a stretch of `call`: the largest correlation
/// (-1..1, sign ignored) between `mic` and any stretch of `call` as long as
/// it, so `call` is read longer by the delays to try. 0 when either is
/// silent or `call` is shorter than `mic`.
pub fn echo_likeness(mic: &[f32], call: &[f32]) -> f32 {
    let (n, m) = (mic.len(), call.len());
    let mic_energy: f64 = mic.iter().map(|&x| x as f64 * x as f64).sum();
    if n == 0 || m < n || mic_energy <= 0.0 {
        return 0.0;
    }
    let len = (n + m).next_power_of_two();
    let mut planner = FftPlanner::<f32>::new();
    let (fwd, inv) = (planner.plan_fft_forward(len), planner.plan_fft_inverse(len));
    let spectrum = |x: &[f32]| {
        let mut buf: Vec<Complex32> = x.iter().map(|&v| Complex32::new(v, 0.0)).collect();
        buf.resize(len, Complex32::default());
        fwd.process(&mut buf);
        buf
    };
    let a = spectrum(mic);
    let mut cross = spectrum(call);
    for (c, a) in cross.iter_mut().zip(&a) {
        *c *= a.conj();
    }
    inv.process(&mut cross);
    let mut window: f64 = call[..n].iter().map(|&x| x as f64 * x as f64).sum();
    let mut best = 0.0f32;
    for shift in 0..=m - n {
        if shift > 0 {
            let (out, add) = (call[shift - 1] as f64, call[shift + n - 1] as f64);
            window = (window - out * out + add * add).max(0.0);
        }
        if window > 1e-9 {
            let dot = cross[shift].re as f64 / len as f64;
            best = best.max((dot.abs() / (mic_energy * window).sqrt()) as f32);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        }
    }

    /// Noise switched on and off a few times a second, like speech.
    fn talk(seed: u64, seconds: f32) -> Vec<f32> {
        let mut noise = Noise(seed);
        let rate = 2.0 + (seed % 5) as f32 * 0.4;
        (0..(seconds * RATE as f32) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let on = (t * rate * std::f32::consts::TAU).sin().max(0.0);
                0.3 * on * noise.next()
            })
            .collect()
    }

    fn quiet(x: &mut [f32], from_s: f32, to_s: f32) {
        let end = ((to_s * RATE as f32) as usize).min(x.len());
        x[(from_s * RATE as f32) as usize..end].fill(0.0);
    }

    /// What the mic hears of `call`: `lag` samples late, a little room echo,
    /// quieter, and slightly distorted by the speaker.
    fn heard(call: &[f32], lag: i64) -> Vec<f32> {
        (0..call.len() as i64)
            .map(|i| {
                let at = |d: i64| call.get((i - d) as usize).copied().unwrap_or(0.0);
                let x = 0.5 * at(lag) + 0.2 * at(lag + 53) - 0.1 * at(lag + 190);
                (1.5 * x).tanh() / 1.5
            })
            .collect()
    }

    fn mix(parts: &[&[f32]]) -> Vec<f32> {
        (0..parts[0].len())
            .map(|i| parts.iter().map(|p| p[i]).sum())
            .collect()
    }

    fn power_db(x: &[f32], from_s: f32, to_s: f32) -> f32 {
        let piece = &x[(from_s * RATE as f32) as usize..(to_s * RATE as f32) as usize];
        10.0 * (piece.iter().map(|v| v * v).sum::<f32>() / piece.len() as f32 + 1e-12).log10()
    }

    fn run(mic: &[f32], call: &[f32]) -> (Vec<DelayPoint>, Vec<f32>) {
        let delays = find_delays(&mut &mic[..], &mut &call[..], lag_range(0)).unwrap();
        let mut out = Vec::new();
        if !delays.is_empty() {
            subtract(&mut &mic[..], &mut &call[..], &delays, |s| {
                out.extend_from_slice(s);
                Ok(())
            })
            .unwrap();
        }
        (delays, out)
    }

    #[test]
    fn a_piece_is_echo_only_when_taking_the_call_out_left_little() {
        let call = talk(7, 12.0);
        let user = talk(11, 12.0);
        let echo = heard(&call, 592);
        let (from, to) = (4 * RATE, 8 * RATE);
        let played = &call[from - 592 - RATE..to - 592 + RATE];
        let piece = |x: &[f32]| x[from..to].to_vec();
        let scale = rms(&piece(&user)) / rms(&piece(&echo));
        let loud_echo: Vec<f32> = echo.iter().map(|x| x * scale).collect();
        let both = mix(&[&user, &loud_echo]);
        assert!(echo_likeness(&piece(&both), played) >= ECHO_LIKENESS);
        assert!(
            !is_echo_piece(&piece(&both), &piece(&user), played),
            "the user over the call at the same loudness is kept"
        );
        let left: Vec<f32> = echo.iter().map(|x| 0.1 * x).collect();
        assert!(is_echo_piece(&piece(&echo), &piece(&left), played));
        let quiet_user: Vec<f32> = user.iter().map(|x| 0.1 * x).collect();
        assert!(!is_echo_piece(&piece(&user), &piece(&quiet_user), played));
    }

    #[test]
    fn tells_a_piece_of_echo_from_the_user_talking() {
        let call = talk(7, 12.0);
        let user = talk(11, 12.0);
        let echo = heard(&call, 592);
        let (from, to) = (4 * RATE, 8 * RATE);
        let search = RATE;
        let played = &call[from - 592 - search..to - 592 + search];
        assert!(echo_likeness(&echo[from..to], played) >= ECHO_LIKENESS);
        assert!(echo_likeness(&user[from..to], played) < ECHO_LIKENESS);
        let soft_echo: Vec<f32> = echo.iter().map(|x| 0.2 * x).collect();
        let user_over_echo = mix(&[&user, &soft_echo]);
        assert!(echo_likeness(&user_over_echo[from..to], played) < ECHO_LIKENESS);
        assert_eq!(echo_likeness(&[0.0; 100], &[0.0; 300]), 0.0);
        assert_eq!(
            echo_likeness(&user[..300], &call[..100]),
            0.0,
            "call too short"
        );
    }

    #[test]
    fn finds_the_delay_of_the_echo() {
        let call = talk(7, 32.0);
        let user = talk(11, 32.0);
        let mic = mix(&[&heard(&call, 592), &user]);
        let delays = find_delays(&mut &mic[..], &mut &call[..], lag_range(0)).unwrap();
        assert_eq!(delays.len(), 2);
        assert!(
            delays.iter().all(|p| (p.lag - 592).abs() <= 1),
            "{delays:?}"
        );
    }

    #[test]
    fn hears_no_echo_with_headphones() {
        let call = talk(7, 32.0);
        let mic = talk(11, 32.0);
        assert!(find_delays(&mut &mic[..], &mut &call[..], lag_range(0))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn follows_a_delay_that_changes() {
        let points = [
            DelayPoint { at: 0, lag: 500 },
            DelayPoint {
                at: 15 * RATE as u64,
                lag: 501,
            },
            DelayPoint {
                at: 30 * RATE as u64,
                lag: 3000,
            },
            DelayPoint {
                at: 200 * RATE as u64,
                lag: 4000,
            },
            DelayPoint {
                at: 215 * RATE as u64,
                lag: 4002,
            },
        ];
        assert_eq!(lag_near(&points, 10 * RATE as u64), Some(501));
        assert_eq!(lag_near(&points, 210 * RATE as u64), Some(4002));
        assert_eq!(lag_near(&points, 110 * RATE as u64), Some(3000));
        assert_eq!(lag_near(&points, 400 * RATE as u64), None);
    }

    #[test]
    fn removes_the_call_and_keeps_the_user() {
        let mut call = talk(7, 45.0);
        quiet(&mut call, 35.0, 45.0);
        let mut user = talk(11, 45.0);
        quiet(&mut user, 0.0, 20.0);
        quiet(&mut user, 30.0, 37.0);
        quiet(&mut user, 43.0, 45.0);
        let mic = mix(&[&heard(&call, 592), &user]);
        let (delays, out) = run(&mic, &call);
        assert!(!delays.is_empty());
        assert_eq!(out.len(), mic.len());

        let echo_only = power_db(&mic, 2.0, 18.0) - power_db(&out, 2.0, 18.0);
        assert!(echo_only > 20.0, "echo only turned down {echo_only:.1} dB");

        let user_alone = power_db(&out, 37.5, 42.5) - power_db(&mic, 37.5, 42.5);
        assert!(
            user_alone.abs() < 0.5,
            "user alone changed {user_alone:.1} dB"
        );

        let error: Vec<f32> = out.iter().zip(&user).map(|(o, u)| o - u).collect();
        let both = power_db(&user, 21.0, 29.0) - power_db(&error, 21.0, 29.0);
        assert!(
            both > 12.0,
            "user over the call kept at {both:.1} dB signal to error"
        );
    }

    /// Noise that never stops, louder and softer a few times a second.
    fn steady_talk(seed: u64, seconds: f32) -> Vec<f32> {
        let mut noise = Noise(seed);
        (0..(seconds * RATE as f32) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let level = 0.7
                    + 0.15 * (t * (2.0 + (seed % 5) as f32 * 0.4) * std::f32::consts::TAU).sin();
                0.3 * level * noise.next()
            })
            .collect()
    }

    /// How much of `user` is in `out` between the two times, as a share of
    /// its level.
    fn kept(out: &[f32], user: &[f32], from_s: f32, to_s: f32) -> f32 {
        let range = (from_s * RATE as f32) as usize..(to_s * RATE as f32) as usize;
        let dot: f32 = out[range.clone()]
            .iter()
            .zip(&user[range.clone()])
            .map(|(o, u)| o * u)
            .sum();
        let own: f32 = user[range].iter().map(|u| u * u).sum();
        dot / own
    }

    #[test]
    fn keeps_the_user_talking_over_the_whole_call() {
        let call = steady_talk(7, 60.0);
        let echo = heard(&call, 592);
        let echo_rms = (echo.iter().map(|v| v * v).sum::<f32>() / echo.len() as f32).sqrt();
        for loudness in [1.0f32, 2.0] {
            let user = steady_talk(11, 60.0);
            let user_rms = (user.iter().map(|v| v * v).sum::<f32>() / user.len() as f32).sqrt();
            let user: Vec<f32> = user
                .iter()
                .map(|u| u * loudness * echo_rms / user_rms)
                .collect();
            let mic = mix(&[&echo, &user]);
            let (delays, out) = run(&mic, &call);
            assert!(!delays.is_empty());
            let share = kept(&out, &user, 5.0, 55.0);
            println!("user at {loudness}x the echo: kept {share:.3}");
            assert!(
                share >= 0.95,
                "user at {loudness}x the echo kept {share:.3}"
            );
        }
    }

    #[test]
    fn follows_the_call_audio_jumping_mid_meeting() {
        let call = talk(7, 120.0);
        let jumped = 592 - 9600;
        let half = 60 * RATE;
        let before = heard(&call, 592);
        let after = heard(&call, jumped);
        let echo: Vec<f32> = before[..half]
            .iter()
            .chain(&after[half..])
            .copied()
            .collect();
        let (delays, out) = run(&echo, &call);
        let last = delays.last().unwrap();
        assert_eq!(last.lag, jumped, "{delays:?}");
        assert!(delays.iter().any(|p| p.lag == 592));
        for (from, to) in [(5.0, 55.0), (65.0, 115.0)] {
            let down = power_db(&echo, from, to) - power_db(&out, from, to);
            assert!(down > 20.0, "{from}-{to} s turned down {down:.1} dB");
        }
    }

    #[test]
    fn writes_a_cleaned_file_only_when_the_call_was_heard() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, samples: &[f32]| {
            let path = dir.path().join(name);
            let mut w = create_wav(&path).unwrap();
            for s in samples {
                w.write_sample((s * i16::MAX as f32) as i16).unwrap();
            }
            w.finalize().unwrap();
            path
        };
        let call = talk(7, 20.0);
        let system = write("system.wav", &call);
        let echoed = write("echoed.wav", &heard(&call, 800));
        let separate = write("separate.wav", &talk(11, 20.0));

        let out = dir.path().join("out.wav");
        let report = remove_call_echo(&echoed, &system, 20, &out)
            .unwrap()
            .unwrap();
        assert!((report.delay_ms - 70.0).abs() < 0.2, "{}", report.delay_ms);
        let reader = WavReader::open(&out).unwrap();
        assert_eq!(reader.duration(), 20 * RATE as u32);

        let none = dir.path().join("none.wav");
        assert!(remove_call_echo(&separate, &system, 20, &none)
            .unwrap()
            .is_none());
        assert!(!none.exists());
    }

    /// Real-call check, run by hand:
    /// LIPIK_ECHO_DIR=<meeting dir> LIPIK_ECHO_LATER_MS=<mic minus system
    /// offset> LIPIK_ECHO_OUT=<out.wav> cargo test --lib real_echo -- --ignored
    /// --nocapture
    #[test]
    #[ignore]
    fn real_echo() {
        let dir = std::path::PathBuf::from(std::env::var("LIPIK_ECHO_DIR").unwrap());
        let later: i64 = std::env::var("LIPIK_ECHO_LATER_MS")
            .unwrap()
            .parse()
            .unwrap();
        let out = std::env::var("LIPIK_ECHO_OUT").unwrap();
        let started = std::time::Instant::now();
        let report = remove_call_echo(
            &dir.join("mic.wav"),
            &dir.join("system.wav"),
            later,
            Path::new(&out),
        )
        .unwrap();
        match report {
            Some(r) => println!(
                "REAL delay_ms={:.2} windows={} run_s={:.1}",
                r.delay_ms,
                r.windows,
                started.elapsed().as_secs_f32()
            ),
            None => println!("REAL no echo"),
        }
    }
}
