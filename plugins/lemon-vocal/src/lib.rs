// SPDX-License-Identifier: GPL-3.0-or-later
//! ML Vocal: Melodyne-style monophonic vocal pitch, formant, expression, and timing editor + live auto-tune as a VST3 / CLAP plugin.
#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod editor;
pub mod engine;
pub mod params;
pub mod view;

use engine::{Sample, SnapTune};
use nih_plug::prelude::*;
use params::*;
use std::{
    num::NonZeroU32,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};

pub struct Shared {
    pub editor_open: AtomicBool,
    pub busy: AtomicBool,
    pub start_transfer: AtomicBool,
    pub stop_transfer: AtomicBool,
    pub transfer_recording: AtomicBool,
    pub preview_playing: AtomicBool,
    pub seek_requested: AtomicBool,
    pub playhead_sec: AtomicU64,
    pub host_bpm: AtomicU64,
    pub audition_active: AtomicBool,
    pub audition_start: AtomicU32,
    pub audition_end: AtomicU32,
    pub audition_cents: AtomicU32,
    pub audition_gain: AtomicU32,
    pub source_audio: Mutex<Option<Arc<Sample>>>,
    pub rendered_audio: Mutex<Option<Arc<Sample>>>,
    pub completed_transfer: Mutex<Option<(Sample, f64, f64)>>,
    pub status: Mutex<String>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            editor_open: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            start_transfer: AtomicBool::new(false),
            stop_transfer: AtomicBool::new(false),
            transfer_recording: AtomicBool::new(false),
            preview_playing: AtomicBool::new(false),
            seek_requested: AtomicBool::new(false),
            playhead_sec: AtomicU64::new(0.0f64.to_bits()),
            host_bpm: AtomicU64::new(120.0f64.to_bits()),
            audition_active: AtomicBool::new(false),
            audition_start: AtomicU32::new(0),
            audition_end: AtomicU32::new(0),
            audition_cents: AtomicU32::new(0.0f32.to_bits()),
            audition_gain: AtomicU32::new(1.0f32.to_bits()),
            source_audio: Mutex::new(None),
            rendered_audio: Mutex::new(None),
            completed_transfer: Mutex::new(None),
            status: Mutex::new(String::new()),
        }
    }
}

pub struct LemonVocal {
    params: Arc<VocalParams>,
    shared: Arc<Shared>,
    sample_rate: f32,
    doc: Arc<VocalDoc>,
    rendered: Option<Arc<Sample>>,
    source: Option<Arc<Sample>>,
    snaptune: Option<SnapTune>,
    snaptune_buf: Vec<f32>,
    transfer_left: Vec<f32>,
    transfer_right: Vec<f32>,
    transfer_start_sec: f64,
    was_host_playing: bool,
    preview_pos: f64,
    audition_pos: f64,
}

impl Default for LemonVocal {
    fn default() -> Self {
        let params = Arc::new(VocalParams::default());
        let doc = params.document();
        Self {
            params,
            shared: Arc::new(Shared::default()),
            sample_rate: 48000.0,
            doc,
            rendered: None,
            source: None,
            snaptune: None,
            snaptune_buf: Vec::with_capacity(4096),
            transfer_left: Vec::new(),
            transfer_right: Vec::new(),
            transfer_start_sec: 0.0,
            was_host_playing: false,
            preview_pos: 0.0,
            audition_pos: 0.0,
        }
    }
}

impl LemonVocal {
    fn finish_transfer(&mut self) {
        self.shared
            .transfer_recording
            .store(false, Ordering::Release);
        if self.transfer_left.len() >= (self.sample_rate as usize / 10).max(64) {
            let sample = Sample {
                rate: self.sample_rate.round() as u32,
                left: std::mem::take(&mut self.transfer_left),
                right: std::mem::take(&mut self.transfer_right),
            };
            let bpm = f64::from_bits(self.shared.host_bpm.load(Ordering::Relaxed));
            if let Ok(mut slot) = self.shared.completed_transfer.try_lock() {
                *slot = Some((sample, self.transfer_start_sec, bpm));
            }
        } else {
            self.transfer_left.clear();
            self.transfer_right.clear();
            if let Ok(mut s) = self.shared.status.try_lock() {
                *s = "Transfer too short; play audio into the plugin while Transfer is armed."
                    .into();
            }
        }
    }
}

impl Plugin for LemonVocal {
    const NAME: &'static str = "ML Vocal";
    const VENDOR: &'static str = "ML Studio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::VocalEditor::new(
            self.params.clone(),
            self.shared.clone(),
        )))
    }

    fn initialize(
        &mut self,
        _: &AudioIOLayout,
        c: &BufferConfig,
        _: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = c.sample_rate.max(16000.0);
        let rate_u32 = self.sample_rate.round() as u32;
        self.snaptune = Some(SnapTune::new(rate_u32));
        self.transfer_left.reserve(rate_u32 as usize * 30);
        self.transfer_right.reserve(rate_u32 as usize * 30);

        let d = self.params.document();
        if let Some(ref edit) = d.edit {
            if !edit.source.is_empty() && Path::new(&edit.source).is_file() {
                if let Ok(sample) = engine::load_sample(Path::new(&edit.source)) {
                    *self.shared.source_audio.lock().unwrap() = Some(sample.clone());
                    *self.shared.rendered_audio.lock().unwrap() = Some(sample.clone());
                    self.source = Some(sample.clone());
                    self.rendered = Some(sample.clone());
                    if edit.changed() {
                        let edit_clone = edit.clone();
                        let sh = self.shared.clone();
                        std::thread::spawn(move || {
                            let stop = AtomicBool::new(false);
                            if let Ok((l, r)) =
                                engine::render_incremental(sample, &edit_clone, &stop)
                            {
                                let rendered = Arc::new(Sample {
                                    rate: edit_clone.rate,
                                    left: l,
                                    right: r,
                                });
                                *sh.rendered_audio.lock().unwrap() = Some(rendered);
                            }
                        });
                    }
                }
            }
        }
        true
    }

    fn reset(&mut self) {
        self.was_host_playing = false;
    }

    fn process(
        &mut self,
        b: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        ctx: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        if let Ok(d) = self.params.doc.try_read() {
            self.doc = d.clone();
        }
        if let Ok(r) = self.shared.rendered_audio.try_lock() {
            self.rendered = r.clone();
        }
        if let Ok(s) = self.shared.source_audio.try_lock() {
            self.source = s.clone();
        }

        let transport = ctx.transport();
        if let Some(bpm) = transport.tempo {
            if bpm > 10.0 {
                self.shared.host_bpm.store(bpm.to_bits(), Ordering::Relaxed);
            }
        }
        let host_playing = transport.playing;
        let host_sec = transport
            .pos_seconds()
            .or_else(|| {
                transport
                    .pos_samples()
                    .map(|s| s as f64 / f64::from(self.sample_rate))
            })
            .unwrap_or(0.0)
            .max(0.0);

        // Handle Transfer start/stop
        if self.shared.start_transfer.swap(false, Ordering::AcqRel) {
            self.transfer_left.clear();
            self.transfer_right.clear();
            self.transfer_start_sec = host_sec;
            self.shared
                .transfer_recording
                .store(true, Ordering::Release);
        }
        if self.shared.stop_transfer.swap(false, Ordering::AcqRel) {
            self.finish_transfer();
        } else if self.shared.transfer_recording.load(Ordering::Relaxed)
            && self.was_host_playing
            && !host_playing
            && !self.transfer_left.is_empty()
        {
            self.finish_transfer();
        }
        self.was_host_playing = host_playing;

        let slices = b.as_slice();
        if slices.len() < 2 {
            return ProcessStatus::Normal;
        }
        let frames = slices[0].len();
        if frames == 0 {
            return ProcessStatus::Normal;
        }

        // Record incoming audio when Transfer is active
        if self.shared.transfer_recording.load(Ordering::Relaxed) {
            let max_frames = (self.sample_rate as usize * 600).min(33_554_432);
            let can_take = frames.min(max_frames.saturating_sub(self.transfer_left.len()));
            if self.transfer_left.is_empty() && host_playing {
                self.transfer_start_sec = host_sec;
            }
            if can_take > 0 {
                self.transfer_left.extend_from_slice(&slices[0][..can_take]);
                self.transfer_right
                    .extend_from_slice(&slices[1][..can_take]);
                let rec_dur =
                    self.transfer_start_sec + self.transfer_left.len() as f64 / f64::from(self.sample_rate);
                self.shared
                    .playhead_sec
                    .store(rec_dur.to_bits(), Ordering::Relaxed);
            }
            if self.transfer_left.len() >= max_frames {
                self.finish_transfer();
            }
            return ProcessStatus::Normal;
        }

        if self.shared.seek_requested.swap(false, Ordering::AcqRel) {
            let t = f64::from_bits(self.shared.playhead_sec.load(Ordering::Relaxed));
            self.preview_pos = t * f64::from(self.sample_rate);
        }

        let bypass = self.params.v(BYPASS) >= 0.5;
        let out_gain = 10f32.powf(self.params.v(GAIN) / 20.0);

        // 1. Note Scrub / Audition while dragging a note blob in the editor
        if self.shared.audition_active.load(Ordering::Acquire) {
            if let Some(ref src) = self.source {
                let start = (self.shared.audition_start.load(Ordering::Relaxed) as usize)
                    .min(src.frames().saturating_sub(1));
                let end = (self.shared.audition_end.load(Ordering::Relaxed) as usize)
                    .min(src.frames())
                    .max(start + 1);
                let cents = f32::from_bits(self.shared.audition_cents.load(Ordering::Relaxed));
                let note_gain =
                    f32::from_bits(self.shared.audition_gain.load(Ordering::Relaxed)) * out_gain;
                let step = f64::from(2f32.powf(cents / 1200.0))
                    * (src.rate as f64 / f64::from(self.sample_rate));
                let span = (end - start) as f64;
                let fade = (src.rate as f64 * 0.008).clamp(1.0, span * 0.4);
                for i in 0..frames {
                    if self.audition_pos < 0.0 || self.audition_pos >= span {
                        self.audition_pos = 0.0;
                    }
                    let pos = start as f64 + self.audition_pos;
                    let idx0 = (pos.floor() as usize).clamp(start, end - 1);
                    let idx1 = (idx0 + 1).min(end - 1);
                    let frac = (pos - pos.floor()) as f32;
                    let env = (self.audition_pos / fade)
                        .min((span - self.audition_pos) / fade)
                        .clamp(0.0, 1.0) as f32;
                    let l = src.left[idx0] + (src.left[idx1] - src.left[idx0]) * frac;
                    let r = src.right[idx0] + (src.right[idx1] - src.right[idx0]) * frac;
                    slices[0][i] = l * note_gain * env;
                    slices[1][i] = r * note_gain * env;
                    self.audition_pos += step;
                }
                return ProcessStatus::KeepAlive;
            }
        } else {
            self.audition_pos = 0.0;
        }

        // 2. Internal Preview Play button
        if self.shared.preview_playing.load(Ordering::Relaxed) {
            let active_sample = if bypass {
                self.source.as_ref().or(self.rendered.as_ref())
            } else {
                self.rendered.as_ref().or(self.source.as_ref())
            };
            if let Some(sample) = active_sample {
                let ratio = sample.rate as f64 / f64::from(self.sample_rate);
                for i in 0..frames {
                    let s_pos = self.preview_pos * ratio;
                    let idx0 = s_pos.floor() as usize;
                    if idx0 + 1 < sample.frames() {
                        let frac = (s_pos - s_pos.floor()) as f32;
                        let l = sample.left[idx0] + (sample.left[idx0 + 1] - sample.left[idx0]) * frac;
                        let r = sample.right[idx0]
                            + (sample.right[idx0 + 1] - sample.right[idx0]) * frac;
                        slices[0][i] = l * out_gain;
                        slices[1][i] = r * out_gain;
                        self.preview_pos += 1.0;
                    } else {
                        slices[0][i] = 0.0;
                        slices[1][i] = 0.0;
                        self.preview_pos = 0.0;
                        self.shared.preview_playing.store(false, Ordering::Relaxed);
                        break;
                    }
                }
                let sec = self.preview_pos / f64::from(self.sample_rate);
                self.shared
                    .playhead_sec
                    .store(sec.to_bits(), Ordering::Relaxed);
                return ProcessStatus::KeepAlive;
            }
        }

        // 3. Live Auto-Tune Mode (SnapTune real-time pitch correction)
        let live_tune = self.params.v(LIVE_TUNE) >= 0.5;
        if live_tune && !bypass {
            if let Some(ref mut st) = self.snaptune {
                let strength = self.params.v(STRENGTH).clamp(0.0, 1.0);
                let key = (self.params.v(KEY).round() as u8).min(11);
                let scale = (self.params.v(SCALE).round() as u8).min(2);
                let voice = (self.params.v(VOICE).round() as u8).min(1);
                st.configure(strength, key, scale, voice, true);
                self.snaptune_buf.resize(frames * 2, 0.0);
                for i in 0..frames {
                    self.snaptune_buf[i * 2] = slices[0][i];
                    self.snaptune_buf[i * 2 + 1] = slices[1][i];
                }
                st.process(&mut self.snaptune_buf);
                for i in 0..frames {
                    slices[0][i] = self.snaptune_buf[i * 2] * out_gain;
                    slices[1][i] = self.snaptune_buf[i * 2 + 1] * out_gain;
                }
                return ProcessStatus::Normal;
            }
        }

        // 4. Host-Synced Playback of Resynthesized Vocal
        let sync_on = self.params.v(SYNC) >= 0.5;
        let active_sample = if bypass {
            self.source.as_ref().or(self.rendered.as_ref())
        } else {
            self.rendered.as_ref().or(self.source.as_ref())
        };
        if sync_on && host_playing {
            if let Some(sample) = active_sample {
                let rel_sec = host_sec - self.doc.start_seconds;
                let clip_dur = sample.frames() as f64 / sample.rate.max(1) as f64;
                if rel_sec >= 0.0 && rel_sec < clip_dur {
                    self.shared
                        .playhead_sec
                        .store(rel_sec.to_bits(), Ordering::Relaxed);
                }
                for i in 0..frames {
                    let frame_sec = rel_sec + i as f64 / f64::from(self.sample_rate);
                    if frame_sec >= 0.0 {
                        let s_pos = frame_sec * sample.rate as f64;
                        let idx0 = s_pos.floor() as usize;
                        if idx0 < sample.frames() {
                            let idx1 = (idx0 + 1).min(sample.frames() - 1);
                            let frac = (s_pos - s_pos.floor()) as f32;
                            let l = sample.left[idx0]
                                + (sample.left[idx1] - sample.left[idx0]) * frac;
                            let r = sample.right[idx0]
                                + (sample.right[idx1] - sample.right[idx0]) * frac;
                            slices[0][i] = l * out_gain;
                            slices[1][i] = r * out_gain;
                        }
                    }
                }
                return ProcessStatus::Normal;
            }
        }

        if (out_gain - 1.0).abs() > 0.0001 && !bypass {
            for ch in slices.iter_mut() {
                for s in ch.iter_mut() {
                    *s *= out_gain;
                }
            }
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for LemonVocal {
    const CLAP_ID: &'static str = "org.lemonstudio.lemon-vocal";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Melodyne-style vocal pitch, formant, expression, and timing editor + live auto-tune");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::PitchShifter,
        ClapFeature::Restoration,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for LemonVocal {
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioVocal";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::PitchShift,
        Vst3SubCategory::Restoration,
    ];
}

#[cfg(not(feature = "linked"))]
nih_export_clap!(LemonVocal);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(LemonVocal);
os_plugkit::linked_clap_entry!(LemonVocal);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn embedded_world_engine_analyzes_and_shifts_pitch() {
        assert_eq!(engine::version(), 1);
        let rate = 24000;
        let frames = rate as usize * 2;
        let l: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let hz = if t < 0.8 {
                    220.0
                } else if t > 1.0 && t < 1.8 {
                    261.626
                } else {
                    0.0
                };
                if hz > 0.0 {
                    ((t * hz * std::f32::consts::TAU).sin()
                        + 0.3 * (t * hz * 2.0 * std::f32::consts::TAU).sin())
                        * 0.2
                } else {
                    0.0
                }
            })
            .collect();
        let sample = Arc::new(Sample {
            rate,
            left: l.clone(),
            right: l,
        });
        let mut edit = engine::analyze_to_edit(&sample, "synthetic.wav".into()).unwrap();
        assert!(edit.notes.len() >= 2);
        edit.notes[0].shift = 3.0;
        let cancel = AtomicBool::new(false);
        let (out_l, out_r) = engine::render_incremental(sample.clone(), &edit, &cancel).unwrap();
        assert_eq!(out_l.len(), frames);
        assert_eq!(out_r.len(), frames);
        assert!(out_l.iter().chain(&out_r).all(|v| v.is_finite()));
        assert!(out_l.iter().zip(&sample.left).any(|(a, b)| (a - b).abs() > 0.01));
    }

    #[test]
    fn editor_paints_and_handles_note_edits_and_undo() {
        let params = Arc::new(VocalParams::default());
        let shared = Arc::new(Shared::default());
        let mut logic = editor::Logic::new(params.clone(), shared, None);
        let rate = 24000;
        let mut f0 = vec![0.0; 401];
        for (i, v) in f0.iter_mut().enumerate() {
            *v = if (20..120).contains(&i) {
                220.0
            } else if (160..280).contains(&i) {
                261.626
            } else {
                0.0
            };
        }
        let notes = engine::segment(&f0);
        let edit = os_model::vocal::Edit {
            channel: 0,
            source: "test.wav".into(),
            source_hash: "h".into(),
            rendered: String::new(),
            render_key: String::new(),
            rate,
            frames: rate * 2,
            f0,
            notes,
            curve: vec![],
            timing: vec![(0.0, 0.0), (2.0, 2.0)],
            enabled: true,
            strength: 1.0,
            formant: 0.0,
        };
        params.set_doc(VocalDoc {
            edit: Some(edit),
            start_seconds: 0.0,
            bpm: 120.0,
        });
        let mut canvas = os_ui::Canvas::new(view::W, view::H, 1.0).unwrap();
        view::paint(&mut logic, &mut canvas, &os_plugkit::palette());
        let note_hit = logic
            .hits
            .iter()
            .find(|(_, t)| *t == view::Target::Note(0))
            .expect("note 0 hit")
            .0;
        logic.down(
            note_hit.cx(),
            note_hit.cy(),
            editor::Btn::Left,
            editor::Mods::default(),
            false,
        );
        logic.moved(
            note_hit.cx(),
            note_hit.cy() - 40.0,
            editor::Mods::default(),
        );
        logic.up(editor::Btn::Left);
        assert!(params.document().edit.as_ref().unwrap().notes[0].shift > 0.0);
        logic.undo();
        assert_eq!(params.document().edit.as_ref().unwrap().notes[0].shift, 0.0);
        logic.redo();
        assert!(params.document().edit.as_ref().unwrap().notes[0].shift > 0.0);
    }
}
