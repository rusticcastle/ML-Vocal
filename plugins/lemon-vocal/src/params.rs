// SPDX-License-Identifier: GPL-3.0-or-later
//! Host-visible parameters and persistent vocal editing state.
use nih_plug::prelude::*;
use os_model::vocal::Edit;
use os_plugkit::live::LiveEdits;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};

pub const STRENGTH: usize = 0;
pub const FORMANT: usize = 1;
pub const KEY: usize = 2;
pub const SCALE: usize = 3;
pub const SYNC: usize = 4;
pub const LIVE_TUNE: usize = 5;
pub const VOICE: usize = 6;
pub const BYPASS: usize = 7;
pub const GAIN: usize = 8;

pub struct Def {
    pub id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub step: f32,
}

pub fn defs() -> Vec<Def> {
    [
        ("strength", "Correction", 0.0, 1.0, 1.0, 0.01),
        ("formant", "Formant st", -12.0, 12.0, 0.0, 0.1),
        ("key", "Key Root", 0.0, 11.0, 0.0, 1.0),
        ("scale", "Scale", 0.0, 2.0, 0.0, 1.0),
        ("sync", "Host Sync", 0.0, 1.0, 1.0, 1.0),
        ("live_tune", "Live Auto-Tune", 0.0, 1.0, 0.0, 1.0),
        ("voice", "Voice Range", 0.0, 1.0, 0.0, 1.0),
        ("bypass", "Bypass", 0.0, 1.0, 0.0, 1.0),
        ("gain", "Output dB", -24.0, 24.0, 0.0, 0.1),
    ]
    .into_iter()
    .map(|(id, name, min, max, default, step)| Def {
        id: id.into(),
        name: name.into(),
        min,
        max,
        default,
        step,
    })
    .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VocalDoc {
    pub edit: Option<Edit>,
    pub start_seconds: f64,
    pub bpm: f64,
}

impl Default for VocalDoc {
    fn default() -> Self {
        Self {
            edit: None,
            start_seconds: 0.0,
            bpm: 120.0,
        }
    }
}

impl VocalDoc {
    pub fn normalize(&mut self) {
        if !self.start_seconds.is_finite() || self.start_seconds < 0.0 {
            self.start_seconds = 0.0;
        }
        if !self.bpm.is_finite() {
            self.bpm = 120.0;
        }
        self.bpm = self.bpm.clamp(20.0, 400.0);
        if let Some(edit) = &self.edit {
            if !edit.valid() {
                self.edit = None;
            }
        }
    }
}

pub fn cache_dir() -> PathBuf {
    os_paths::user_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("VocalCaptures")
}

pub struct VocalParams {
    pub p: Vec<FloatParam>,
    pub defs: Vec<Def>,
    pub doc: RwLock<Arc<VocalDoc>>,
    pub live: Arc<LiveEdits>,
}

impl Default for VocalParams {
    fn default() -> Self {
        let defs = defs();
        let p = defs
            .iter()
            .map(|d| {
                FloatParam::new(
                    &d.name,
                    d.default,
                    FloatRange::Linear {
                        min: d.min,
                        max: d.max,
                    },
                )
                .with_step_size(d.step)
            })
            .collect();
        Self {
            p,
            defs,
            doc: RwLock::new(Arc::new(VocalDoc::default())),
            live: Arc::default(),
        }
    }
}

impl VocalParams {
    pub fn v(&self, i: usize) -> f32 {
        self.live.value(&self.p[i])
    }

    pub fn document(&self) -> Arc<VocalDoc> {
        self.doc.read().unwrap().clone()
    }

    pub fn set_doc(&self, mut d: VocalDoc) {
        d.normalize();
        *self.doc.write().unwrap() = Arc::new(d);
    }
}

unsafe impl Params for VocalParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        self.p
            .iter()
            .zip(&self.defs)
            .map(|(p, d)| (d.id.clone(), p.as_ptr(), String::new()))
            .collect()
    }

    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let doc = (*self.document()).clone();
        BTreeMap::from([(
            "vocal-document".into(),
            serde_json::to_string(&doc).unwrap_or_default(),
        )])
    }

    fn deserialize_fields(&self, s: &BTreeMap<String, String>) {
        if let Some(d) = s
            .get("vocal-document")
            .and_then(|v| serde_json::from_str::<VocalDoc>(v).ok())
        {
            self.set_doc(d);
        }
    }
}
