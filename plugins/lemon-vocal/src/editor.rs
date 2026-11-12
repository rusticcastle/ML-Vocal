// SPDX-License-Identifier: GPL-3.0-or-later
//! Interactive Melodyne-style editor logic, background resynthesis scheduler, and platform window glue.
use crate::{
    Shared, engine,
    params::*,
    view::{self, Field, NavKind, Target, Tool},
};
use nih_plug::prelude::*;
use os_model::vocal::{Edit, STEP};
use os_plugkit::vals::Vals;
use os_ui::Rect;
use std::{
    any::Any,
    path::PathBuf,
    sync::{
        Arc, mpsc,
        atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

pub struct VocalEditor {
    pub params: Arc<VocalParams>,
    pub shared: Arc<Shared>,
    pub scale: AtomicU32,
    #[cfg_attr(not(windows), allow(dead_code))]
    hwnd: Arc<AtomicIsize>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    slot: os_plugkit::gui::Slot,
}

impl VocalEditor {
    pub fn new(params: Arc<VocalParams>, shared: Arc<Shared>) -> Self {
        Self {
            params,
            shared,
            scale: AtomicU32::new(1f32.to_bits()),
            hwnd: Arc::new(AtomicIsize::new(0)),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            slot: Default::default(),
        }
    }

    fn notify(&self) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        os_plugkit::gui::notify(&self.slot);
    }
}

impl Editor for VocalEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let scale = f32::from_bits(self.scale.load(Ordering::Relaxed));
        match parent {
            #[cfg(target_os = "linux")]
            ParentWindowHandle::X11Window(window) => Box::new(gui::open(
                window,
                scale,
                self.params.clone(),
                self.shared.clone(),
                context,
                &self.slot,
            )),
            #[cfg(target_os = "macos")]
            ParentWindowHandle::AppKitNsView(view) => {
                let scale = os_plugkit::gui::backing_scale(view as usize);
                Box::new(gui::open(
                    view as usize,
                    scale,
                    self.params.clone(),
                    self.shared.clone(),
                    context,
                    &self.slot,
                ))
            }
            _ => {
                let _ = (context, scale);
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        (view::W, view::H)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scale.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.notify();
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        self.notify();
    }

    fn param_values_changed(&self) {
        self.notify();
    }
}

#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Btn {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Repaint,
}

#[derive(Clone)]
struct Gesture {
    note: Option<usize>,
    before: Edit,
    selected: Vec<usize>,
    x: f32,
    y: f32,
    last: Option<(u32, f32)>,
    tool: Tool,
    edge: i8,
    axis: i8,
    field: Option<Field>,
    start_val: f32,
    start_offset: f64,
}

#[derive(Clone)]
struct NavGesture {
    grabbed: bool,
    kind: NavKind,
    x: f32,
    y: f32,
    scroll_sec: f64,
    px_per_sec: f64,
    low: f32,
    span: f32,
    selected: Vec<usize>,
}

#[derive(Clone)]
enum DragState {
    Vocal(Gesture),
    Nav(NavGesture),
    Scrub,
}

enum JobDone {
    Analyzed {
        edit: Edit,
        sample: Arc<engine::Sample>,
        start_seconds: f64,
        bpm: f64,
    },
    Rendered {
        render_key: String,
        audio: Arc<engine::Sample>,
    },
}

struct WorkerJob {
    rx: mpsc::Receiver<Result<JobDone, String>>,
    cancel: Arc<AtomicBool>,
    render_key: String,
}

impl Drop for WorkerJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct Logic {
    pub vals: Vals,
    pub params: Arc<VocalParams>,
    pub shared: Arc<Shared>,
    pub hits: Vec<(Rect, Target)>,
    pub hover: Option<Target>,
    pub tool: Tool,
    pub selected: Vec<usize>,
    pub low: f32,
    pub span: f32,
    pub scroll_sec: f64,
    pub px_per_sec: f64,
    pub snap: bool,
    pub marquee: Option<Rect>,
    pub palette_pos: Option<(f32, f32)>,
    pub hint: String,
    pub last_edit: Instant,
    click: Option<(usize, Tool, Instant)>,
    drag: Option<DragState>,
    contour_cache: Option<(String, bool, Arc<Vec<f32>>)>,
    undo_stack: Vec<VocalDoc>,
    redo_stack: Vec<VocalDoc>,
    job: Option<WorkerJob>,
    rendered_key: String,
    failed_keys: std::collections::HashSet<String>,
}

impl Logic {
    pub fn new(
        params: Arc<VocalParams>,
        shared: Arc<Shared>,
        ctx: Option<Arc<dyn GuiContext>>,
    ) -> Self {
        let vals = Vals::new(params.as_ref(), params.live.clone(), ctx);
        let mut l = Self {
            vals,
            params,
            shared,
            hits: Vec::new(),
            hover: None,
            tool: Tool::Main,
            selected: Vec::new(),
            low: 48.0,
            span: 24.0,
            scroll_sec: 0.0,
            px_per_sec: 140.0,
            snap: true,
            marquee: None,
            palette_pos: None,
            hint: "Ready. Click '● Transfer' and play your DAW, or 'Load WAV' to analyze vocals."
                .into(),
            last_edit: Instant::now(),
            click: None,
            drag: None,
            contour_cache: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            job: None,
            rendered_key: String::new(),
            failed_keys: std::collections::HashSet::new(),
        };
        if let Some(d) = &l.params.document().edit {
            l.rendered_key = engine::edit_key(d);
            l.fit_notes(d, false);
        }
        l
    }

    pub fn val(&self, i: usize) -> f32 {
        self.vals.plain(&self.params.defs[i].id)
    }

    pub fn set_param(&mut self, i: usize, v: f32) {
        let d = &self.params.defs[i];
        let clamped = v.clamp(d.min, d.max);
        let id = d.id.clone();
        self.vals.edit_plain(&id, clamped);
    }

    pub fn hovered(&self, t: &Target) -> bool {
        self.hover.as_ref() == Some(t)
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn playhead_sec(&self) -> f64 {
        f64::from_bits(self.shared.playhead_sec.load(Ordering::Relaxed))
    }

    pub fn status_line(&self) -> String {
        if let Ok(msg) = self.shared.status.lock() {
            if !msg.is_empty() {
                return msg.clone();
            }
        }
        format!(
            "{} selected  ·  {}  ·  {}  ·  Wheel: pitch   Shift: time   Ctrl: zoom   Ctrl+Shift: pitch zoom",
            self.selected.len(),
            if self.snap { "Snap on" } else { "Free" },
            view::label(self.tool)
        )
    }

    pub fn set_status(&self, msg: impl Into<String>) {
        let s = msg.into();
        if let Ok(mut g) = self.shared.status.lock() {
            *g = s;
        }
    }

    pub fn cached_contour(&mut self, d: &Edit) -> Arc<Vec<f32>> {
        let k = engine::edit_key(d);
        if let Some((ref old_k, on, ref p)) = self.contour_cache {
            if *old_k == k && on == d.enabled {
                return p.clone();
            }
        }
        let p = Arc::new(d.pitches());
        self.contour_cache = Some((k, d.enabled, p.clone()));
        p
    }

    pub fn inspector_value(
        &self,
        field: Field,
        edit_opt: Option<&Edit>,
        start_seconds: f64,
    ) -> String {
        match field {
            Field::Strength => format!("{:.0}%", self.val(STRENGTH) * 100.0),
            Field::GlobalFormant => format!("{:+.1} st", self.val(FORMANT)),
            Field::Offset => format!("{:.2} s", start_seconds),
            Field::Gain => format!("{:+.1} dB", self.val(GAIN)),
            _ => {
                let Some(d) = edit_opt else {
                    return "—".into();
                };
                if self.selected.is_empty() {
                    return "—".into();
                }
                let values: Vec<f32> = self
                    .selected
                    .iter()
                    .filter_map(|&ni| d.notes.get(ni))
                    .map(|n| match field {
                        Field::Pitch => n.base + n.shift,
                        Field::Modulation => n.variation * 100.0,
                        Field::Drift => n.drift * 100.0,
                        Field::Transition => n.transition * 100.0,
                        Field::Formant => n.formant,
                        Field::Amplitude => n.gain,
                        _ => 0.0,
                    })
                    .collect();
                if values.is_empty() {
                    return "—".into();
                }
                if values.windows(2).any(|w| (w[0] - w[1]).abs() > 0.01) {
                    return "Mixed".into();
                }
                let v = values[0];
                match field {
                    Field::Pitch => {
                        let p = v.round() as i32;
                        let names = [
                            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                        ];
                        format!(
                            "{}{} {:+.0} ct",
                            names[p.rem_euclid(12) as usize],
                            p / 12 - 1,
                            (v - p as f32) * 100.0
                        )
                    }
                    Field::Modulation | Field::Drift | Field::Transition => format!("{v:.0}%"),
                    Field::Formant => format!("{v:+.1} st"),
                    Field::Amplitude => format!("{v:+.1} dB"),
                    _ => "—".into(),
                }
            }
        }
    }

    fn snapshot(&mut self) {
        let cur = (*self.params.document()).clone();
        if self.undo_stack.last() != Some(&cur) {
            self.undo_stack.push(cur);
            if self.undo_stack.len() > 64 {
                self.undo_stack.remove(0);
            }
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            let cur = (*self.params.document()).clone();
            self.redo_stack.push(cur);
            self.apply_doc(prev);
            self.failed_keys.clear();
            self.set_status("Undo vocal edit");
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            let cur = (*self.params.document()).clone();
            self.undo_stack.push(cur);
            self.apply_doc(next);
            self.failed_keys.clear();
            self.set_status("Redo vocal edit");
        }
    }

    fn apply_doc(&mut self, doc: VocalDoc) {
        if let Some(ref d) = doc.edit {
            self.set_param(STRENGTH, d.strength);
            self.set_param(FORMANT, d.formant);
            self.set_param(BYPASS, if d.enabled { 0.0 } else { 1.0 });
        }
        self.params.set_doc(doc);
        self.last_edit = Instant::now();
    }

    pub fn fit_notes(&mut self, d: &Edit, selected_only: bool) {
        let chosen: Vec<_> = d
            .notes
            .iter()
            .enumerate()
            .filter(|(i, _)| !selected_only || self.selected.contains(i))
            .map(|(_, n)| n)
            .collect();
        if !chosen.is_empty() {
            let mut lo = 128.0f32;
            let mut hi = 0.0f32;
            let mut min_t = f64::MAX;
            let mut max_t = 0.0f64;
            for n in chosen {
                lo = lo.min(n.base + n.shift);
                hi = hi.max(n.base + n.shift);
                min_t = min_t.min(d.destination(n.start as f64 * STEP));
                max_t = max_t.max(d.destination(n.end as f64 * STEP));
            }
            self.span = (hi - lo + 8.0).clamp(12.0, 48.0);
            self.low = (lo - 4.0).clamp(0.0, 128.0 - self.span);
            if max_t > min_t + 0.1 {
                let pr = view::pitch_rect();
                let dur = (max_t - min_t) * 1.15;
                self.px_per_sec = (f64::from(pr.w) / dur).clamp(20.0, 2000.0);
                self.scroll_sec = (min_t - (max_t - min_t) * 0.075).max(0.0);
            }
        } else {
            let dur = (d.frames as f64 / d.rate.max(1) as f64).max(1.0);
            let pr = view::pitch_rect();
            self.px_per_sec = (f64::from(pr.w) / dur).clamp(20.0, 2000.0);
            self.scroll_sec = 0.0;
        }
    }

    pub fn load_file(&mut self, path: PathBuf) {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.job = Some(WorkerJob {
            rx,
            cancel,
            render_key: String::new(),
        });
        self.shared.busy.store(true, Ordering::Relaxed);
        self.set_status(format!("Analysing {}...", path.display()));
        let bpm = f64::from_bits(self.shared.host_bpm.load(Ordering::Relaxed)).clamp(20.0, 400.0);
        std::thread::spawn(move || {
            let res = (|| {
                let sample = engine::load_sample(&path)?;
                let edit = engine::analyze_to_edit(&sample, path.to_string_lossy().into_owned())?;
                Ok(JobDone::Analyzed {
                    edit,
                    sample,
                    start_seconds: 0.0,
                    bpm,
                })
            })();
            if !stop.load(Ordering::Relaxed) {
                let _ = tx.send(res);
            }
        });
    }

    pub fn start_analysis_from_sample(
        &mut self,
        sample: Arc<engine::Sample>,
        start_seconds: f64,
        bpm: f64,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.job = Some(WorkerJob {
            rx,
            cancel,
            render_key: String::new(),
        });
        self.shared.busy.store(true, Ordering::Relaxed);
        self.set_status("Analysing captured vocal pitch & notes...");
        std::thread::spawn(move || {
            let res = (|| {
                let dir = cache_dir();
                let _ = std::fs::create_dir_all(&dir);
                let hash = engine::sample_hash(&sample);
                let path = dir.join(format!("Transfer-{}.wav", &hash[..16]));
                if !path.is_file() {
                    engine::write_wav(&path, &sample)?;
                }
                let edit = engine::analyze_to_edit(&sample, path.to_string_lossy().into_owned())?;
                Ok(JobDone::Analyzed {
                    edit,
                    sample,
                    start_seconds,
                    bpm,
                })
            })();
            if !stop.load(Ordering::Relaxed) {
                let _ = tx.send(res);
            }
        });
    }

    pub fn poll(&mut self) {
        self.vals.expire();

        // Check if a DAW transfer recording just finished
        let taken = self
            .shared
            .completed_transfer
            .try_lock()
            .ok()
            .and_then(|mut c| c.take());
        if let Some((sample, start_sec, bpm)) = taken {
            self.start_analysis_from_sample(Arc::new(sample), start_sec, bpm);
        }

        // Check background job completion
        let done = self.job.as_ref().and_then(|j| match j.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Vocal worker stopped unexpectedly.".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = done {
            let finished = self.job.take().unwrap();
            self.shared.busy.store(false, Ordering::Relaxed);
            match result {
                Err(e) => {
                    if !finished.render_key.is_empty() {
                        self.failed_keys.insert(finished.render_key.clone());
                    }
                    self.set_status(format!("Vocal tuning: {e}"));
                }
                Ok(JobDone::Analyzed {
                    edit,
                    sample,
                    start_seconds,
                    bpm,
                }) => {
                    self.snapshot();
                    let note_count = edit.notes.len();
                    self.rendered_key = engine::edit_key(&edit);
                    *self.shared.source_audio.lock().unwrap() = Some(sample.clone());
                    *self.shared.rendered_audio.lock().unwrap() = Some(sample);
                    self.selected.clear();
                    self.failed_keys.clear();
                    self.fit_notes(&edit, false);
                    let doc = VocalDoc {
                        edit: Some(edit),
                        start_seconds,
                        bpm,
                    };
                    self.apply_doc(doc);
                    self.set_status(format!(
                        "Analysed {note_count} vocal notes. Drag blobs to tune pitch, formant, vibrato & timing."
                    ));
                }
                Ok(JobDone::Rendered { render_key, audio }) => {
                    self.rendered_key = render_key.clone();
                    *self.shared.rendered_audio.lock().unwrap() = Some(audio);
                    let mut doc = (*self.params.document()).clone();
                    if let Some(ref mut d) = doc.edit {
                        d.render_key = render_key;
                    }
                    self.params.set_doc(doc);
                    self.set_status("Vocal resynthesis ready.");
                }
            }
        }

        // Sync host-automated strength/formant/bypass into Edit if changed externally
        let doc = self.params.document();
        if let Some(ref d) = doc.edit {
            let p_str = self.val(STRENGTH).clamp(0.0, 1.0);
            let p_fmt = self.val(FORMANT).clamp(-12.0, 12.0);
            let p_en = self.val(BYPASS) < 0.5;
            if (d.strength - p_str).abs() > 0.005
                || (d.formant - p_fmt).abs() > 0.05
                || d.enabled != p_en
            {
                let mut next_doc = (*doc).clone();
                if let Some(ref mut ed) = next_doc.edit {
                    ed.strength = p_str;
                    ed.formant = p_fmt;
                    ed.enabled = p_en;
                }
                self.params.set_doc(next_doc);
                self.last_edit = Instant::now();
            }
        }

        // Schedule incremental resynthesis when edits settle
        if self.job.is_some()
            || self.drag.is_some()
            || self.last_edit.elapsed() < Duration::from_millis(25)
        {
            return;
        }

        let doc = self.params.document();
        let Some(ref d) = doc.edit else { return };
        let k = engine::edit_key(d);
        if k == self.rendered_key || self.failed_keys.contains(&k) {
            return;
        }
        let source_opt = self.shared.source_audio.lock().unwrap().clone();
        let Some(source) = source_opt else { return };

        if !d.changed() {
            *self.shared.rendered_audio.lock().unwrap() = Some(source);
            self.rendered_key = k;
            return;
        }

        let edit_clone = d.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.job = Some(WorkerJob {
            rx,
            cancel,
            render_key: k.clone(),
        });
        self.shared.busy.store(true, Ordering::Relaxed);
        self.set_status("Applying vocal tuning...");
        std::thread::spawn(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let (l, r) = engine::render_incremental(source, &edit_clone, &stop)?;
                if stop.load(Ordering::Relaxed) {
                    return Err("Superseded vocal edit".into());
                }
                Ok(JobDone::Rendered {
                    render_key: k,
                    audio: Arc::new(engine::Sample {
                        rate: edit_clone.rate,
                        left: l,
                        right: r,
                    }),
                })
            }))
            .unwrap_or_else(|_| Err("Vocal synthesis worker panicked.".into()));
            if !stop.load(Ordering::Relaxed) {
                let _ = tx.send(res);
            }
        });
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Target> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, t)| t.clone())
    }

    pub fn down(
        &mut self,
        x: f32,
        y: f32,
        btn: Btn,
        mods: Mods,
        _dbl: bool,
    ) -> Vec<Act> {
        if btn == Btn::Right {
            if self.palette_pos.is_some() {
                self.palette_pos = None;
            } else if view::pitch_rect().contains(x, y) {
                self.palette_pos = Some((x, y));
            }
            return vec![Act::Repaint];
        }
        if btn != Btn::Left {
            return vec![];
        }

        let hit = self.hit_at(x, y);
        if self.palette_pos.is_some() {
            self.palette_pos = None;
            if let Some(ref t) = hit {
                return self.trigger_target(t.clone(), x, y, mods);
            }
            return vec![Act::Repaint];
        }

        if let Some(t) = hit {
            return self.trigger_target(t, x, y, mods);
        }
        vec![]
    }

    fn trigger_target(&mut self, t: Target, x: f32, y: f32, mods: Mods) -> Vec<Act> {
        match t {
            Target::Transfer => {
                let cur = self.shared.transfer_recording.load(Ordering::Relaxed);
                if cur {
                    self.shared.stop_transfer.store(true, Ordering::Release);
                    self.set_status("Finishing transfer and analysing vocal...");
                } else {
                    self.shared.preview_playing.store(false, Ordering::Relaxed);
                    self.shared.start_transfer.store(true, Ordering::Release);
                    self.set_status(
                        "Transfer armed: play your DAW track to capture vocal audio, then click Stop Xfer.",
                    );
                }
            }
            Target::Load => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Load Vocal Audio")
                    .add_filter("Audio Files", &["wav", "flac", "mp3", "ogg"])
                    .pick_file()
                {
                    self.load_file(path);
                }
            }
            Target::Export => {
                let rendered = self.shared.rendered_audio.lock().unwrap().clone();
                if let Some(sample) = rendered {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Export Tuned Vocal WAV")
                        .set_file_name("TunedVocal.wav")
                        .add_filter("WAV Audio", &["wav"])
                        .save_file()
                    {
                        match engine::write_wav(&path, &sample) {
                            Ok(()) => self.set_status(format!("Exported to {}", path.display())),
                            Err(e) => self.set_status(format!("Export failed: {e}")),
                        }
                    }
                } else {
                    self.set_status("No vocal audio loaded to export.");
                }
            }
            Target::Play => {
                let now = !self.shared.preview_playing.load(Ordering::Relaxed);
                self.shared.preview_playing.store(now, Ordering::Relaxed);
                self.set_status(if now {
                    "Playing vocal preview..."
                } else {
                    "Preview stopped."
                });
            }
            Target::Sync => {
                let next = if self.val(SYNC) >= 0.5 { 0.0 } else { 1.0 };
                self.set_param(SYNC, next);
            }
            Target::LiveTune => {
                let next = if self.val(LIVE_TUNE) >= 0.5 { 0.0 } else { 1.0 };
                self.set_param(LIVE_TUNE, next);
                self.set_status(if next >= 0.5 {
                    "Live Auto-Tune ON (real-time SnapTune correction active on incoming audio)."
                } else {
                    "Live Auto-Tune OFF (Melodyne transfer/blob playback mode)."
                });
            }
            Target::Voice => {
                let next = if self.val(VOICE) >= 0.5 { 0.0 } else { 1.0 };
                self.set_param(VOICE, next);
            }
            Target::Tool(tool) => {
                self.tool = tool;
                self.set_status(view::label(tool));
            }
            Target::Key => {
                let next = ((self.val(KEY).round() as i32 + 1).rem_euclid(12)) as f32;
                self.set_param(KEY, next);
            }
            Target::Scale => {
                let next = ((self.val(SCALE).round() as i32 + 1).rem_euclid(3)) as f32;
                self.set_param(SCALE, next);
            }
            Target::Snap => {
                self.snap = !self.snap;
            }
            Target::Undo => self.undo(),
            Target::Redo => self.redo(),
            Target::Cancel => {
                if let Some(j) = &self.job {
                    if !j.render_key.is_empty() {
                        self.failed_keys.insert(j.render_key.clone());
                    }
                }
                self.job = None;
                self.shared.busy.store(false, Ordering::Relaxed);
                self.set_status("Operation cancelled; previous audio retained.");
            }
            Target::Reanalyse => {
                let source = self.shared.source_audio.lock().unwrap().clone();
                if let Some(sample) = source {
                    let doc = self.params.document();
                    self.start_analysis_from_sample(sample, doc.start_seconds, doc.bpm);
                }
            }
            Target::XZoom(dir) => {
                let pr = view::pitch_rect();
                self.zoom_x(1.4f64.powi(dir as i32), pr.cx());
            }
            Target::Fit => {
                let doc = self.params.document();
                if let Some(ref d) = doc.edit {
                    let d_clone = d.clone();
                    self.fit_notes(&d_clone, false);
                }
            }
            Target::Ruler => {
                let t_sec = view::x_to_time(self, x);
                self.shared
                    .playhead_sec
                    .store(t_sec.to_bits(), Ordering::Relaxed);
                self.shared.seek_requested.store(true, Ordering::Release);
                self.drag = Some(DragState::Scrub);
            }
            Target::Keys => {
                self.begin_nav(NavKind::Pan, x, y, mods);
            }
            Target::Nav(kind) => {
                self.begin_nav(kind, x, y, mods);
            }
            Target::Surface => {
                if self.tool == Tool::Draw {
                    self.begin_note_or_draw(None, x, y, mods);
                } else {
                    let kind = match self.tool {
                        Tool::Hand => NavKind::Pan,
                        Tool::Zoom => NavKind::Zoom,
                        _ => NavKind::Box,
                    };
                    self.begin_nav(kind, x, y, mods);
                }
            }
            Target::Note(ni) => {
                self.begin_note_or_draw(Some(ni), x, y, mods);
            }
            Target::Boundary(ni) => {
                self.begin_boundary(ni, x, y);
            }
            Target::Field(field) => {
                self.begin_field(field, x, y, mods);
            }
            Target::Correct
            | Target::Join
            | Target::Mute
            | Target::Bypass
            | Target::ResetNote
            | Target::ResetAll => {
                self.exec_edit_cmd(t);
            }
        }
        vec![Act::Repaint]
    }

    fn exec_edit_cmd(&mut self, cmd: Target) {
        let mut doc = (*self.params.document()).clone();
        let Some(ref mut d) = doc.edit else { return };
        self.snapshot();
        let root = (self.val(KEY).round() as u8).min(11);
        let scale = (self.val(SCALE).round() as u8).min(2);
        match cmd {
            Target::Bypass => {
                d.enabled = !d.enabled;
                self.set_param(BYPASS, if d.enabled { 0.0 } else { 1.0 });
            }
            Target::ResetAll => {
                d.curve.clear();
                let dur = d.frames as f64 / d.rate.max(1) as f64;
                d.timing = vec![(0.0, 0.0), (dur, dur)];
                d.formant = 0.0;
                d.strength = 1.0;
                for n in &mut d.notes {
                    n.reset();
                }
                self.selected.clear();
            }
            Target::Join => {
                if self.selected.len() >= 2 {
                    let a = *self.selected.iter().min().unwrap();
                    let b = *self.selected.iter().max().unwrap();
                    if b < d.notes.len() {
                        let end = d.notes[b].end;
                        d.notes[a].end = end;
                        d.notes.drain(a + 1..=b);
                        self.selected = vec![a];
                    }
                }
            }
            _ => {
                let all_selected = self.selected.clone();
                for (ni, note) in d.notes.iter_mut().enumerate() {
                    if !all_selected.is_empty() && !all_selected.contains(&ni) {
                        continue;
                    }
                    match cmd {
                        Target::Correct => {
                            let target = view::nearest(note.base, root, scale);
                            note.shift = target - note.base;
                        }
                        Target::Mute => note.muted = !note.muted,
                        Target::ResetNote => {
                            note.reset();
                            d.curve.retain(|(i, _)| *i < note.start || *i >= note.end);
                        }
                        _ => {}
                    }
                }
            }
        }
        self.apply_doc(doc);
    }

    fn frame_at(&self, d: &Edit, x: f32) -> u32 {
        let dest = view::x_to_time(self, x);
        (d.original(dest) / STEP)
            .round()
            .clamp(0.0, (d.f0.len().saturating_sub(1)) as f64) as u32
    }

    fn begin_note_or_draw(&mut self, note: Option<usize>, x: f32, y: f32, mods: Mods) {
        if matches!(self.tool, Tool::Hand | Tool::Zoom) {
            self.begin_nav(
                if self.tool == Tool::Hand {
                    NavKind::Pan
                } else {
                    NavKind::Zoom
                },
                x,
                y,
                mods,
            );
            return;
        }
        let doc = (*self.params.document()).clone();
        let Some(d) = doc.edit.clone() else { return };

        if let Some(n) = note {
            if n >= d.notes.len() {
                return;
            }
            if mods.ctrl {
                if let Some(pos) = self.selected.iter().position(|&i| i == n) {
                    self.selected.remove(pos);
                } else {
                    self.selected.push(n);
                }
                return;
            }
            if mods.shift {
                if !self.selected.contains(&n) {
                    self.selected.push(n);
                }
            } else if !self.selected.contains(&n) {
                self.selected = vec![n];
            }

            let now = Instant::now();
            let double = self.click.is_some_and(|(ni, t, at)| {
                ni == n && t == self.tool && now.duration_since(at).as_millis() < 330
            });
            self.click = if double {
                None
            } else {
                Some((n, self.tool, now))
            };

            if double
                && !matches!(
                    self.tool,
                    Tool::Split | Tool::Draw | Tool::Time | Tool::Attack
                )
            {
                self.snapshot();
                let root = (self.val(KEY).round() as u8).min(11);
                let scale = (self.val(SCALE).round() as u8).min(2);
                let mut next_doc = doc;
                if let Some(ref mut ed) = next_doc.edit {
                    for &ni in &self.selected {
                        if let Some(note_mut) = ed.notes.get_mut(ni) {
                            match self.tool {
                                Tool::Modulation => {
                                    note_mut.variation =
                                        if note_mut.variation.abs() < 0.001 { 1.0 } else { 0.0 }
                                }
                                Tool::Drift => {
                                    note_mut.drift =
                                        if note_mut.drift.abs() < 0.001 { 1.0 } else { 0.0 }
                                }
                                Tool::Transition => note_mut.transition = 0.0,
                                Tool::Formant => note_mut.formant = 0.0,
                                Tool::Amplitude => note_mut.muted = !note_mut.muted,
                                _ => {
                                    note_mut.shift =
                                        view::nearest(note_mut.base + note_mut.shift, root, scale)
                                            - note_mut.base
                                }
                            }
                        }
                    }
                }
                self.apply_doc(next_doc);
                return;
            }
        }

        if self.tool == Tool::Split {
            if let Some(n) = note {
                if mods.alt {
                    self.selected = vec![n, (n + 1).min(d.notes.len() - 1)];
                    self.exec_edit_cmd(Target::Join);
                    return;
                }
                let frame = self.frame_at(&d, x);
                let original = d.notes[n].clone();
                if frame > original.start + 2 && frame + 2 < original.end {
                    self.snapshot();
                    let mut next_doc = doc;
                    if let Some(ref mut ed) = next_doc.edit {
                        ed.notes[n].end = frame;
                        let mut right = original;
                        right.start = frame;
                        ed.notes.insert(n + 1, right);
                    }
                    self.selected.clear();
                    self.apply_doc(next_doc);
                }
            }
            return;
        }

        self.snapshot();
        let selected = self.selected.clone();
        self.drag = Some(DragState::Vocal(Gesture {
            note,
            before: d.clone(),
            selected,
            x,
            y,
            last: None,
            tool: self.tool,
            edge: 0,
            axis: 0,
            field: None,
            start_val: 0.0,
            start_offset: doc.start_seconds,
        }));
        if let Some(n) = note {
            self.start_audition(&d, n);
        }
        if self.tool == Tool::Draw {
            self.drag_vocal(x, y, mods);
        }
    }

    fn begin_boundary(&mut self, n: usize, x: f32, y: f32) {
        let doc = (*self.params.document()).clone();
        let Some(d) = doc.edit else { return };
        if n >= d.notes.len() {
            return;
        }
        self.snapshot();
        self.selected = vec![n];
        self.drag = Some(DragState::Vocal(Gesture {
            note: Some(n),
            before: d,
            selected: vec![n],
            x,
            y,
            last: None,
            tool: if self.tool == Tool::Split {
                Tool::Split
            } else {
                Tool::Time
            },
            edge: 1,
            axis: 0,
            field: None,
            start_val: 0.0,
            start_offset: doc.start_seconds,
        }));
    }

    fn begin_field(&mut self, field: Field, x: f32, y: f32, mods: Mods) {
        let mut doc = (*self.params.document()).clone();
        if mods.alt {
            self.snapshot();
            match field {
                Field::Strength => {
                    self.set_param(STRENGTH, 1.0);
                    if let Some(ref mut d) = doc.edit {
                        d.strength = 1.0;
                    }
                }
                Field::GlobalFormant => {
                    self.set_param(FORMANT, 0.0);
                    if let Some(ref mut d) = doc.edit {
                        d.formant = 0.0;
                    }
                }
                Field::Offset => {
                    doc.start_seconds = 0.0;
                }
                Field::Gain => {
                    self.set_param(GAIN, 0.0);
                }
                _ => {
                    if let Some(ref mut d) = doc.edit {
                        for &ni in &self.selected {
                            if let Some(n) = d.notes.get_mut(ni) {
                                match field {
                                    Field::Pitch => n.shift = 0.0,
                                    Field::Modulation => n.variation = 1.0,
                                    Field::Drift => n.drift = 1.0,
                                    Field::Transition => n.transition = 0.0,
                                    Field::Formant => n.formant = 0.0,
                                    Field::Amplitude => {
                                        n.gain = 0.0;
                                        n.muted = false;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            self.apply_doc(doc);
            return;
        }

        let start_val = match field {
            Field::Strength => self.val(STRENGTH),
            Field::GlobalFormant => self.val(FORMANT),
            Field::Gain => self.val(GAIN),
            _ => 0.0,
        };
        let before = doc.edit.clone().unwrap_or_else(|| Edit {
            channel: 0,
            source: String::new(),
            source_hash: String::new(),
            rendered: String::new(),
            render_key: String::new(),
            rate: 48000,
            frames: 48000,
            f0: vec![0.0; 201],
            notes: vec![],
            curve: vec![],
            timing: vec![(0.0, 0.0), (1.0, 1.0)],
            enabled: true,
            strength: 1.0,
            formant: 0.0,
        });
        let tool = match field {
            Field::Pitch => Tool::Select,
            Field::Modulation => Tool::Modulation,
            Field::Drift => Tool::Drift,
            Field::Transition => Tool::Transition,
            Field::Formant => Tool::Formant,
            Field::Amplitude => Tool::Amplitude,
            _ => Tool::Select,
        };
        self.snapshot();
        self.drag = Some(DragState::Vocal(Gesture {
            note: self.selected.first().copied(),
            before,
            selected: self.selected.clone(),
            x,
            y,
            last: None,
            tool,
            edge: 0,
            axis: 0,
            field: Some(field),
            start_val,
            start_offset: doc.start_seconds,
        }));
    }

    fn begin_nav(&mut self, kind: NavKind, x: f32, y: f32, mods: Mods) {
        let pr = view::pitch_rect();
        let grabbed = if kind == NavKind::Vertical {
            let top = pr.y + (128.0 - self.low - self.span) / 128.0 * pr.h;
            y >= top && y <= top + (self.span / 128.0 * pr.h).max(10.0)
        } else if kind == NavKind::Horizontal {
            true
        } else {
            false
        };
        if kind == NavKind::Box && !mods.shift {
            self.selected.clear();
        }
        self.drag = Some(DragState::Nav(NavGesture {
            grabbed,
            kind,
            x,
            y,
            scroll_sec: self.scroll_sec,
            px_per_sec: self.px_per_sec,
            low: self.low,
            span: self.span,
            selected: if mods.shift {
                self.selected.clone()
            } else {
                Vec::new()
            },
        }));
        if matches!(kind, NavKind::Horizontal | NavKind::Vertical) {
            self.drag_nav(x, y);
        }
    }

    fn start_audition(&self, d: &Edit, ni: usize) {
        let Some(note) = d.notes.get(ni) else { return };
        let a = (note.start as f64 * STEP * d.rate as f64).round() as usize;
        let b = (note.end as f64 * STEP * d.rate as f64).round() as usize;
        self.shared.audition_start.store(a as u32, Ordering::Relaxed);
        self.shared.audition_end.store(b as u32, Ordering::Relaxed);
        self.update_audition(d, ni);
        self.shared.audition_active.store(true, Ordering::Release);
    }

    fn update_audition(&self, d: &Edit, ni: usize) {
        let Some(note) = d.notes.get(ni) else { return };
        let cents = if d.enabled {
            note.shift * d.strength * 100.0
        } else {
            0.0
        };
        let gain = if note.muted && d.enabled {
            0.0
        } else if d.enabled {
            10f32.powf(note.gain / 20.0)
        } else {
            1.0
        };
        self.shared
            .audition_cents
            .store(cents.to_bits(), Ordering::Relaxed);
        self.shared
            .audition_gain
            .store(gain.to_bits(), Ordering::Relaxed);
    }

    fn stop_audition(&self) {
        self.shared.audition_active.store(false, Ordering::Release);
    }

    pub fn moved(&mut self, x: f32, y: f32, mods: Mods) -> Vec<Act> {
        if self.drag.is_some() {
            match self.drag {
                Some(DragState::Vocal(_)) => self.drag_vocal(x, y, mods),
                Some(DragState::Nav(_)) => self.drag_nav(x, y),
                Some(DragState::Scrub) => {
                    let t_sec = view::x_to_time(self, x);
                    self.shared
                        .playhead_sec
                        .store(t_sec.to_bits(), Ordering::Relaxed);
                    self.shared.seek_requested.store(true, Ordering::Release);
                }
                None => {}
            }
            return vec![Act::Repaint];
        }
        let next = self.hit_at(x, y);
        if next != self.hover {
            self.hover = next;
            return vec![Act::Repaint];
        }
        vec![]
    }

    pub fn up(&mut self, _btn: Btn) -> Vec<Act> {
        self.stop_audition();
        self.marquee = None;
        if self.drag.take().is_some() {
            self.last_edit = Instant::now();
            return vec![Act::Repaint];
        }
        vec![]
    }

    pub fn leave(&mut self) -> Vec<Act> {
        if self.hover.take().is_some() {
            return vec![Act::Repaint];
        }
        vec![]
    }

    fn anchor(points: &mut Vec<(f64, f64)>, source: f64, dest: f64) {
        if let Some(p) = points.iter_mut().find(|p| (p.0 - source).abs() < 0.00001) {
            p.1 = dest;
        } else {
            points.push((source, dest));
        }
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
    }

    fn limit_timing(before: &Edit, after: &mut Edit) {
        if engine::timing_valid(after) || !engine::timing_valid(before) {
            return;
        }
        let candidate = after.timing.clone();
        let mut best = before.timing.clone();
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..24 {
            let t = (lo + hi) / 2.0;
            after.timing = candidate
                .iter()
                .map(|&(x, y)| (x, before.destination(x) + (y - before.destination(x)) * t))
                .collect();
            if engine::timing_valid(after) {
                best = after.timing.clone();
                lo = t;
            } else {
                hi = t;
            }
        }
        after.timing = best;
    }

    fn drag_vocal(&mut self, x: f32, y: f32, mods: Mods) {
        let Some(DragState::Vocal(mut g)) = self.drag.clone() else {
            return;
        };
        let pr = view::pitch_rect();
        if (x - g.x).abs() > 3.0 || (y - g.y).abs() > 3.0 {
            self.click = None;
        }
        let fine = if mods.alt { 0.2 } else { 1.0 };
        let dy = (g.y - y) * fine;

        match g.field {
            Some(Field::Strength) => {
                let v = (g.start_val + dy / 150.0).clamp(0.0, 1.0);
                self.set_param(STRENGTH, v);
                let mut doc = (*self.params.document()).clone();
                if let Some(ref mut d) = doc.edit {
                    d.strength = v;
                }
                self.params.set_doc(doc);
                self.last_edit = Instant::now();
                self.drag = Some(DragState::Vocal(g));
                return;
            }
            Some(Field::GlobalFormant) => {
                let v = (g.start_val + dy / 12.0).clamp(-12.0, 12.0);
                self.set_param(FORMANT, v);
                let mut doc = (*self.params.document()).clone();
                if let Some(ref mut d) = doc.edit {
                    d.formant = v;
                }
                self.params.set_doc(doc);
                self.last_edit = Instant::now();
                self.drag = Some(DragState::Vocal(g));
                return;
            }
            Some(Field::Offset) => {
                let mut doc = (*self.params.document()).clone();
                doc.start_seconds = (g.start_offset + f64::from(dy) * 0.02).max(0.0);
                self.params.set_doc(doc);
                self.drag = Some(DragState::Vocal(g));
                return;
            }
            Some(Field::Gain) => {
                let v = (g.start_val + dy / 8.0).clamp(-24.0, 24.0);
                self.set_param(GAIN, v);
                self.drag = Some(DragState::Vocal(g));
                return;
            }
            _ => {}
        }

        let mut doc = (*self.params.document()).clone();
        let Some(ref cur_edit) = doc.edit else {
            return;
        };

        let mut tool = g.tool;
        if tool == Tool::Main {
            if g.axis == 0 && ((x - g.x).abs() > 3.0 || (y - g.y).abs() > 3.0) {
                g.axis = if (x - g.x).abs() > (y - g.y).abs() {
                    1
                } else {
                    2
                };
            }
            if g.axis == 0 {
                self.drag = Some(DragState::Vocal(g));
                return;
            }
            tool = if g.axis == 1 {
                Tool::Time
            } else {
                Tool::Select
            };
        }

        let mut d = if tool == Tool::Draw {
            cur_edit.clone()
        } else {
            g.before.clone()
        };

        if tool == Tool::Draw {
            let frame = self.frame_at(&d, x);
            let pitch = self.low + (pr.bottom() - y) / pr.h * self.span;
            let start = g.last.unwrap_or((frame, pitch));
            let (a, b) = (start.0.min(frame), start.0.max(frame));
            let mut clean = d.clone();
            clean.curve.clear();
            let baseline = clean.pitches();
            for f in a..=b {
                if d.f0[f as usize] <= 0.0 {
                    continue;
                }
                let t = if start.0 == frame {
                    1.0
                } else {
                    (f as f32 - start.0 as f32) / (frame as f32 - start.0 as f32)
                };
                let target = start.1 + (pitch - start.1) * t;
                let delta =
                    ((target - baseline[f as usize]) / d.strength.max(0.01)).clamp(-24.0, 24.0);
                match d.curve.binary_search_by_key(&f, |p| p.0) {
                    Ok(i) => d.curve[i].1 = delta,
                    Err(i) => d.curve.insert(i, (f, delta)),
                }
            }
            g.last = Some((frame, pitch));
        } else if tool == Tool::Split {
            if let Some(n) = g.note {
                let frame = self.frame_at(&d, x);
                let end = if n + 1 < d.notes.len() {
                    d.notes[n + 1].end.saturating_sub(3)
                } else {
                    d.f0.len() as u32 - 1
                };
                let at = frame.clamp(d.notes[n].start + 3, end.max(d.notes[n].start + 3));
                d.notes[n].end = at;
                if n + 1 < d.notes.len() {
                    d.notes[n + 1].start = at;
                }
            }
        } else if matches!(tool, Tool::Time | Tool::Attack) {
            if let Some(n) = g.note {
                let note = &g.before.notes[n];
                let duration = d.frames as f64 / d.rate.max(1) as f64;
                let (a, b) = (
                    note.start as f64 * STEP,
                    (note.end as f64 * STEP).min(duration),
                );
                let mut delta = f64::from(x - g.x) / self.px_per_sec;
                if !mods.alt && self.snap {
                    let bpm = doc.bpm.clamp(20.0, 400.0);
                    let sixteenth = 15.0 / bpm;
                    delta = (delta / sixteenth).round() * sixteenth;
                }
                let epsilon = 1.0 / d.rate.max(1) as f64;
                if tool == Tool::Attack {
                    let mid = (a + b) / 2.0;
                    if b - a > 0.02
                        && g.before.destination(b) - g.before.destination(a) > 2.0 * epsilon
                    {
                        let left = g.before.destination(a);
                        let right = g.before.destination(b);
                        let target = (g.before.destination(mid) + delta)
                            .clamp(left + epsilon, right - epsilon);
                        Self::anchor(&mut d.timing, a, left);
                        Self::anchor(&mut d.timing, b, right);
                        d.timing.retain(|p| p.0 <= a || p.0 >= b);
                        Self::anchor(&mut d.timing, mid, target);
                    }
                } else if g.edge != 0 {
                    if b > 0.005 && b < duration - 0.005 {
                        let left = g
                            .before
                            .timing
                            .iter()
                            .filter(|p| p.0 < b - 0.00001)
                            .last()
                            .copied()
                            .unwrap_or((0.0, 0.0));
                        let right = g
                            .before
                            .timing
                            .iter()
                            .find(|p| p.0 > b + 0.00001)
                            .copied()
                            .unwrap_or((duration, duration));
                        let (lo, hi) = (left.1 + epsilon, right.1 - epsilon);
                        if lo <= hi {
                            Self::anchor(
                                &mut d.timing,
                                b,
                                (g.before.destination(b) + delta).clamp(lo, hi),
                            );
                        }
                    }
                } else if a > 0.0 && b < duration {
                    let mut timing: Vec<_> = d
                        .timing
                        .iter()
                        .copied()
                        .filter(|p| p.0 < a - 0.00001 || p.0 > b + 0.00001)
                        .collect();
                    let left = timing
                        .iter()
                        .filter(|p| p.0 < a)
                        .last()
                        .copied()
                        .unwrap_or((0.0, 0.0));
                    let right = timing
                        .iter()
                        .find(|p| p.0 > b)
                        .copied()
                        .unwrap_or((duration, duration));
                    let min = left.1 + epsilon - g.before.destination(a);
                    let max = right.1 - epsilon - g.before.destination(b);
                    if min <= max {
                        let delta = delta.clamp(min, max);
                        timing.extend([
                            (a, g.before.destination(a) + delta),
                            (b, g.before.destination(b) + delta),
                        ]);
                        timing.sort_by(|a, b| a.0.total_cmp(&b.0));
                        d.timing = timing;
                    }
                }
            }
        } else {
            for &n in &g.selected {
                if let Some(note) = d.notes.get_mut(n) {
                    match tool {
                        Tool::Select => {
                            let mut delta = dy / pr.h * self.span;
                            if !mods.alt && self.snap {
                                delta = delta.round();
                            }
                            note.shift = (note.shift + delta).clamp(-24.0, 24.0);
                        }
                        Tool::Modulation => {
                            note.variation = (note.variation + dy / 100.0).clamp(-1.0, 2.0)
                        }
                        Tool::Drift => note.drift = (note.drift + dy / 100.0).clamp(-1.0, 2.0),
                        Tool::Transition => {
                            note.transition = (note.transition + dy / 100.0).clamp(0.0, 1.0)
                        }
                        Tool::Formant => {
                            note.formant = (note.formant + dy / 12.0).clamp(-12.0, 12.0)
                        }
                        Tool::Amplitude => note.gain = (note.gain + dy / 4.0).clamp(-60.0, 24.0),
                        _ => {}
                    }
                }
            }
        }

        if matches!(tool, Tool::Time | Tool::Attack) {
            Self::limit_timing(&g.before, &mut d);
        }
        if d.valid() && (!matches!(tool, Tool::Time | Tool::Attack) || engine::timing_valid(&d)) {
            if let Some(ni) = g.note {
                self.update_audition(&d, ni);
            }
            doc.edit = Some(d);
            self.params.set_doc(doc);
            self.last_edit = Instant::now();
        }
        self.drag = Some(DragState::Vocal(g));
    }

    pub fn zoom_x(&mut self, factor: f64, focus_x: f32) {
        let pr = view::pitch_rect();
        let at_sec = self.scroll_sec + f64::from(focus_x - pr.x) / self.px_per_sec;
        self.px_per_sec = (self.px_per_sec * factor).clamp(15.0, 2500.0);
        self.scroll_sec = (at_sec - f64::from(focus_x - pr.x) / self.px_per_sec).max(0.0);
    }

    pub fn zoom_y(&mut self, factor: f32, focus_y: f32) {
        let pr = view::pitch_rect();
        let t = ((pr.bottom() - focus_y) / pr.h).clamp(0.0, 1.0);
        let pitch = self.low + t * self.span;
        self.span = (self.span / factor).clamp(6.0, 72.0);
        self.low = (pitch - t * self.span).clamp(0.0, 128.0 - self.span);
    }

    fn drag_nav(&mut self, x: f32, y: f32) {
        let Some(DragState::Nav(n)) = self.drag.clone() else {
            return;
        };
        let pr = view::pitch_rect();
        match n.kind {
            NavKind::Pan => {
                self.scroll_sec = (n.scroll_sec + f64::from(n.x - x) / self.px_per_sec).max(0.0);
                self.low = (n.low + (y - n.y) / pr.h * n.span).clamp(0.0, 128.0 - n.span);
            }
            NavKind::Zoom => {
                self.scroll_sec = n.scroll_sec;
                self.px_per_sec = n.px_per_sec;
                self.low = n.low;
                self.span = n.span;
                self.zoom_x(2f64.powf(f64::from(x - n.x) / 160.0), n.x);
                self.zoom_y(2f32.powf((n.y - y) / 160.0), n.y);
            }
            NavKind::Horizontal => {
                let doc = self.params.document();
                let total_sec = doc
                    .edit
                    .as_ref()
                    .map(|d| (d.frames as f64 / d.rate.max(1) as f64).max(4.0))
                    .unwrap_or(16.0);
                let w = (pr.w - 145.0).max(30.0);
                let frac = ((x - pr.x) / w).clamp(0.0, 1.0);
                self.scroll_sec = f64::from(frac) * total_sec;
            }
            NavKind::Vertical => {
                self.low = if n.grabbed {
                    n.low - (y - n.y) / pr.h * 128.0
                } else {
                    (1.0 - (y - pr.y) / pr.h) * 128.0 - n.span / 2.0
                }
                .clamp(0.0, 128.0 - n.span);
            }
            NavKind::Box => {
                let area = Rect::new(n.x.min(x), n.y.min(y), (x - n.x).abs(), (y - n.y).abs());
                let mut selected = n.selected.clone();
                let doc = self.params.document();
                if let Some(ref d) = doc.edit {
                    for (ni, _) in d.notes.iter().enumerate() {
                        if let Some(nr) = view::note_rect(self, d, ni) {
                            if !nr.intersect(&pr).intersect(&area).is_empty()
                                && !selected.contains(&ni)
                            {
                                selected.push(ni);
                            }
                        }
                    }
                }
                self.selected = selected;
                self.marquee = Some(area);
            }
        }
    }

    pub fn wheel(&mut self, x: f32, y: f32, notches: f32, mods: Mods) -> Vec<Act> {
        let pr = view::pitch_rect();
        let kr = view::keys_rect();
        if !pr.union(&kr).contains(x, y) || self.drag.is_some() {
            return vec![];
        }
        if mods.ctrl {
            if mods.shift {
                self.zoom_y(1.18f32.powf(notches), y);
            } else {
                self.zoom_x(1.18f64.powf(f64::from(notches)), x.max(pr.x));
            }
        } else if mods.shift {
            self.scroll_sec = (self.scroll_sec - f64::from(notches) * 65.0 / self.px_per_sec).max(0.0);
        } else {
            self.low = (self.low + notches * self.span / 12.0).clamp(0.0, 128.0 - self.span);
        }
        vec![Act::Repaint]
    }

    pub fn key_press(&mut self, ch: Option<char>, keysym: u32, mods: Mods) -> Vec<Act> {
        // Escape = 0xff1b
        if keysym == 0xff1b {
            self.selected.clear();
            self.palette_pos = None;
            return vec![Act::Repaint];
        }
        if let Some(c) = ch {
            let lower = c.to_ascii_lowercase();
            if mods.ctrl {
                match lower {
                    'z' => {
                        if mods.shift {
                            self.redo();
                        } else {
                            self.undo();
                        }
                        return vec![Act::Repaint];
                    }
                    'y' => {
                        self.redo();
                        return vec![Act::Repaint];
                    }
                    'a' => {
                        if let Some(ref d) = self.params.document().edit {
                            self.selected = (0..d.notes.len()).collect();
                        }
                        return vec![Act::Repaint];
                    }
                    _ => {}
                }
            } else if !mods.alt {
                match lower {
                    '1' => self.tool = Tool::Main,
                    '2' => self.tool = Tool::Select,
                    '3' => self.tool = Tool::Modulation,
                    '4' => self.tool = Tool::Drift,
                    '5' => self.tool = Tool::Formant,
                    '6' => self.tool = Tool::Amplitude,
                    '7' => self.tool = Tool::Time,
                    '8' => self.tool = Tool::Attack,
                    '9' => self.tool = Tool::Split,
                    't' => self.tool = Tool::Transition,
                    'd' => self.tool = Tool::Draw,
                    'h' => self.tool = Tool::Hand,
                    'z' => self.tool = Tool::Zoom,
                    'f' => {
                        if let Some(ref d) = self.params.document().edit {
                            let d_clone = d.clone();
                            self.fit_notes(&d_clone, false);
                        }
                    }
                    'm' => self.exec_edit_cmd(Target::Mute),
                    ' ' => {
                        let now = !self.shared.preview_playing.load(Ordering::Relaxed);
                        self.shared.preview_playing.store(now, Ordering::Relaxed);
                    }
                    _ => return vec![],
                }
                return vec![Act::Repaint];
            }
        }
        vec![]
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod gui {
    use super::{Act, Btn, Logic, Mods};
    use crate::{Shared, params::VocalParams, view};
    use nih_plug::prelude::GuiContext;
    use os_hostlink::{PageIcon, Peer};
    use os_plugkit::gui::{self as px, Button, Input, Window};
    use os_theme::Palette;
    use os_ui::Canvas;
    use std::path::PathBuf;
    use std::sync::{Arc, atomic::Ordering};

    struct Ed {
        win: Window,
        canvas: Canvas,
        buf: Vec<u32>,
        scale: f32,
        size: (u32, u32),
        logic: Logic,
        palette: Palette,
        dirty: bool,
        link: Arc<Peer>,
    }

    impl Drop for Ed {
        fn drop(&mut self) {
            self.link.unregister();
        }
    }

    pub struct Handle(Option<px::EditorThread>, Arc<Shared>);

    impl Drop for Handle {
        fn drop(&mut self) {
            self.0.take();
            self.1.editor_open.store(false, Ordering::Relaxed);
        }
    }

    pub fn open(
        parent: px::Parent,
        scale: f32,
        params: Arc<VocalParams>,
        shared: Arc<Shared>,
        ctx: Arc<dyn GuiContext>,
        slot: &px::Slot,
    ) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let size = (
            (view::W as f32 * scale).round() as u32,
            (view::H as f32 * scale).round() as u32,
        );
        let sh = shared.clone();
        let wake = slot.clone();
        let link = Peer::register(
            parent as u64,
            vec![PageIcon::Plugin],
            (view::W, view::H),
            move || px::notify(&wake),
        );
        let peer = link.clone();
        let thread = px::spawn(parent, size, slot, move |mut win| {
            win.key_releases();
            #[cfg(target_os = "macos")]
            win.accept_files();
            let Ok(canvas) = Canvas::new(size.0, size.1, scale) else {
                return;
            };
            let logic = Logic::new(params, shared, Some(ctx));
            let ed = Ed {
                win,
                canvas,
                buf: Vec::new(),
                scale,
                size,
                logic,
                palette: os_plugkit::palette(),
                dirty: true,
                link: peer,
            };
            ed.run();
        });
        if thread.is_none() {
            link.unregister();
        }
        sh.editor_open.store(thread.is_some(), Ordering::Relaxed);
        Handle(thread, sh)
    }

    fn btn(b: Button) -> Btn {
        match b {
            Button::Left => Btn::Left,
            Button::Right => Btn::Right,
            Button::Middle => Btn::Middle,
        }
    }

    impl Ed {
        fn mods(&self) -> Mods {
            let (ctrl, shift, alt) = self.win.mods();
            Mods { ctrl, shift, alt }
        }

        fn run(mut self) {
            while let Some(input) = self.win.next() {
                match input {
                    Input::Expose => self.dirty = true,
                    Input::Tick => {
                        self.logic.poll();
                        self.dirty = true;
                    }
                    Input::Down { x, y, btn: b, dbl } => {
                        let acts = self.logic.down(
                            x / self.scale,
                            y / self.scale,
                            btn(b),
                            self.mods(),
                            dbl && b == Button::Left,
                        );
                        self.act(acts);
                    }
                    Input::Move { x, y } => {
                        let acts =
                            self.logic
                                .moved(x / self.scale, y / self.scale, self.mods());
                        self.act(acts);
                    }
                    Input::Delta { .. } => {}
                    Input::Up { btn: b, .. } => {
                        self.win.unlock();
                        let acts = self.logic.up(btn(b));
                        self.act(acts);
                    }
                    Input::Leave => {
                        if !self.win.locked() && !self.logic.dragging() {
                            let acts = self.logic.leave();
                            self.act(acts);
                        }
                    }
                    Input::Wheel { x, y, notches } => {
                        let acts = self.logic.wheel(
                            x / self.scale,
                            y / self.scale,
                            notches,
                            self.mods(),
                        );
                        self.act(acts);
                    }
                    Input::Drop { path } => {
                        self.logic.load_file(PathBuf::from(path));
                        self.dirty = true;
                    }
                    Input::Key {
                        sym,
                        text,
                        pressed,
                    } => {
                        if pressed {
                            let ch = text.or_else(|| px::keysym_char(sym));
                            let acts = self.logic.key_press(ch, sym, self.mods());
                            self.act(acts);
                        }
                    }
                }
                if self.dirty && self.win.idle() {
                    self.paint();
                }
            }
        }

        fn act(&mut self, acts: Vec<Act>) {
            for a in acts {
                match a {
                    Act::Repaint => self.dirty = true,
                }
            }
        }

        fn paint(&mut self) {
            self.dirty = false;
            view::paint(&mut self.logic, &mut self.canvas, &self.palette);
            self.buf.resize((self.size.0 * self.size.1) as usize, 0);
            self.canvas.copy_to_xrgb(&mut self.buf);
            self.win.blit(&self.buf, self.size.0, self.size.1);
        }
    }
}
