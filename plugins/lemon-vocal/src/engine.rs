// SPDX-License-Identifier: GPL-3.0-or-later
//! Statically linked WORLD vocal analysis/resynthesis engine, SnapTune live pitch correction, audio I/O, and WSOLA time-warping.
use os_model::vocal::{Edit, Note, STEP};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    io::{Cursor, Write},
    path::Path,
    ptr::NonNull,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use symphonia::core::{
    codecs::{audio::AudioDecoderOptions, registry::CodecRegistry},
    errors::Error as SymError,
    formats::{
        FormatOptions, TrackType,
        probe::{Hint, Probe},
    },
    io::MediaSourceStream,
    meta::MetadataOptions,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Sample {
    pub fn frames(&self) -> usize {
        self.left.len()
    }
}

fn codecs() -> &'static CodecRegistry {
    static REG: OnceLock<CodecRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let mut r = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut r);
        r
    })
}

fn probe() -> &'static Probe {
    static PROBE: OnceLock<Probe> = OnceLock::new();
    PROBE.get_or_init(|| {
        let mut p = Probe::default();
        symphonia::default::register_enabled_formats(&mut p);
        p
    })
}

pub fn load_sample(path: &Path) -> Result<Arc<Sample>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ext = path.extension().and_then(|e| e.to_str());
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = ext {
        hint.with_extension(ext);
    }
    let mut format = probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("No audio track found")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("No audio codec parameters")?;
    let mp3 = format.format_info().short_name == "mp3";
    let mut decoder = codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default().gapless(!mp3))
        .map_err(|e| e.to_string())?;

    let mut rate = 0;
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut buf: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(a) => a,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        rate = audio.spec().rate();
        let ch = audio.spec().channels().count().max(1);
        buf.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut buf);
        for frame in buf.chunks_exact(ch) {
            left.push(frame[0]);
            right.push(if ch > 1 { frame[1] } else { frame[0] });
        }
    }
    if rate == 0 || left.is_empty() {
        return Err("No audio decoded".into());
    }
    Ok(Arc::new(Sample { rate, left, right }))
}

pub fn write_wav(path: &Path, s: &Sample) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut w = std::io::BufWriter::with_capacity(
        256 * 1024,
        std::fs::File::create(path).map_err(|e| e.to_string())?,
    );
    let frames = s.frames() as u64;
    let data_len = u32::try_from(frames * 8).map_err(|_| "Audio too long for WAV")?;
    let block_align: u16 = 8;
    w.write_all(b"RIFF").map_err(|e| e.to_string())?;
    w.write_all(&(36 + data_len).to_le_bytes())
        .map_err(|e| e.to_string())?;
    w.write_all(b"WAVEfmt ").map_err(|e| e.to_string())?;
    w.write_all(&16u32.to_le_bytes()).map_err(|e| e.to_string())?;
    w.write_all(&3u16.to_le_bytes()).map_err(|e| e.to_string())?;
    w.write_all(&2u16.to_le_bytes()).map_err(|e| e.to_string())?;
    w.write_all(&s.rate.to_le_bytes())
        .map_err(|e| e.to_string())?;
    w.write_all(&(s.rate * u32::from(block_align)).to_le_bytes())
        .map_err(|e| e.to_string())?;
    w.write_all(&block_align.to_le_bytes())
        .map_err(|e| e.to_string())?;
    w.write_all(&32u16.to_le_bytes()).map_err(|e| e.to_string())?;
    w.write_all(b"data").map_err(|e| e.to_string())?;
    w.write_all(&data_len.to_le_bytes())
        .map_err(|e| e.to_string())?;

    let mut bytes = Vec::with_capacity(65536 * 8);
    for (a, b) in s.left.chunks(65536).zip(s.right.chunks(65536)) {
        bytes.clear();
        for (&l, &r) in a.iter().zip(b) {
            bytes.extend_from_slice(&l.to_le_bytes());
            bytes.extend_from_slice(&r.to_le_bytes());
        }
        w.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())?;
    Ok(())
}

// --- SnapTune C ABI ---
unsafe extern "C" {
    fn lemon_tune_new(rate: f64) -> *mut c_void;
    fn lemon_tune_delete(p: *mut c_void);
    fn lemon_tune_reset(p: *mut c_void);
    fn lemon_tune_process(
        p: *mut c_void,
        buf: *mut f32,
        frames: i32,
        strength: f32,
        key: i32,
        scale: i32,
        voice: i32,
        learn: bool,
    );
}

pub struct SnapTune {
    ptr: NonNull<c_void>,
    pub amount: f32,
    pub key: u8,
    pub scale: u8,
    pub voice: u8,
    pub enabled: bool,
    active: bool,
}

unsafe impl Send for SnapTune {}

impl SnapTune {
    pub fn new(rate: u32) -> Self {
        let ptr = NonNull::new(unsafe { lemon_tune_new(rate.max(1) as f64) })
            .expect("SnapTune allocation");
        Self {
            ptr,
            amount: 1.0,
            key: 0,
            scale: 0,
            voice: 0,
            enabled: true,
            active: false,
        }
    }

    pub fn configure(&mut self, amount: f32, key: u8, scale: u8, voice: u8, enabled: bool) {
        if self.enabled != enabled {
            self.active = false;
        }
        self.enabled = enabled;
        self.amount = amount;
        self.key = key;
        self.scale = scale;
        self.voice = voice;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        if !self.enabled || self.amount <= 0.0 {
            self.active = false;
            return;
        }
        if !self.active {
            unsafe { lemon_tune_reset(self.ptr.as_ptr()) };
            self.active = true;
        }
        for part in buf.chunks_mut(8192) {
            unsafe {
                lemon_tune_process(
                    self.ptr.as_ptr(),
                    part.as_mut_ptr(),
                    (part.len() / 2) as i32,
                    self.amount.clamp(0.0, 1.0),
                    self.key.into(),
                    self.scale.into(),
                    self.voice.into(),
                    false,
                );
            }
        }
    }
}

impl Drop for SnapTune {
    fn drop(&mut self) {
        unsafe { lemon_tune_delete(self.ptr.as_ptr()) }
    }
}

// --- WORLD C ABI ---
unsafe extern "C" {
    fn lemon_vocal_version() -> i32;
    fn lemon_vocal_analyze(
        l: *const f32,
        r: *const f32,
        n: i32,
        fs: i32,
        out: *mut f64,
        capacity: i32,
    ) -> i32;
    fn lemon_vocal_render(
        l: *const f32,
        r: *const f32,
        n: i32,
        fs: i32,
        original: *const f64,
        corrected: *const f64,
        frames: i32,
        formant: f64,
        outl: *mut f32,
        outr: *mut f32,
    ) -> i32;
}

static NATIVE_CALL: Mutex<()> = Mutex::new(());

pub fn version() -> i32 {
    unsafe { lemon_vocal_version() }
}

pub fn validate(s: &Sample) -> Result<(), String> {
    if s.left.len() != s.right.len()
        || s.frames() == 0
        || !(16000..=192000).contains(&s.rate)
        || s.frames() > s.rate as usize * 600
        || s.frames() > 33_554_432
    {
        return Err("Choose a vocal recording up to 10 minutes at 16–192 kHz".into());
    }
    Ok(())
}

pub fn sample_hash(s: &Sample) -> String {
    let mut h = Sha256::new();
    h.update(s.rate.to_le_bytes());
    h.update((s.frames() as u64).to_le_bytes());
    let stride = (s.frames() / 4096).max(1);
    for i in (0..s.frames()).step_by(stride) {
        h.update(s.left[i].to_le_bytes());
        h.update(s.right[i].to_le_bytes());
    }
    format!("{:x}", h.finalize())
}

pub fn edit_key(d: &Edit) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                "plugin-v1",
                &d.source_hash,
                d.rate,
                d.frames,
                &d.notes,
                &d.curve,
                &d.timing,
                d.enabled,
                d.strength,
                d.formant,
            ))
            .unwrap_or_default()
        )
    )
}

pub fn analyze(s: &Sample) -> Result<Vec<f32>, String> {
    let _guard = NATIVE_CALL.lock().unwrap_or_else(|e| e.into_inner());
    validate(s)?;
    let mut f0 = vec![0.0f64; (s.frames() as f64 / s.rate as f64 / STEP).floor() as usize + 1];
    let n = unsafe {
        lemon_vocal_analyze(
            s.left.as_ptr(),
            s.right.as_ptr(),
            s.frames() as i32,
            s.rate as i32,
            f0.as_mut_ptr(),
            f0.len() as i32,
        )
    };
    if n <= 0 || n as usize != f0.len() {
        return Err("Vocal pitch analysis failed".into());
    }
    Ok(f0.into_iter().map(|v| v as f32).collect())
}

pub fn segment(f0: &[f32]) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut i = 0;
    while i < f0.len() {
        if f0[i] <= 0.0 {
            i += 1;
            continue;
        }
        let start = i;
        let mut values = vec![69.0 + 12.0 * (f0[i] / 440.0).log2()];
        i += 1;
        while i < f0.len() && f0[i] > 0.0 {
            let p = 69.0 + 12.0 * (f0[i] / 440.0).log2();
            let base = values.iter().rev().take(8).copied().sum::<f32>()
                / values.len().min(8) as f32;
            if (p - base).abs() > 0.85 && i - start >= 8 {
                break;
            }
            values.push(p);
            i += 1;
        }
        if i - start >= 6 {
            values.sort_by(f32::total_cmp);
            let base = values[values.len() / 2];
            notes.push(Note::new(start as u32, i as u32, base));
        }
    }
    notes
}

pub fn analyze_to_edit(s: &Sample, source_path: String) -> Result<Edit, String> {
    let f0 = analyze(s)?;
    let notes = segment(&f0);
    let duration = s.frames() as f64 / s.rate as f64;
    let source_hash = sample_hash(s);
    let mut d = Edit {
        channel: 0,
        source: source_path,
        source_hash,
        rendered: String::new(),
        render_key: String::new(),
        rate: s.rate,
        frames: s.frames() as u32,
        f0,
        notes,
        curve: vec![],
        timing: vec![(0.0, 0.0), (duration, duration)],
        enabled: true,
        strength: 1.0,
        formant: 0.0,
    };
    d.render_key = edit_key(&d);
    Ok(d)
}

/// Pitch-preserving WSOLA (Waveform Similarity Overlap-Add) piecewise time warp.
pub fn warp(
    left: &[f32],
    right: &[f32],
    rate: u32,
    points: &[(usize, usize)],
) -> Result<(Vec<f32>, Vec<f32>), String> {
    let n = left.len().min(right.len());
    if points.len() < 2
        || points.len() > 1024
        || points[0].1 != 0
        || points.last().unwrap().0 > n
        || points
            .windows(2)
            .any(|w| w[1].0 <= w[0].0 || w[1].1 <= w[0].1)
    {
        return Err("Warp anchors must stay in order and inside the sample".into());
    }
    let out_frames = points.last().unwrap().1;
    if out_frames == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    let win = ((rate as usize * 25) / 1000).clamp(128, 4096);
    let hop = win / 2;
    let search = (win / 4).max(16);

    let mut hann = vec![0.0f32; win];
    for (i, w) in hann.iter_mut().enumerate() {
        *w = 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / win as f32).cos());
    }

    let map_dest_to_src = |dst: usize| -> usize {
        let idx = points
            .partition_point(|p| p.1 < dst)
            .clamp(1, points.len() - 1);
        let (s0, d0) = points[idx - 1];
        let (s1, d1) = points[idx];
        if d1 <= d0 {
            return s0.min(n.saturating_sub(1));
        }
        let t = (dst.saturating_sub(d0)) as f64 / (d1 - d0) as f64;
        (s0 as f64 + t * (s1 - s0) as f64)
            .round()
            .clamp(0.0, n.saturating_sub(1) as f64) as usize
    };

    let mut out_l = vec![0.0f32; out_frames + win];
    let mut out_r = vec![0.0f32; out_frames + win];
    let mut norm = vec![0.0f32; out_frames + win];

    let mut prev_src_end: Option<usize> = None;
    let mut dst_pos = 0;
    while dst_pos < out_frames {
        let target_src = map_dest_to_src(dst_pos);
        let best_src = if let Some(expected) = prev_src_end {
            let lo = target_src.saturating_sub(search);
            let hi = (target_src + search).min(n.saturating_sub(win).max(0));
            if hi > lo && expected + hop <= n {
                let mut best = target_src.min(n.saturating_sub(win));
                let mut best_corr = f32::MIN;
                let step = (search / 16).max(1);
                for cand in (lo..=hi).step_by(step) {
                    let mut corr = 0.0f32;
                    for k in (0..hop.min(n - cand).min(n - expected)).step_by(4) {
                        corr += left[cand + k] * left[expected + k];
                    }
                    if corr > best_corr {
                        best_corr = corr;
                        best = cand;
                    }
                }
                best
            } else {
                target_src.min(n.saturating_sub(win))
            }
        } else {
            target_src.min(n.saturating_sub(win))
        };

        for k in 0..win {
            if best_src + k < n && dst_pos + k < out_l.len() {
                let w = hann[k];
                out_l[dst_pos + k] += left[best_src + k] * w;
                out_r[dst_pos + k] += right[best_src + k] * w;
                norm[dst_pos + k] += w;
            }
        }
        prev_src_end = Some(best_src + hop);
        dst_pos += hop;
    }

    for i in 0..out_frames {
        if norm[i] > 1e-5 {
            out_l[i] /= norm[i];
            out_r[i] /= norm[i];
        }
    }
    out_l.truncate(out_frames);
    out_r.truncate(out_frames);
    Ok((out_l, out_r))
}

pub fn render_region(s: &Sample, d: &Edit) -> Result<(Vec<f32>, Vec<f32>), String> {
    let _guard = NATIVE_CALL.lock().unwrap_or_else(|e| e.into_inner());
    validate(s)?;
    if !d.valid() || d.frames as usize != s.frames() || d.rate != s.rate {
        return Err("Vocal source changed; reanalyse this recording".into());
    }
    if !d.changed() {
        return Ok((s.left.clone(), s.right.clone()));
    }
    let original: Vec<f64> = d.f0.iter().map(|v| f64::from(*v)).collect();
    let target = d.corrected();
    if target
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0 || *v > 4000.0)
    {
        return Err(
            "Requested pitch exceeds this engine's range. Reduce the pitch or modulation and retry."
                .into(),
        );
    }
    let (mut l, mut r) = (vec![0.0; s.frames()], vec![0.0; s.frames()]);
    let result = unsafe {
        lemon_vocal_render(
            s.left.as_ptr(),
            s.right.as_ptr(),
            s.frames() as i32,
            s.rate as i32,
            original.as_ptr(),
            target.as_ptr(),
            original.len() as i32,
            f64::from(d.formant),
            l.as_mut_ptr(),
            r.as_mut_ptr(),
        )
    };
    if result != 0 || l.iter().chain(&r).any(|v| !v.is_finite()) {
        return Err("Vocal rendering failed; previous audio retained".into());
    }
    for note in &d.notes {
        if note.formant.abs() <= 0.001 {
            continue;
        }
        let begin = (note.start as usize).saturating_sub(20);
        let end = (note.end as usize + 20).min(original.len());
        let start = (begin as f64 * STEP * s.rate as f64).round() as usize;
        let stop = ((end as f64 * STEP * s.rate as f64).round() as usize).min(s.frames());
        if stop <= start {
            continue;
        }
        let frames = (((stop - start) as f64 / s.rate as f64 / STEP).floor() as usize) + 1;
        let f: Vec<_> = (0..frames)
            .map(|i| original[(begin + i).min(original.len() - 1)])
            .collect();
        let t: Vec<_> = (0..frames)
            .map(|i| target[(begin + i).min(target.len() - 1)])
            .collect();
        let (mut nl, mut nr) = (vec![0.0; stop - start], vec![0.0; stop - start]);
        let result = unsafe {
            lemon_vocal_render(
                s.left[start..].as_ptr(),
                s.right[start..].as_ptr(),
                (stop - start) as i32,
                s.rate as i32,
                f.as_ptr(),
                t.as_ptr(),
                frames as i32,
                f64::from((d.formant + note.formant).clamp(-12.0, 12.0)),
                nl.as_mut_ptr(),
                nr.as_mut_ptr(),
            )
        };
        if result != 0 || nl.iter().chain(&nr).any(|v| !v.is_finite()) {
            return Err("Note formant rendering failed; previous audio retained".into());
        }
        let a = (note.start as f64 * STEP * s.rate as f64).round() as usize;
        let b = ((note.end as f64 * STEP * s.rate as f64).round() as usize).min(s.frames());
        let fade = (s.rate as f32 * 0.005).max(1.0);
        for i in a..b {
            let mix = (((i - a).min(b - 1 - i)) as f32 / fade).clamp(0.0, 1.0);
            l[i] += (nl[i - start] - l[i]) * mix;
            r[i] += (nr[i - start] - r[i]) * mix;
        }
    }
    for note in &d.notes {
        if !note.muted && note.gain.abs() < 0.001 {
            continue;
        }
        let gain = if note.muted {
            0.0
        } else {
            10f32.powf(note.gain / 20.0)
        };
        let a = (note.start as f64 * STEP * s.rate as f64).round() as usize;
        let b = ((note.end as f64 * STEP * s.rate as f64).round() as usize).min(s.frames());
        let fade = (s.rate as f32 * 0.005).max(1.0);
        for i in a..b {
            let mix = (((i - a).min(b - 1 - i)) as f32 / fade).clamp(0.0, 1.0);
            let g = 1.0 + (gain - 1.0) * mix;
            l[i] *= g;
            r[i] *= g;
        }
    }
    if d.timing.iter().any(|(a, b)| (a - b).abs() > 0.0001) {
        let anchors: Vec<_>= d
            .timing
            .iter()
            .map(|(a, b)| {
                (
                    (a * s.rate as f64).round() as usize,
                    (b * s.rate as f64).round() as usize,
                )
            })
            .collect();
        warp(&l, &r, s.rate, &anchors)
    } else {
        Ok((l, r))
    }
}

pub fn timing_valid(d: &Edit) -> bool {
    d.timing.len() <= 1024
        && d.timing.windows(2).all(|w| {
            let frame = |v: f64| (v * d.rate as f64).round() as usize;
            let (a, b, x, y) = (frame(w[0].0), frame(w[1].0), frame(w[0].1), frame(w[1].1));
            b > a && y > x && (0.0625..=16.0).contains(&((y - x) as f64 / (b - a) as f64))
        })
}

struct Cache {
    source: Arc<Sample>,
    edit: Edit,
    audio: Sample,
}

fn cache() -> &'static Mutex<Option<Cache>> {
    static CACHE: OnceLock<Mutex<Option<Cache>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn controls(d: &Edit) -> Vec<(f64, f32, f32)> {
    let pitch = d.corrected();
    let mut out: Vec<_> = pitch
        .into_iter()
        .map(|p| (p, if d.enabled { d.formant } else { 0.0 }, 1.0))
        .collect();
    if d.enabled {
        for n in &d.notes {
            for i in n.start as usize..n.end as usize {
                out[i].1 = (d.formant + n.formant).clamp(-12.0, 12.0);
                out[i].2 = if n.muted {
                    0.0
                } else {
                    10f32.powf(n.gain / 20.0)
                };
            }
        }
    }
    out
}

pub fn render_incremental(
    s: Arc<Sample>,
    d: &Edit,
    cancel: &AtomicBool,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    if !d.valid() || d.frames as usize != s.frames() || d.rate != s.rate {
        return Err("Vocal source changed; reanalyse this recording".into());
    }
    let mut guard = match cache().lock() {
        Ok(g) => g,
        Err(poisoned) => {
            let mut g = poisoned.into_inner();
            *g = None;
            cache().clear_poison();
            g
        }
    };
    if guard
        .as_ref()
        .is_none_or(|c| !Arc::ptr_eq(&c.source, &s) || c.edit.source_hash != d.source_hash)
    {
        let mut dry = d.clone();
        dry.enabled = false;
        *guard = Some(Cache {
            source: s.clone(),
            edit: dry,
            audio: (*s).clone(),
        });
    }
    let c = guard.as_mut().unwrap();
    let before = controls(&c.edit);
    let after = controls(d);
    let mut spans = Vec::<(usize, usize)>::new();
    let mut i = 0;
    while i < after.len() {
        if before[i] == after[i] {
            i += 1;
            continue;
        }
        let a = i;
        i += 1;
        while i < after.len() && before[i] != after[i] {
            i += 1;
        }
        let (a, b) = (a.saturating_sub(12), (i + 12).min(after.len()));
        if let Some(last) = spans.last_mut().filter(|last| last.1 >= a) {
            last.1 = last.1.max(b);
        } else {
            spans.push((a, b));
        }
    }
    let mut next = c.audio.clone();
    let mut regions = Vec::new();
    for (a, b) in spans {
        let mut p = a;
        while p < b {
            let end = (p + 400).min(b);
            regions.push((p, end));
            if end == b {
                break;
            }
            p = end - 12;
        }
    }
    for (a, b) in regions {
        if cancel.load(Ordering::Relaxed) {
            return Err("Superseded vocal edit".into());
        }
        let begin = a.saturating_sub(24);
        let end = (b + 24).min(d.f0.len());
        let start = (begin as f64 * STEP * s.rate as f64).round() as usize;
        let stop = ((end as f64 * STEP * s.rate as f64).round() as usize).min(s.frames());
        if stop <= start {
            continue;
        }
        let region = Sample {
            rate: s.rate,
            left: s.left[start..stop].to_vec(),
            right: s.right[start..stop].to_vec(),
        };
        let frames = (((stop - start) as f64 / s.rate as f64 / STEP).floor() as usize) + 1;
        let f0 = (0..frames)
            .map(|j| d.f0[(begin + j).min(d.f0.len() - 1)])
            .collect();
        let duration = (stop - start) as f64 / s.rate as f64;
        let mut local = Edit {
            channel: d.channel,
            source: d.source.clone(),
            source_hash: d.source_hash.clone(),
            rendered: String::new(),
            render_key: String::new(),
            rate: d.rate,
            frames: (stop - start) as u32,
            f0,
            notes: vec![],
            curve: vec![],
            timing: vec![(0.0, 0.0), (duration, duration)],
            enabled: d.enabled,
            strength: d.strength,
            formant: d.formant,
        };
        local.notes = d
            .notes
            .iter()
            .filter_map(|n| {
                let mut n = n.clone();
                let s_idx = (n.start as usize).max(begin);
                let e_idx = (n.end as usize).min(begin + frames);
                if e_idx <= s_idx {
                    return None;
                }
                n.start = (s_idx - begin) as u32;
                n.end = (e_idx - begin) as u32;
                Some(n)
            })
            .collect();
        local.curve = d
            .curve
            .iter()
            .filter(|(j, _)| (*j as usize) >= begin && (*j as usize) < begin + frames)
            .map(|(j, v)| (*j - begin as u32, *v))
            .collect();
        let (l, r) = render_region(&region, &local)?;
        let lo = (a as f64 * STEP * s.rate as f64).round() as usize;
        let hi = ((b as f64 * STEP * s.rate as f64).round() as usize).min(s.frames());
        let fade = (s.rate as f32 * 0.005).max(1.0);
        for j in lo..hi {
            let blend = (((j - lo).min(hi - 1 - j)) as f32 / fade).clamp(0.0, 1.0);
            next.left[j] += (l[j - start] - next.left[j]) * blend;
            next.right[j] += (r[j - start] - next.right[j]) * blend;
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Superseded vocal edit".into());
    }
    c.audio = next;
    c.edit = d.clone();
    if d.enabled && d.timing.iter().any(|(a, b)| (a - b).abs() > 0.0001) {
        let anchors: Vec<_> = d
            .timing
            .iter()
            .map(|(a, b)| {
                (
                    (a * d.rate as f64).round() as usize,
                    (b * d.rate as f64).round() as usize,
                )
            })
            .collect();
        warp(&c.audio.left, &c.audio.right, d.rate, &anchors)
    } else {
        Ok((c.audio.left.clone(), c.audio.right.clone()))
    }
}
