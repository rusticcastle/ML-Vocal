# ML Vocal — Standalone Melodyne-Style Vocal Editor & Live Auto-Tune (VST3 / CLAP)

**ML Vocal** is a native, open-source (**GPL-3.0-or-later**) monophonic vocal pitch, formant, expression, and timing editor — plus a real-time low-latency Auto-Tune processor — built in Rust and C++17 as a standalone **VST3** and **CLAP** plugin for **Linux**, **macOS**, and **Windows**.

Extracted and adapted from the ML Studio vocal editor, **ML Vocal** statically embeds both the **WORLD** high-precision vocoder (Harvest F0 + CheapTrick spectral envelope + D4C aperiodicity) and the **SnapTune** real-time pitch corrector into a single zero-dependency plugin binary.

---

## Key Features

- **Full 13-Tool Melodyne-Style Editing Workflow:**
  1. **Main Tool (`1`):** Context-aware smart tool — drag note body for pitch, drag left/right edges for note timing, double-click upper edge to split, double-click body to snap to scale.
  2. **Pitch Tool (`2`):** Fine or quantized semitone/cent pitch shifting (`Alt`-drag for fine cents override).
  3. **Modulation Tool (`3`):** Scale natural vibrato & pitch modulation from `0%` (robotic flat) to `200%` (exaggerated).
  4. **Pitch Drift Tool (`4`):** Tame slow pitch drift within sustained notes (`0%` = rock-solid center, `100%` = natural drift).
  5. **Pitch Transition Tool (`5`):** Control portamento/glide speed between adjacent notes.
  6. **Formant Tool (`6`):** Independently shift vocal tract formants (`-12 st` to `+12 st`) per note or globally without altering pitch.
  7. **Amplitude Tool (`7`):** Adjust per-note volume (`-24 dB` to `+18 dB`) or mute individual notes/breaths.
  8. **Time Tool (`8`):** Shift note timing horizontally (`-500 ms` to `+500 ms`) using pitch-preserving **WSOLA** time-warping.
  9. **Attack Speed Tool (`9`):** Speed up or slow down note onset transients (`-100%` to `+100%`).
  10. **Split Note Tool (`0`):** Slice notes at any 5 ms frame boundary; merge adjacent notes with **Join**.
  11. **Freehand Pitch Draw Tool:** Draw custom pitch curves directly over frames (`5 ms` resolution).
  12. **Hand Tool:** Smooth 2D pan across time and MIDI pitch.
  13. **Zoom Tool:** Interactive horizontal time zoom and vertical pitch zoom.
- **3 Studio Operating Modes:**
  - **DAW Transfer Mode (`● Transfer`):** Capture audio directly from your DAW track at the exact bar/beat transport position, automatically segment into editable note blobs, and play back locked to your DAW timeline (`Host Sync`).
  - **File Loader & Exporter (`Load WAV` / `Export WAV`):** Load or drag-and-drop `.wav`, `.flac`, `.mp3`, or `.ogg` vocal stems directly into the editor, audition with `▶ Preview` or piano-roll scrubbing, and export 32-bit float `.wav` files.
  - **Live Auto-Tune Mode (`Live Auto-Tune`):** Zero-setup real-time pitch & formant correction quantized to your chosen **Root** (`C..B`) and **Scale** (`Chromatic`, `Major`, `Minor`, `PentMaj`, `PentMin`, `Blues`, `Dorian`, `Mixolydian`, `HarmMin`).
- **Incremental Region Resynthesis:** Editing a single note re-renders only the affected time slice (`+80 ms` context padding) on a cancellable background worker thread and crossfades it seamlessly into playback.

---

## Building & Installing from Source

### Prerequisites
- **Rust** (stable toolchain with `cargo`)
- **C++17 compiler** (`g++` or `clang++` on Linux/macOS, MSVC on Windows)
- On Linux: standard X11 headers (only `libc` / X11 runtime; `x11rb` talks to the X server in pure Rust)

### Build & Install (`~/.vst3` and `~/.clap` on Linux)
```bash
# Build release VST3 & CLAP bundles and install to your user plugin directories
cargo run -p bundle-plugins -- --install
```

The compiled plugin bundles are also placed in:
- `target/bundled/ML Vocal.vst3`
- `target/bundled/ML Vocal.clap`

### Run Unit & DSP Tests
```bash
cargo test -p lemon-vocal
```

---

## Keyboard & Mouse Shortcuts

| Shortcut | Action |
| :--- | :--- |
| **`1` – `9`, `0`** | Select editing tool (`1` Main, `2` Pitch, `3` Modulation, `4` Drift, `5` Transition, `6` Formant, `7` Amplitude, `8` Time, `9` Attack, `0` Split) |
| **Right-Click on Grid** | Open floating Quick Tool Palette at cursor |
| **Drag Note (`Main` / `Pitch`)** | Shift pitch in semitones (hold `Alt` for fine cent adjustment without scale snap) |
| **Double-Click Note** | Snap note to nearest active scale pitch (or reset active tool parameter) |
| **Double-Click Note Top Edge** | Split note at cursor |
| **Click / Drag Piano Roll** | Audition reference MIDI pitch |
| **Click / Drag Timeline Ruler** | Scrub playhead position |
| **`Space`** | Toggle `▶ Preview` playback |
| **`Ctrl+Z` / `Ctrl+Y`** | Undo / Redo |
| **`Ctrl+A`** | Select all notes |
| **`Delete` / `Backspace`** | Reset selected notes to original pitch/parameters |
| **`Mouse Wheel`** | Scroll pitch (`Shift+Wheel`: scroll time; `Ctrl+Wheel`: zoom time; `Ctrl+Shift+Wheel`: zoom pitch) |

---

## Architecture & Vibe Coding Journal

See [ML_VOCAL_JOURNAL.md](ML_VOCAL_JOURNAL.md) for a complete walkthrough of the DSP pipeline, static C++ FFI bridge, incremental region resynthesis, WSOLA time-warping, and GUI architecture.

## Credits & Acknowledgments

- **[Michael Lane](https://michaellanemusic.de/ml-studio/)** ([Dorfhaus/ML-Studio](https://github.com/Dorfhaus/ML-Studio)) — Maintainer of **ML Studio**, from which this standalone vocal plugin was extracted and adapted.
- **[ItsAlphaNeon and contributors](https://github.com/ItsAlphaNeon/Lemon-Studio)** — Creators of **Lemon Studio**, the foundational open-source DAW architecture, UI toolkit (`os-ui`, `os-plugkit`, `os-theme`), and vocal engine model (`os-model`).
- **Masanori Morise** — Creator of the **WORLD** high-quality speech analysis, manipulation, and synthesis vocoder (`tools/vocal-engine/vendor/world`, Modified BSD License).
- **Robbert van der Helm** — Creator of [`nih-plug`](https://github.com/robbert-vdh/nih-plug), the Rust VST3 and CLAP plugin framework.

## License

Licensed under **GPL-3.0-or-later** ([LICENSE](LICENSE)).  
Includes the **WORLD** vocoder (`tools/vocal-engine/vendor/world`, Modified BSD License by Masanori Morise).
