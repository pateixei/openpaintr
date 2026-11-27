//! Per-tool performance baseline (architecture plan item 0.3).
//!
//! ```sh
//! cargo test --release --locked --bin openpaintr perf_baseline -- --ignored --nocapture
//! ```
//!
//! The parent test runs every case in its own process, so each `VmHWM` is
//! independent, and writes JSON lines to `target/perf/baseline.jsonl`.
//! `OPENPAINTR_PERF_RUNS` overrides the number of timed runs.
//!
//! A frame time is one `ctx.run(ui_frame)`: UI, preview, compositing, and
//! texture conversion, without tessellation or GPU upload. Every case checks
//! that the timed frames did their work; those checks are not timed.

use std::{io::Write as _, process::Command as Process, time::Instant};

use serde_json::json;

use super::{super::transform::Transform, *};

const CASE_ENV: &str = "OPENPAINTR_PERF_CASE";
const OUTPUT_ENV: &str = "OPENPAINTR_PERF_OUTPUT";
const RUNS_ENV: &str = "OPENPAINTR_PERF_RUNS";
const TEST_NAME: &str = "gui::tests::perf_baseline::perf_baseline";
const WARMUPS: usize = 2;
const RUNS: usize = 10;
const DRAG_FRAMES: usize = 10;
const FREEFORM_FRAMES: usize = 60;
const TYPED: &str = "OpenPaintR 123";

/// `(name, width, height, layers)`.
const DOCUMENTS: [(&str, u32, u32, usize); 2] =
    [("typical", 800, 600, 4), ("large", 2560, 1440, 8)];

/// `(workload, variant)`.
const CASES: [(&str, &str); 27] = [
    ("phase1", "default"),
    ("transform_layer", "nearest"),
    ("transform_floating", "bilinear-64x64"),
    ("rotate_zoom", "nearest"),
    ("gradient", "linear"),
    ("gradient", "conical"),
    ("shape_star", "star5"),
    ("shape_freeform", "spiral"),
    ("text", "24px"),
    ("text", "96px"),
    ("text", "240px"),
    ("bucket", "contiguous"),
    ("bucket", "global"),
    ("magic_wand", "default"),
    ("clone_stamp", "default"),
    ("layer_opacity", "default"),
    ("thumbnails", "after-stroke"),
    ("save_dialog", "pdn-1"),
    ("save_dialog", "pdn-5"),
    ("save_dialog", "pdn-9"),
    ("save_dialog", "png-none"),
    ("save_dialog", "png-fast"),
    ("save_dialog", "png-balanced"),
    ("save_dialog", "png-high"),
    ("pdn_open", "default"),
    ("pdn_save", "default"),
    ("png_export", "default"),
];

/// Samples of one case, grouped by timed run.
#[derive(Default)]
struct Recorder {
    phases: Vec<(&'static str, Vec<Vec<f64>>)>,
    current: Vec<(&'static str, f64)>,
    undo_bytes: Vec<usize>,
    zoom: Option<f32>,
}

impl Recorder {
    fn time<R>(&mut self, phase: &'static str, work: impl FnOnce() -> R) -> R {
        let start = Instant::now();
        let result = work();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        self.current.push((phase, elapsed));
        result
    }

    fn undo_entry(&mut self, app: &App) {
        let bytes = app.tab().editor.last_undo_entry_bytes();
        self.undo_bytes.push(bytes.expect("an undo entry"));
    }

    fn finish_run(&mut self, measured: bool) {
        let frames = std::mem::take(&mut self.current);
        if !measured {
            self.undo_bytes.clear();
            return;
        }
        let mut run: Vec<(&'static str, Vec<f64>)> = Vec::new();
        for (phase, elapsed) in frames {
            match run.iter_mut().find(|(name, _)| *name == phase) {
                Some((_, samples)) => samples.push(elapsed),
                None => run.push((phase, vec![elapsed])),
            }
        }
        for (phase, samples) in run {
            match self.phases.iter_mut().find(|(name, _)| *name == phase) {
                Some((_, runs)) => runs.push(samples),
                None => self.phases.push((phase, vec![samples])),
            }
        }
    }
}

fn updated(output: &egui::FullOutput, texture: Option<&TextureHandle>) -> bool {
    texture.is_some_and(|texture| {
        output
            .textures_delta
            .set
            .iter()
            .any(|(id, _)| *id == texture.id())
    })
}

#[track_caller]
fn assert_preview(driver: &TestDriver, output: &egui::FullOutput) {
    assert!(
        updated(output, driver.app.tab().image.as_ref()),
        "the frame did not update the document texture"
    );
}

/// A driver with the top layer active and the first layout settled.
fn open(bytes: &[u8], recorder: &mut Recorder) -> io::Result<TestDriver> {
    let mut driver = TestDriver::new(Document::read_pdn(bytes)?);
    let top = driver.app.tab().editor.document.layer_count() - 1;
    driver.app.tab_mut().editor.selected = top;
    for _ in 0..3 {
        driver.step();
    }
    recorder.zoom = Some(driver.app.tab().display_zoom);
    Ok(driver)
}

/// Screen position of a point given as a fraction of the document size.
fn at(driver: &TestDriver, x: f32, y: f32) -> egui::Pos2 {
    let rect = driver
        .ctx
        .read_response(egui::Id::new(CANVAS_ID))
        .expect("canvas")
        .rect;
    rect.min + vec2(rect.width() * x, rect.height() * y)
}

fn button(pos: egui::Pos2, button: PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn send(driver: &mut TestDriver, events: Vec<egui::Event>) -> egui::FullOutput {
    driver.run(egui::Modifiers::NONE, events)
}

/// Drags along `path`. The press, the first move (previews lag one frame behind
/// input), and the release are not timed; every later move is a preview frame.
fn drag_preview(
    driver: &mut TestDriver,
    recorder: &mut Recorder,
    pointer: PointerButton,
    path: &[egui::Pos2],
) {
    send(driver, button(path[0], pointer, true));
    send(driver, vec![egui::Event::PointerMoved(path[1])]);
    for &pos in &path[2..] {
        let output = recorder.time("preview", || {
            send(driver, vec![egui::Event::PointerMoved(pos)])
        });
        assert_preview(driver, &output);
    }
    send(driver, button(*path.last().unwrap(), pointer, false));
}

/// Screen positions along a straight line: two untimed plus `DRAG_FRAMES`.
fn line(driver: &TestDriver, from: (f32, f32), to: (f32, f32)) -> Vec<egui::Pos2> {
    let from = at(driver, from.0, from.1);
    let to = at(driver, to.0, to.1);
    let steps = DRAG_FRAMES + 1;
    (0..=steps)
        .map(|step| from.lerp(to, step as f32 / steps as f32))
        .collect()
}

fn commit_session(driver: &mut TestDriver, recorder: &mut Recorder) {
    let before = driver.app.tab().editor.undo_len();
    let output = recorder.time("commit", || {
        driver.send_key(egui::Key::Enter, egui::Modifiers::NONE)
    });
    assert!(driver.app.tab().session.is_none(), "session still open");
    assert_eq!(driver.app.tab().editor.undo_len(), before + 1);
    assert_preview(driver, &output);
    recorder.undo_entry(&driver.app);
}

fn transform(driver: &TestDriver) -> &Transform {
    driver
        .app
        .tab()
        .session
        .as_ref()
        .and_then(ActiveSession::as_transform)
        .expect("transform session")
}

/// Runs one measured iteration of a UI workload.
fn run_once(
    workload: &str,
    variant: &str,
    bytes: &[u8],
    recorder: &mut Recorder,
) -> io::Result<()> {
    match workload {
        "transform_layer" | "transform_floating" | "rotate_zoom" => {
            let mut driver = open(bytes, recorder)?;
            let size = driver.app.tab().editor.document.dimensions();
            match workload {
                "transform_layer" => driver.app.start_transform(TransformTarget::Layer),
                "rotate_zoom" => driver
                    .app
                    .execute_command(&driver.ctx.clone(), Command::RotateZoom),
                _ => {
                    let origin = (size.0 / 2 - 32, size.1 / 2 - 32);
                    let floating = clipboard::Image {
                        size: (64, 64),
                        origin,
                        pixels: (0..64 * 64).map(pattern).collect(),
                    };
                    let min = vec2(origin.0 as f32, origin.1 as f32);
                    let max = min + vec2(64.0, 64.0);
                    let center = (min + max) * 0.5;
                    let mut affine = geometry::Affine::centered(size);
                    affine.center = (f64::from(center.x), f64::from(center.y));
                    driver.app.tab_mut().session = Some(ActiveSession::Transform(
                        Transform::with_floating(affine, (min, max), floating),
                    ));
                    driver.app.tab_mut().editor.mark_full_refresh();
                }
            }
            driver.step();
            // Away from every handle: the left button moves, the right one rotates.
            let rotate = workload == "rotate_zoom";
            let pointer = if rotate {
                PointerButton::Secondary
            } else {
                PointerButton::Primary
            };
            let path = line(&driver, (0.25, 0.3), (0.4, 0.2));
            drag_preview(&mut driver, recorder, pointer, &path);
            let affine = transform(&driver).affine;
            if rotate {
                assert!(affine.degrees.abs() > 1.0, "not rotated");
            } else {
                assert!(affine.shift.0.abs() > 1.0, "not moved");
            }
            commit_session(&mut driver, recorder);
        }
        "gradient" | "shape_star" | "shape_freeform" => {
            let mut driver = open(bytes, recorder)?;
            if workload == "gradient" {
                driver.app.activate_tool(Tool::Gradient);
                driver.app.gradient.shape = match variant {
                    "linear" => GradientShape::Linear,
                    _ => GradientShape::Conical,
                };
            } else {
                driver.app.activate_tool(Tool::DrawShape);
                driver.app.shape.kind = if workload == "shape_star" {
                    ShapeKind::Star5
                } else {
                    ShapeKind::Freeform
                };
                driver.app.shape.fill_mode = ShapeFillMode::Both;
            }
            driver.step();
            let path = match workload {
                "gradient" => line(&driver, (0.5, 0.5), (0.95, 0.8)),
                "shape_star" => line(&driver, (0.05, 0.05), (0.95, 0.95)),
                _ => (0..FREEFORM_FRAMES + 2)
                    .map(|step| {
                        let turn = step as f32 / (FREEFORM_FRAMES + 1) as f32;
                        let angle = turn * 3.0 * std::f32::consts::TAU;
                        let radius = 0.05 + 0.4 * turn;
                        at(
                            &driver,
                            0.5 + radius * angle.cos(),
                            0.5 + radius * angle.sin(),
                        )
                    })
                    .collect(),
            };
            drag_preview(&mut driver, recorder, PointerButton::Primary, &path);
            assert!(driver.app.has_active_session(), "no session");
            commit_session(&mut driver, recorder);
        }
        "text" => {
            let mut driver = open(bytes, recorder)?;
            driver.app.activate_tool(Tool::Text);
            driver.app.text.size_px = variant.trim_end_matches("px").parse().unwrap();
            driver.step();
            let anchor = at(&driver, 0.05, 0.1);
            send(&mut driver, button(anchor, PointerButton::Primary, true));
            send(&mut driver, button(anchor, PointerButton::Primary, false));
            driver.step();
            for character in TYPED.chars() {
                let output = recorder.time("typing", || {
                    send(&mut driver, vec![egui::Event::Text(character.into())])
                });
                assert_preview(&driver, &output);
            }
            let typed = driver
                .app
                .tab()
                .session
                .as_ref()
                .and_then(ActiveSession::as_text)
                .map(|session| session.text.as_str());
            assert_eq!(typed, Some(TYPED));
            commit_session(&mut driver, recorder);
        }
        "bucket" => {
            let mut driver = open(bytes, recorder)?;
            driver.app.activate_tool(Tool::Bucket);
            driver.app.bucket_contiguous = variant == "contiguous";
            driver.step();
            let pos = at(&driver, 0.5, 0.5);
            send(&mut driver, button(pos, PointerButton::Primary, true));
            let before = driver.app.tab().editor.undo_len();
            recorder.time("click", || {
                send(&mut driver, button(pos, PointerButton::Primary, false))
            });
            assert_eq!(driver.app.tab().editor.undo_len(), before + 1);
            recorder.undo_entry(&driver.app);
            let output = recorder.time("refresh", || driver.step());
            assert_preview(&driver, &output);
        }
        "magic_wand" => {
            let mut driver = open(bytes, recorder)?;
            driver.app.activate_tool(Tool::Wand);
            driver.step();
            let pos = at(&driver, 0.5, 0.5);
            send(&mut driver, button(pos, PointerButton::Primary, true));
            recorder.time("click", || {
                send(&mut driver, button(pos, PointerButton::Primary, false))
            });
            assert!(driver.app.tab().editor.selection.is_some(), "no selection");
            recorder.undo_entry(&driver.app);
            recorder.time("refresh", || driver.step());
        }
        "clone_stamp" => {
            let mut driver = open(bytes, recorder)?;
            driver.app.activate_tool(Tool::CloneStamp);
            let size = driver.app.tab().editor.document.dimensions();
            driver.app.tab_mut().clone_anchor = Some((size.0 / 8, size.1 / 8));
            driver.step();
            let path = line(&driver, (0.4, 0.4), (0.8, 0.7));
            let before = driver.app.tab().editor.undo_len();
            recorder.time("start", || {
                send(&mut driver, button(path[0], PointerButton::Primary, true))
            });
            assert!(driver.app.tab().stroke_gesture.is_some(), "no stroke");
            send(&mut driver, vec![egui::Event::PointerMoved(path[1])]);
            for &pos in &path[2..] {
                let output = recorder.time("stroke", || {
                    send(&mut driver, vec![egui::Event::PointerMoved(pos)])
                });
                assert_preview(&driver, &output);
            }
            let end = *path.last().unwrap();
            recorder.time("release", || {
                send(&mut driver, button(end, PointerButton::Primary, false))
            });
            assert_eq!(driver.app.tab().editor.undo_len(), before + 1);
            recorder.undo_entry(&driver.app);
        }
        "layer_opacity" => {
            let mut driver = open(bytes, recorder)?;
            let layer = driver.app.tab().editor.selected;
            let meta = driver.app.tab().editor.layer_meta(layer).expect("layer");
            for step in 1..=DRAG_FRAMES {
                let opacity = 255 - (step * 20) as u8;
                let output = recorder.time("drag", || {
                    driver
                        .app
                        .tab_mut()
                        .editor
                        .set_properties(layer, (true, opacity))
                        .expect("opacity");
                    driver.step()
                });
                assert_preview(&driver, &output);
            }
            driver
                .app
                .tab_mut()
                .editor
                .record_properties(layer, (meta.visible, meta.opacity));
            recorder.undo_entry(&driver.app);
        }
        "thumbnails" => {
            let mut driver = open(bytes, recorder)?;
            for step in 0..DRAG_FRAMES as u32 {
                let tab = driver.app.tab_mut();
                tab.editor.begin_stroke(Pixel([220, 40, 40, 255]), 24);
                tab.editor.draw_to(Some((40 + step * 8, 40 + step * 4)));
                tab.editor.finish_stroke();
                tab.thumbs_dirty = true;
                let output = recorder.time("refresh", || driver.step());
                assert!(
                    updated(&output, driver.app.tab().tab_thumb.as_ref()),
                    "thumbnails not refreshed"
                );
            }
        }
        "save_dialog" => {
            let document = Document::read_pdn(bytes)?;
            let ctx = egui::Context::default();
            let format = save_format(variant);
            let dialog = recorder.time("encode", || {
                SaveOptionsDialog::new("perf".into(), format, false, &document, &ctx)
            });
            assert!(dialog.encoded_len().is_some_and(|len| len > 0));
        }
        "pdn_open" => {
            let ctx = egui::Context::default();
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
            let (app, output) = recorder.time("open", || -> io::Result<_> {
                let mut app = App::new(&ctx, Document::read_pdn(bytes)?);
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let output = ctx.run(input, |ctx| app.ui_frame(ctx));
                Ok((app, output))
            })?;
            assert!(updated(&output, app.tab().image.as_ref()));
        }
        "pdn_save" => {
            let document = Document::read_pdn(bytes)?;
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("perf.pdn");
            recorder.time("save", || write_pdn_atomically(&document, &path, false))?;
            assert!(std::fs::metadata(&path)?.len() > 0);
        }
        "png_export" => {
            let document = Document::read_pdn(bytes)?;
            let mut png = Vec::new();
            recorder.time("export", || document.write_png(&mut png))?;
            assert!(!png.is_empty());
        }
        _ => panic!("unknown workload {workload}"),
    }
    Ok(())
}

fn save_format(variant: &str) -> SaveFormat {
    if let Some(effort) = variant.strip_prefix("pdn-") {
        return SaveFormat::Pdn {
            effort: effort.parse().unwrap(),
        };
    }
    let compression = match variant {
        "png-none" => openpaintr::PngCompression::None,
        "png-fast" => openpaintr::PngCompression::Fast,
        "png-balanced" => openpaintr::PngCompression::Balanced,
        _ => openpaintr::PngCompression::High,
    };
    SaveFormat::Png {
        depth: openpaintr::PngDepth::Rgba32,
        compression,
    }
}

/// A deterministic non-uniform pixel.
fn pattern(index: u32) -> Pixel {
    let mixed = index.wrapping_mul(2_654_435_761);
    let [r, g, b, _] = mixed.to_le_bytes();
    Pixel([r, g, b, 255])
}

/// The phase 1 document. Transforms and Clone Stamp get a patterned top layer:
/// moving or cloning a uniform layer changes almost no pixels.
fn workload_document(
    workload: &str,
    width: u32,
    height: u32,
    layers: usize,
) -> io::Result<Vec<u8>> {
    let bytes = build_workload_pdn(width, height, layers)?;
    if !matches!(workload, "transform_layer" | "rotate_zoom" | "clone_stamp") {
        return Ok(bytes);
    }
    let mut document = Document::read_pdn(bytes.as_slice())?;
    for y in 0..height {
        for x in 0..width {
            document.set_pixel(layers - 1, x, y, pattern(y * width + x))?;
        }
    }
    let mut bytes = Vec::new();
    document.write_pdn(&mut bytes)?;
    Ok(bytes)
}

fn vm_hwm_kib() -> Option<u64> {
    vm_hwm_line().split_whitespace().nth(1)?.parse().ok()
}

fn run_case(case: &str, output: &Path, runs: usize) -> io::Result<()> {
    let mut parts = case.split('|');
    let (workload, variant, name) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    let (_, width, height, layers) = DOCUMENTS
        .into_iter()
        .find(|document| document.0 == name)
        .expect("known document");
    let warmups = WARMUPS.min(runs);
    let mut recorder = Recorder::default();
    if workload == "phase1" {
        let samples = measure_phase1_workload(width, height, layers, warmups, runs)?;
        recorder.phases = vec![
            ("open", samples.open),
            ("stroke", samples.stroke),
            ("nav", samples.nav),
            ("export", samples.export),
        ];
        recorder.undo_bytes = samples.stroke_undo_bytes;
    } else {
        let bytes = workload_document(workload, width, height, layers)?;
        for iteration in 0..warmups + runs {
            run_once(workload, variant, &bytes, &mut recorder)?;
            recorder.finish_run(iteration >= warmups);
        }
    }

    let vm_hwm = vm_hwm_kib();
    let undo = (!recorder.undo_bytes.is_empty())
        .then(|| recorder.undo_bytes.iter().sum::<usize>() / recorder.undo_bytes.len());
    let mut file = std::fs::OpenOptions::new().append(true).open(output)?;
    for (phase, runs) in &recorder.phases {
        let frames: Vec<f64> = runs.iter().flatten().copied().collect();
        let (median, max) = stats(&frames);
        let (run_median, run_max) = stats(&run_means(runs));
        // Only the phase that creates the history entry reports its size.
        let creates_entry = match workload {
            "phase1" => *phase == "stroke",
            "bucket" | "magic_wand" => *phase == "click",
            "clone_stamp" => *phase == "release",
            "layer_opacity" => *phase == "drag",
            _ => *phase == "commit",
        };
        let line = json!({
            "type": "result",
            "schema": 1,
            "workload": workload,
            "variant": variant,
            "document": name,
            "width": width,
            "height": height,
            "layers": layers,
            "zoom": recorder.zoom,
            "phase": phase,
            "samples": frames.len(),
            "median_ms": median,
            "max_ms": max,
            "run_mean_median_ms": run_median,
            "run_mean_max_ms": run_max,
            "vm_hwm_kib": vm_hwm,
            "undo_bytes_per_entry": if creates_entry { undo } else { None },
        });
        writeln!(file, "{line}")?;
        println!("{line}");
    }
    Ok(())
}

fn command_output(program: &str, args: &[&str]) -> String {
    Process::new(program)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || "unknown".into(),
            |output| String::from_utf8_lossy(&output.stdout).trim().into(),
        )
}

fn environment(runs: usize) -> serde_json::Value {
    let read = |path: &str| std::fs::read_to_string(path).unwrap_or_default();
    let cpu = read("/proc/cpuinfo")
        .lines()
        .find_map(|line| line.strip_prefix("model name"))
        .map_or_else(
            || "unknown".into(),
            |value| value.trim_start_matches([' ', '\t', ':']).to_owned(),
        );
    let governor = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
    json!({
        "type": "environment",
        "schema": 1,
        "uname": command_output("uname", &["-a"]),
        "cpu": cpu,
        "governor": governor.trim(),
        "rustc": command_output("rustc", &["--version"]),
        "git_commit": command_output("git", &["rev-parse", "HEAD"]),
        "git_dirty": !command_output("git", &["status", "--porcelain"]).is_empty(),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "aa_mode": "analytical",
        "warmups": WARMUPS.min(runs),
        "runs": runs,
    })
}

#[test]
#[ignore]
fn perf_baseline() -> io::Result<()> {
    let runs = std::env::var(RUNS_ENV).map_or(RUNS, |value| {
        value
            .parse()
            .ok()
            .filter(|&runs| runs > 0)
            .expect("OPENPAINTR_PERF_RUNS must be a positive integer")
    });
    if let Ok(case) = std::env::var(CASE_ENV) {
        let output = std::env::var_os(OUTPUT_ENV).expect("output path");
        return run_case(&case, Path::new(&output), runs);
    }

    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
        |path| Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
    );
    let output = target.join("perf").join("baseline.jsonl");
    std::fs::create_dir_all(output.parent().unwrap())?;
    let header = environment(runs);
    println!("{header}");
    std::fs::write(&output, format!("{header}\n"))?;

    let executable = std::env::current_exe()?;
    for (workload, variant) in CASES {
        for (document, ..) in DOCUMENTS {
            let case = format!("{workload}|{variant}|{document}");
            let before = std::fs::metadata(&output)?.len();
            let started = Instant::now();
            let status = Process::new(&executable)
                .args(["--exact", TEST_NAME, "--ignored", "--nocapture"])
                .env(CASE_ENV, &case)
                .env(OUTPUT_ENV, &output)
                .status()?;
            assert!(status.success(), "case {case} failed: {status}");
            assert!(
                std::fs::metadata(&output)?.len() > before,
                "case {case} recorded nothing"
            );
            println!("case {case}: {:.1} s", started.elapsed().as_secs_f64());
        }
    }
    println!("wrote {}", output.display());
    Ok(())
}
