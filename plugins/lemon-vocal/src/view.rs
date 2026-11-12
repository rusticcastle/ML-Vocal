// SPDX-License-Identifier: GPL-3.0-or-later
//! Melodyne-style vocal editor view and layout for the standalone VST3 / CLAP plugin.
use crate::editor::Logic;
use crate::params::*;
use os_model::vocal::{Edit, STEP};
use os_theme::{Color, Palette};
use os_ui::{Canvas, Font, Rect, tabler::TablerIcon};

pub const W: u32 = 1040;
pub const H: u32 = 620;

pub const KEYS_W: f32 = 52.0;
pub const HEADER_H: f32 = 38.0;
pub const TOOLS_H: f32 = 36.0;
pub const INSPECTOR_H: f32 = 38.0;
pub const RULER_H: f32 = 22.0;
pub const GRID_TOP: f32 = HEADER_H + TOOLS_H + INSPECTOR_H + RULER_H;
pub const GRID_BOTTOM: f32 = 568.0;
pub const GRID_RIGHT: f32 = 1022.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Main,
    Select,
    Modulation,
    Drift,
    Transition,
    Formant,
    Amplitude,
    Time,
    Attack,
    Split,
    Draw,
    Hand,
    Zoom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Pitch,
    Modulation,
    Drift,
    Transition,
    Formant,
    Amplitude,
    Strength,
    GlobalFormant,
    Offset,
    Gain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    Pan,
    Zoom,
    Box,
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Transfer,
    Load,
    Export,
    Play,
    Sync,
    LiveTune,
    Voice,
    Tool(Tool),
    Key,
    Scale,
    Correct,
    Join,
    Snap,
    Mute,
    Bypass,
    ResetNote,
    ResetAll,
    Reanalyse,
    Undo,
    Redo,
    Field(Field),
    Ruler,
    Keys,
    Surface,
    Note(usize),
    Boundary(usize),
    Nav(NavKind),
    XZoom(i8),
    Fit,
    Cancel,
}

pub const TOOLS: [Tool; 13] = [
    Tool::Main,
    Tool::Select,
    Tool::Modulation,
    Tool::Drift,
    Tool::Transition,
    Tool::Formant,
    Tool::Amplitude,
    Tool::Time,
    Tool::Attack,
    Tool::Split,
    Tool::Draw,
    Tool::Hand,
    Tool::Zoom,
];

pub fn label(t: Tool) -> &'static str {
    match t {
        Tool::Main => {
            "Main: drag vertically for pitch, horizontally for time; drag note edge for duration (1)"
        }
        Tool::Select => "Pitch: drag to transpose, Alt for cents; double-click to tune (2)",
        Tool::Modulation => "Pitch modulation: drag vibrato amount; double-click original/flat (3)",
        Tool::Drift => "Pitch drift: drag slow intonation movement; double-click original/flat (4)",
        Tool::Transition => {
            "Pitch transition: drag to smooth the connection to the next adjacent note (T)"
        }
        Tool::Formant => "Formant: drag vocal colour per note; double-click reset (5)",
        Tool::Amplitude => "Amplitude: drag note level; double-click mute (6)",
        Tool::Time => "Timing: drag note position or its right edge for duration (7)",
        Tool::Attack => "Time handle: drag the note's internal midpoint, keeping its edges (8)",
        Tool::Split => {
            "Separation: click to split, drag a boundary to adjust; Alt-click joins next (9)"
        }
        Tool::Draw => "Draw a pitch curve (D)",
        Tool::Hand => "Hand: drag to scroll time and pitch (H)",
        Tool::Zoom => "Zoom: drag right/left for time, up/down for pitch (Z)",
    }
}

pub fn icon(t: Tool) -> TablerIcon {
    if t == Tool::Zoom {
        return os_ui::tabler::ZOOM_IN;
    }
    TablerIcon {
        filled: false,
        d: match t {
            Tool::Main => "M 5 3 L 5 20 L 10 15 L 14 22 L 17 20 L 13 13 L 20 12 Z",
            Tool::Select => {
                "M 3 12 L 15 12 M 9 7 L 9 17 M 18 3 L 18 21 M 15 6 L 18 3 L 21 6 M 15 18 L 18 21 L 21 18"
            }
            Tool::Modulation => "M 2 13 Q 5 2 8 13 Q 11 24 14 13 Q 17 2 20 13 L 22 13",
            Tool::Drift => "M 3 17 C 8 17 8 7 13 7 C 17 7 17 12 21 12",
            Tool::Transition => "M 2 17 L 7 17 Q 12 17 12 12 Q 12 7 17 7 L 22 7",
            Tool::Formant => "M 3 19 L 6 10 L 9 19 M 10 19 L 14 4 L 18 19 M 18 19 L 21 12 L 23 19",
            Tool::Amplitude => "M 4 15 L 4 9 L 8 9 L 14 4 L 14 20 L 8 15 Z M 18 8 Q 23 12 18 16",
            Tool::Time => "M 3 12 L 21 12 M 7 8 L 3 12 L 7 16 M 17 8 L 21 12 L 17 16 M 12 5 L 12 19",
            Tool::Attack => {
                "M 3 18 C 6 18 7 7 10 7 C 13 7 15 18 21 18 M 12 4 L 12 20 M 10 4 L 14 4 M 10 20 L 14 20"
            }
            Tool::Split => {
                "M 12 3 L 12 21 M 3 9 L 8 9 L 8 15 L 3 15 M 21 9 L 16 9 L 16 15 L 21 15"
            }
            Tool::Draw => "M 4 20 L 5 15 L 17 3 L 21 7 L 9 19 Z M 5 15 L 9 19",
            Tool::Hand => {
                "M 8 13 L 8 6 C 8 3 11 3 11 6 L 11 11 L 11 4 C 11 1 14 1 14 4 L 14 11 L 14 6 C 14 3 17 3 17 6 L 17 12 L 17 9 C 17 6 20 6 20 9 L 20 16 C 20 20 17 22 14 22 L 12 22 C 10 22 8 20 7 18 L 3 13 C 2 11 4 10 6 12 L 8 14"
            }
            Tool::Zoom => "",
        },
    }
}

pub fn sym(d: &'static str) -> TablerIcon {
    TablerIcon { filled: false, d }
}

pub fn nearest(p: f32, root: u8, scale: u8) -> f32 {
    let allowed: &[i32] = match scale {
        1 => &[0, 2, 4, 5, 7, 9, 11],
        2 => &[0, 2, 3, 5, 7, 8, 10],
        _ => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    };
    (0..=127)
        .filter(|n| allowed.contains(&((*n - i32::from(root)).rem_euclid(12))))
        .min_by(|a, b| (*a as f32 - p).abs().total_cmp(&(*b as f32 - p).abs()))
        .unwrap_or(60) as f32
}

pub fn pitch_rect() -> Rect {
    Rect::new(
        KEYS_W,
        GRID_TOP,
        GRID_RIGHT - KEYS_W,
        GRID_BOTTOM - GRID_TOP,
    )
}

pub fn keys_rect() -> Rect {
    Rect::new(0.0, GRID_TOP, KEYS_W, GRID_BOTTOM - GRID_TOP)
}

pub fn ruler_rect() -> Rect {
    Rect::new(KEYS_W, GRID_TOP - RULER_H, GRID_RIGHT - KEYS_W, RULER_H)
}

pub fn time_to_x(l: &Logic, sec: f64) -> f32 {
    let r = pitch_rect();
    r.x + ((sec - l.scroll_sec) * l.px_per_sec) as f32
}

pub fn x_to_time(l: &Logic, x: f32) -> f64 {
    let r = pitch_rect();
    (l.scroll_sec + f64::from(x - r.x) / l.px_per_sec).max(0.0)
}

pub fn note_rect(l: &Logic, d: &Edit, ni: usize) -> Option<Rect> {
    let n = d.notes.get(ni)?;
    let r = pitch_rect();
    let x = time_to_x(l, d.destination(n.start as f64 * STEP));
    let end = time_to_x(l, d.destination(n.end as f64 * STEP));
    let y = r.bottom() - (n.base + n.shift * d.strength - l.low) / l.span * r.h;
    let h = (r.h / l.span * 0.78).max(6.0);
    Some(Rect::new(x, y - h * 0.5, (end - x).max(2.0), h))
}

fn btn_face(c: &mut Canvas, t: &Palette, r: Rect, on: bool, hot: bool, accent: Option<Color>) {
    let fill = if on {
        accent.unwrap_or(t.glyph).with_alpha(52)
    } else if hot {
        t.button_hi
    } else {
        Color::rgb(0x252e34)
    };
    c.fill_rrect(r, 4.0, fill);
    if on {
        let bar = Rect::new(r.x + 6.0, r.bottom() - 2.5, (r.w - 12.0).max(2.0), 1.5);
        c.fill_rrect(bar, 0.75, accent.unwrap_or(t.glyph));
    }
}

pub fn paint(l: &mut Logic, c: &mut Canvas, t: &Palette) {
    l.hits.clear();
    let full = Rect::new(0.0, 0.0, W as f32, H as f32);
    c.fill_rect(full, Color::rgb(0x161d22));

    let doc = l.params.document();
    let edit_opt = doc.edit.as_ref();
    let transferring = l
        .shared
        .transfer_recording
        .load(std::sync::atomic::Ordering::Relaxed);
    let playing = l
        .shared
        .preview_playing
        .load(std::sync::atomic::Ordering::Relaxed);
    let sync_on = l.val(SYNC) >= 0.5;
    let live_on = l.val(LIVE_TUNE) >= 0.5;
    let voice_female = l.val(VOICE) >= 0.5;
    let bypass_on = l.val(BYPASS) >= 0.5 || edit_opt.is_some_and(|d| !d.enabled);

    // 1. Header Bar
    c.fill_rect(
        Rect::new(0.0, 0.0, W as f32, HEADER_H),
        Color::rgb(0x1d262c),
    );
    c.line(
        0.0,
        HEADER_H - 0.5,
        W as f32,
        HEADER_H - 0.5,
        1.0,
        Color::rgb(0x0f1519),
    );
    c.text(
        Font::new(15),
        12.0,
        10.0,
        Color::rgb(0xffcc70),
        "ML VOCAL",
    );
    c.text(
        Font::new(10),
        86.0,
        13.0,
        t.text_dim,
        "PITCH · TIME · FORMANT",
    );

    let mut hx = 215.0;
    let hy = 5.0;
    let mut top_btn =
        |l: &mut Logic, c: &mut Canvas, w: f32, label_txt: &str, tgt: Target, on: bool, col: Option<Color>| {
            let r = Rect::new(hx, hy, w, 27.0);
            hx += w + 5.0;
            let hot = l.hovered(&tgt);
            btn_face(c, t, r, on, hot, col);
            c.text_center(
                Font::new(11),
                r,
                if on { col.unwrap_or(t.text) } else { t.text },
                label_txt,
            );
            l.hits.push((r, tgt));
        };

    top_btn(
        l,
        c,
        if transferring { 98.0 } else { 82.0 },
        if transferring {
            "● Stop Xfer"
        } else {
            "● Transfer"
        },
        Target::Transfer,
        transferring,
        Some(Color::rgb(0xfb655e)),
    );
    top_btn(l, c, 66.0, "Load WAV", Target::Load, false, None);
    top_btn(l, c, 74.0, "Export WAV", Target::Export, false, None);
    top_btn(
        l,
        c,
        64.0,
        if playing { "■ Stop" } else { "▶ Play" },
        Target::Play,
        playing,
        Some(Color::rgb(0x9fe05a)),
    );
    top_btn(
        l,
        c,
        74.0,
        "Host Sync",
        Target::Sync,
        sync_on,
        Some(Color::rgb(0x70c4ff)),
    );
    top_btn(
        l,
        c,
        76.0,
        "Live Tune",
        Target::LiveTune,
        live_on,
        Some(Color::rgb(0xffcc70)),
    );
    if live_on {
        top_btn(
            l,
            c,
            88.0,
            if voice_female {
                "Voice: High"
            } else {
                "Voice: Low"
            },
            Target::Voice,
            true,
            Some(Color::rgb(0xffcc70)),
        );
    }

    // Source file name in header right
    if let Some(d) = edit_opt {
        let name = std::path::Path::new(&d.source)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&d.source);
        let dur = d.frames as f64 / d.rate.max(1) as f64;
        let info = format!(
            "{}  ({:.1}s · {} Hz · {} notes)",
            name,
            dur,
            d.rate,
            d.notes.len()
        );
        c.text_clip(
            Font::new(11),
            hx + 10.0,
            12.0,
            (W as f32 - hx - 20.0).max(40.0),
            t.text_dim,
            &info,
        );
    }

    // 2. Melodyne Tool & Action Bar
    let ty = HEADER_H;
    c.fill_rect(
        Rect::new(0.0, ty, W as f32, TOOLS_H),
        Color::rgb(0x192227),
    );
    c.line(
        0.0,
        ty + TOOLS_H - 0.5,
        W as f32,
        ty + TOOLS_H - 0.5,
        1.0,
        Color::rgb(0x0f1519),
    );

    let mut tx = 8.0;
    let by = ty + 4.5;
    for tool in TOOLS {
        let r = Rect::new(tx, by, 28.0, 27.0);
        tx += 31.0;
        let tgt = Target::Tool(tool);
        let on = l.tool == tool;
        let hot = l.hovered(&tgt);
        btn_face(c, t, r, on, hot, Some(Color::rgb(0xffcc70)));
        c.icon_stroke(
            &icon(tool),
            r.inset(5.0),
            if on { Color::rgb(0xffcc70) } else { t.text },
            1.45,
        );
        l.hits.push((r, tgt));
    }
    tx += 6.0;

    let key_idx = (l.val(KEY).round() as usize).min(11);
    let scale_idx = (l.val(SCALE).round() as usize).min(2);
    let key_name = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][key_idx];
    let scale_name = ["Chromatic", "Major", "Minor"][scale_idx];

    for (w, txt, tgt, on) in [
        (52.0, format!("Key {key_name}"), Target::Key, false),
        (74.0, scale_name.to_string(), Target::Scale, false),
    ] {
        let r = Rect::new(tx, by, w, 27.0);
        tx += w + 4.0;
        let hot = l.hovered(&tgt);
        btn_face(c, t, r, on, hot, None);
        c.text_center(Font::new(11), r, t.text, &txt);
        l.hits.push((r, tgt));
    }
    tx += 4.0;

    let extras = [
        (Target::Correct, "M 4 12 L 9 17 L 20 5", false),
        (
            Target::Join,
            "M 3 9 L 21 9 L 21 15 L 3 15 Z M 9 6 L 9 18 M 15 6 L 15 18",
            false,
        ),
        (
            Target::Snap,
            "M 5 4 L 5 13 C 5 23 19 23 19 13 L 19 4 L 15 4 L 15 13 C 15 17 9 17 9 13 L 9 4 Z",
            l.snap,
        ),
        (
            Target::Mute,
            "M 4 9 L 8 9 L 13 4 L 13 20 L 8 15 L 4 15 Z M 17 9 L 23 15 M 23 9 L 17 15",
            false,
        ),
        (
            Target::Bypass,
            "M 12 2 L 12 12 M 6 5 C 0 9 2 22 12 22 C 22 22 24 9 18 5",
            bypass_on,
        ),
        (
            Target::ResetNote,
            "M 5 5 L 5 11 L 11 11 M 5 11 C 8 1 22 5 21 14 C 20 21 11 23 6 18",
            false,
        ),
        (
            Target::Reanalyse,
            "M 3 11 L 3 16 M 7 6 L 7 20 M 12 3 L 12 22 M 17 7 L 17 18 M 21 11 L 21 15",
            false,
        ),
        (
            Target::Undo,
            "M 9 5 L 3 11 L 9 17 M 3 11 L 14 11 Q 21 11 21 20",
            false,
        ),
        (
            Target::Redo,
            "M 15 5 L 21 11 L 15 17 M 21 11 L 10 11 Q 3 11 3 20",
            false,
        ),
    ];
    for (tgt, path, on) in extras {
        let r = Rect::new(tx, by, 28.0, 27.0);
        tx += 31.0;
        let hot = l.hovered(&tgt);
        btn_face(c, t, r, on, hot, Some(Color::rgb(0xffcc70)));
        c.icon_stroke(
            &sym(path),
            r.inset(5.0),
            if on { Color::rgb(0xffcc70) } else { t.text },
            1.45,
        );
        l.hits.push((r, tgt));
    }

    let reset_all_r = Rect::new(tx + 4.0, by, 68.0, 27.0);
    let hot = l.hovered(&Target::ResetAll);
    btn_face(c, t, reset_all_r, false, hot, None);
    c.text_center(Font::new(11), reset_all_r, t.text, "Reset All");
    l.hits.push((reset_all_r, Target::ResetAll));

    // 3. Draggable Parameter Inspector Bar
    let iy = HEADER_H + TOOLS_H;
    c.fill_rect(
        Rect::new(0.0, iy, W as f32, INSPECTOR_H),
        Color::rgb(0x161f24),
    );
    c.line(
        0.0,
        iy + INSPECTOR_H - 0.5,
        W as f32,
        iy + INSPECTOR_H - 0.5,
        1.0,
        Color::rgb(0x0f1519),
    );

    let fields = [
        (Field::Pitch, "PITCH"),
        (
            if l.tool == Tool::Transition {
                Field::Transition
            } else {
                Field::Modulation
            },
            if l.tool == Tool::Transition {
                "TRANSITION"
            } else {
                "MODULATION"
            },
        ),
        (Field::Drift, "DRIFT"),
        (Field::Formant, "NOTE FORMANT"),
        (Field::Amplitude, "NOTE LEVEL"),
        (Field::Strength, "CORRECTION"),
        (Field::GlobalFormant, "GLOBAL FORMANT"),
        (Field::Offset, "DAW OFFSET"),
        (Field::Gain, "OUTPUT GAIN"),
    ];
    let cell_w = (W as f32 - 12.0) / fields.len() as f32;
    for (j, (field, title)) in fields.into_iter().enumerate() {
        let rr = Rect::new(6.0 + j as f32 * cell_w, iy + 4.0, cell_w - 4.0, 30.0);
        let tgt = Target::Field(field);
        let hot = l.hovered(&tgt);
        c.fill_rrect(
            rr,
            3.0,
            if hot {
                Color::rgb(0x2a363e)
            } else {
                Color::rgb(0x212b31)
            },
        );
        c.text(Font::new(9), rr.x + 6.0, rr.y + 2.0, t.text_dim, title);
        let val_str = l.inspector_value(field, edit_opt, doc.start_seconds);
        let enabled = !l.selected.is_empty()
            || matches!(
                field,
                Field::Strength | Field::GlobalFormant | Field::Offset | Field::Gain
            );
        c.text_clip(
            Font::new(12),
            rr.x + 6.0,
            rr.y + 13.0,
            rr.w - 10.0,
            if enabled { t.text } else { t.text_faint },
            &val_str,
        );
        l.hits.push((rr, tgt));
    }

    // 4. Timeline Ruler
    let rr = ruler_rect();
    c.fill_rect(Rect::new(0.0, rr.y, KEYS_W, rr.h), Color::rgb(0x1b2328));
    c.fill_rect(rr, Color::rgb(0x11181c));
    c.push_clip(rr);
    let bpm = doc.bpm.clamp(20.0, 400.0);
    let sec_per_beat = 60.0 / bpm;
    let px_per_beat = l.px_per_sec * sec_per_beat;
    let step_beats = if px_per_beat < 12.0 {
        16.0
    } else if px_per_beat < 28.0 {
        4.0
    } else if px_per_beat < 70.0 {
        1.0
    } else {
        0.25
    };
    let step_sec = step_beats * sec_per_beat;
    let first_step = (l.scroll_sec / step_sec).floor() as i64;
    let max_sec = l.scroll_sec + f64::from(rr.w) / l.px_per_sec;
    let mut idx = first_step.max(0);
    while (idx as f64) * step_sec <= max_sec {
        let sec = idx as f64 * step_sec;
        let x = time_to_x(l, sec);
        let beat = (sec / sec_per_beat).round() as i64;
        let is_bar = beat % 4 == 0 && (sec - beat as f64 * sec_per_beat).abs() < 0.001;
        c.line(
            x,
            if is_bar { rr.y + 3.0 } else { rr.bottom() - 6.0 },
            x,
            rr.bottom(),
            1.0,
            if is_bar {
                Color::rgb(0x52656e)
            } else {
                Color::rgb(0x313f47)
            },
        );
        if is_bar {
            let bar_num = beat / 4 + 1;
            c.text(
                Font::new(10),
                x + 4.0,
                rr.y + 4.0,
                Color::rgb(0x8aa1ab),
                &format!("Bar {bar_num} ({sec:.1}s)"),
            );
        }
        idx += 1;
    }
    let playhead_x = time_to_x(l, l.playhead_sec());
    if playhead_x >= rr.x && playhead_x <= rr.right() {
        c.fill_rect(
            Rect::new(playhead_x - 4.0, rr.bottom() - 7.0, 8.0, 7.0),
            Color::rgb(0x9fe05a),
        );
    }
    c.pop_clip();
    l.hits.push((rr, Target::Ruler));

    // 5. Left Piano Keys Sidebar
    let kr = keys_rect();
    c.push_clip(kr);
    c.fill_rect(kr, Color::rgb(0x344047));
    let (low, span) = (l.low, l.span);
    let row_h = kr.h / span;
    let key_font = Font::new(10);
    for p in low.floor() as i32..=(low + span).ceil() as i32 {
        let y = kr.bottom() - (p as f32 - low) / span * kr.h;
        let black = [1, 3, 6, 8, 10].contains(&p.rem_euclid(12));
        c.fill_rect(
            Rect::new(
                kr.x,
                y - row_h / 2.0,
                if black { kr.w * 0.63 } else { kr.w },
                row_h,
            ),
            Color::rgb(if black { 0x1b2328 } else { 0x46535a }),
        );
        c.line(
            kr.x,
            y + row_h / 2.0,
            kr.right(),
            y + row_h / 2.0,
            0.6,
            Color::rgb(0x2b3439),
        );
        if row_h >= 10.0 || p.rem_euclid(12) == 0 {
            let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
            c.text(
                key_font,
                kr.right() - 29.0,
                y - 6.0,
                t.text,
                &format!("{}{}", names[p.rem_euclid(12) as usize], p / 12 - 1),
            );
        }
    }
    c.pop_clip();
    l.hits.push((kr, Target::Keys));

    // 6. Main Pitch & Blob Canvas
    let pr = pitch_rect();
    let py = |p: f32| pr.bottom() - (p - low) / span * pr.h;
    c.push_clip(pr);
    c.fill_rect(pr, Color::rgb(0x202b31));
    l.hits.push((pr, Target::Surface));

    // Pitch rows
    for p in low.floor() as i32..=(low + span).ceil() as i32 {
        let yy = py(p as f32);
        let h = pr.h / span;
        let allowed = nearest(p as f32, key_idx as u8, scale_idx as u8) == p as f32;
        c.fill_rect(
            Rect::new(pr.x, yy - h / 2.0, pr.w, h),
            Color::rgb(if !allowed {
                0x182127
            } else if [1, 3, 6, 8, 10].contains(&p.rem_euclid(12)) {
                0x1c262c
            } else {
                0x253138
            }),
        );
        c.line(
            pr.x,
            yy + h / 2.0,
            pr.right(),
            yy + h / 2.0,
            0.6,
            Color::rgb(if p.rem_euclid(12) == 0 {
                0x50616a
            } else {
                0x34434c
            }),
        );
    }

    // Vertical time grid lines
    let mut idx = first_step.max(0);
    while (idx as f64) * step_sec <= max_sec {
        let sec = idx as f64 * step_sec;
        let x = time_to_x(l, sec);
        let beat = (sec / sec_per_beat).round() as i64;
        let is_bar = beat % 4 == 0 && (sec - beat as f64 * sec_per_beat).abs() < 0.001;
        c.line(
            x,
            pr.y,
            x,
            pr.bottom(),
            if is_bar { 0.8 } else { 0.5 },
            Color::rgb(if is_bar { 0x3b4d57 } else { 0x2a3840 }),
        );
        idx += 1;
    }

    let ink = t.glyph.mix(Color::rgb(0xffcc70), 0.82);
    if let Some(d) = edit_opt {
        let pitches = l.cached_contour(d);
        for (ni, note) in d.notes.iter().enumerate() {
            let Some(nr) = note_rect(l, d, ni) else {
                continue;
            };
            if nr.intersect(&pr).is_empty() {
                continue;
            }
            let selected = l.selected.contains(&ni);
            let color = if note.muted { t.text_faint } else { ink };
            let opacity = if bypass_on {
                45
            } else if selected {
                205
            } else {
                110
            };
            let radius = nr.h / 2.0;
            c.fill_rrect(nr, radius, color.with_alpha(opacity));
            c.stroke_rrect(
                nr,
                radius,
                0.9,
                color.with_alpha(if selected { 255 } else { 175 }),
            );
            if note.muted {
                c.line(
                    nr.x + 3.0,
                    nr.cy(),
                    nr.right() - 3.0,
                    nr.cy(),
                    1.0,
                    t.text_dim,
                );
            }
            if nr.w > 26.0 && nr.h >= 12.0 {
                let p_val = (note.base + note.shift * d.strength).round() as i32;
                let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
                let note_lbl = format!("{}{}", names[p_val.rem_euclid(12) as usize], p_val / 12 - 1);
                c.text_clip(
                    Font::new(10),
                    nr.x + 6.0,
                    nr.cy() - 6.0,
                    nr.w - 10.0,
                    Color::rgb(0x11181c),
                    &note_lbl,
                );
            }
            let hit_r = nr.intersect(&pr);
            l.hits.push((hit_r, Target::Note(ni)));
            if matches!(l.tool, Tool::Main | Tool::Time | Tool::Split)
                && (nr.w > 12.0 || l.tool == Tool::Time)
            {
                let edge = Rect::new(nr.right() - 5.0, nr.y, 5.0, nr.h).intersect(&pr);
                if !edge.is_empty() {
                    l.hits.push((edge, Target::Boundary(ni)));
                }
                if selected {
                    c.line(
                        nr.right() - 3.0,
                        nr.y + 2.0,
                        nr.right() - 3.0,
                        nr.bottom() - 2.0,
                        1.0,
                        color,
                    );
                }
            }
        }

        // Draw original (dim) and corrected (bright gold) F0 pitch contour
        let t_a = l.scroll_sec.max(0.0);
        let t_b = max_sec.max(t_a + 0.01);
        let lo = (d.original(t_a).max(0.0) / STEP) as usize;
        let hi = ((d.original(t_b).max(0.0) / STEP) as usize + 1).min(d.f0.len());
        let stride = ((hi.saturating_sub(lo)) as f32 / pr.w.max(1.0))
            .ceil()
            .max(1.0) as usize;
        let mut previous = None;
        let mut dry = None;
        for i in (lo..hi).step_by(stride) {
            let x = time_to_x(l, d.destination(i as f64 * STEP));
            if d.f0[i] > 0.0 {
                let yy = py(pitches[i]);
                let original = py(69.0 + 12.0 * (d.f0[i] / 440.0).log2());
                if let Some((px, prev)) = dry {
                    c.line(px, prev, x, original, 0.75, t.text.with_alpha(52));
                }
                if let Some((px, prev)) = previous {
                    c.line(px, prev, x, yy, 1.35, Color::rgb(0xffe5ac));
                }
                previous = Some((x, yy));
                dry = Some((x, original));
            } else {
                previous = None;
                dry = None;
            }
        }
    } else {
        let msg = if transferring {
            "Recording incoming audio from DAW... Press Stop Xfer or stop DAW playback to analyze notes."
        } else if live_on {
            "Live Auto-Tune is active on incoming audio. Click 'Transfer' or 'Load WAV' to edit vocal blobs."
        } else {
            "Click '● Transfer' and play your DAW track, or click 'Load WAV' (or drop an audio file) to edit vocal pitch & time."
        };
        c.text_center(
            Font::new(13),
            pr,
            t.text_dim,
            msg,
        );
    }

    // Playhead line
    if playhead_x >= pr.x && playhead_x <= pr.right() {
        c.line(
            playhead_x,
            pr.y,
            playhead_x,
            pr.bottom(),
            1.4,
            Color::rgb(0x9fe05a),
        );
    }

    // Marquee selection box
    if let Some(boxr) = l.marquee {
        c.fill_rect(boxr, ink.with_alpha(28));
        c.stroke_rrect(boxr, 1.0, 1.0, ink.with_alpha(165));
    }
    c.pop_clip();

    // 7. Vertical Pitch Scrollbar
    let rail = Rect::new(pr.right() + 3.0, pr.y, 10.0, pr.h);
    c.fill_rrect(rail, 4.0, Color::rgb(0x0e1519));
    let thumb = Rect::new(
        rail.x + 1.5,
        rail.y + (128.0 - low - span) / 128.0 * rail.h,
        7.0,
        (span / 128.0 * rail.h).max(10.0),
    );
    c.fill_rrect(thumb, 3.0, Color::rgb(0x657780));
    l.hits.push((rail, Target::Nav(NavKind::Vertical)));

    // 8. Horizontal Time Scrollbar + Zoom Controls
    let overview = Rect::new(pr.x, pr.bottom() + 5.0, pr.w - 145.0, 15.0);
    c.fill_rrect(overview, 3.0, Color::rgb(0x0f171c));
    let total_sec = edit_opt
        .map(|d| (d.frames as f64 / d.rate.max(1) as f64).max(4.0))
        .unwrap_or(16.0);
    let view_sec = f64::from(pr.w) / l.px_per_sec;
    let total_span = (total_sec + view_sec * 0.25).max(view_sec);
    let tw = ((view_sec / total_span) as f32 * overview.w).clamp(16.0, overview.w);
    let tx_bar = ((l.scroll_sec / total_span) as f32 * overview.w).clamp(0.0, overview.w - tw);
    let hthumb = Rect::new(overview.x + tx_bar, overview.y + 2.5, tw, 10.0);
    c.fill_rrect(hthumb, 3.0, Color::rgb(0x526772));
    l.hits.push((overview, Target::Nav(NavKind::Horizontal)));

    for (j, (lbl, tgt)) in [
        ("−", Target::XZoom(-1)),
        ("+", Target::XZoom(1)),
        ("Fit", Target::Fit),
    ]
    .into_iter()
    .enumerate()
    {
        let zr = Rect::new(pr.right() - 136.0 + j as f32 * 45.0, pr.bottom() + 3.0, 41.0, 19.0);
        let hot = l.hovered(&tgt);
        btn_face(c, t, zr, false, hot, None);
        c.text_center(Font::new(11), zr, t.text, lbl);
        l.hits.push((zr, tgt));
    }

    // 9. Bottom Status & Hint Bar
    let sy = 594.0;
    c.fill_rect(
        Rect::new(0.0, sy, W as f32, H as f32 - sy),
        Color::rgb(0x12191d),
    );
    let busy = l
        .shared
        .busy
        .load(std::sync::atomic::Ordering::Relaxed);
    let status = l.status_line();
    c.text_clip(
        Font::new(11),
        10.0,
        sy + 5.0,
        W as f32 - 90.0,
        if busy { Color::rgb(0xffcc70) } else { t.text_dim },
        &status,
    );
    if busy {
        let cr = Rect::new(W as f32 - 74.0, sy + 2.5, 64.0, 21.0);
        let hot = l.hovered(&Target::Cancel);
        btn_face(c, t, cr, false, hot, Some(Color::rgb(0xfb655e)));
        c.text_center(Font::new(11), cr, t.text, "Cancel");
        l.hits.push((cr, Target::Cancel));
    }

    // 10. Right-Click Floating Tool Palette
    if let Some((px, py)) = l.palette_pos {
        let pw = 330.0;
        let ph = 230.0;
        let menu = Rect::new(
            px.clamp(pr.x + 4.0, pr.right() - pw - 4.0),
            py.clamp(pr.y + 4.0, pr.bottom() - ph - 4.0),
            pw,
            ph,
        );
        c.fill_rrect(menu, 6.0, Color::rgb(0x10191f));
        c.stroke_rrect(menu, 6.0, 1.0, Color::rgb(0x667881));
        let names = [
            "Main",
            "Pitch",
            "Modulation",
            "Drift",
            "Transition",
            "Formant",
            "Amplitude",
            "Timing",
            "Time handle",
            "Separation",
            "Draw",
            "Hand",
            "Zoom",
        ];
        for (j, tool) in TOOLS.into_iter().enumerate() {
            let rr = Rect::new(
                menu.x + 5.0 + (j % 2) as f32 * (pw - 10.0) / 2.0,
                menu.y + 6.0 + (j / 2) as f32 * 27.0,
                (pw - 12.0) / 2.0,
                25.0,
            );
            let tgt = Target::Tool(tool);
            let hot = l.hovered(&tgt);
            if hot || l.tool == tool {
                c.fill_rrect(rr, 3.0, t.glyph.with_alpha(38));
            }
            let glyph = Rect::new(rr.x + 4.0, rr.y + 3.0, 19.0, 19.0);
            c.icon_stroke(&icon(tool), glyph, t.text, 1.4);
            c.text(Font::new(12), rr.x + 30.0, rr.y + 5.0, t.text, names[j]);
            l.hits.push((rr, tgt));
        }
        for (j, (name, tgt)) in [
            ("Correct", Target::Correct),
            ("Join", Target::Join),
            ("Reset note", Target::ResetNote),
            ("Reset all", Target::ResetAll),
        ]
        .into_iter()
        .enumerate()
        {
            let br = Rect::new(
                menu.x + 5.0 + j as f32 * (pw - 10.0) / 4.0,
                menu.bottom() - 30.0,
                (pw - 14.0) / 4.0,
                24.0,
            );
            let hot = l.hovered(&tgt);
            btn_face(c, t, br, false, hot, None);
            c.text_center(Font::new(11), br, t.text, name);
            l.hits.push((br, tgt));
        }
    }
}
