//! Prototypie, native: a Rune app over proto-core's store.
// Skeleton phase: interfaces exist before their callers (cards, inspector, features).
#![allow(dead_code)]

mod app;
mod canvas;
mod cli;
mod cards;
mod composer;
mod features;
#[cfg(test)]
mod font_check_tests;
mod fullscreen;
mod inspector;
mod perf;
mod runner;
mod shell;

use app::insets::HEADER_H;
use app::state::{AppState, ThemePref};
use rune::{App, AppSpec, KeyEvent, Titlebar, Tx};

type Scene = (&'static str, fn(&mut AppState));

/// The shell's own scenes; `scenes()` appends each owner module's `SCENES`.
const SHELL_SCENES: &[Scene] = &[
    ("light", |s| s.theme = ThemePref::Light),
    ("nested", |s| {
        s.enter("qQiPnYYC", vec![("prototypie".into(), "Prototypie".into()), ("qQiPnYYC".into(), "Header navbar".into())]);
    }),
    ("new-menu", |s| {
        s.enter("qQiPnYYC", vec![("qQiPnYYC".into(), "Header navbar".into())]);
        s.shell.new_menu_open = true;
    }),
    ("crumbs-deep", |s| {
        let path: Vec<(String, String)> = ["Prototypie", "Header navbar", "Toolbar", "Icons", "Bottom dock"]
            .iter().enumerate().map(|(i, n)| (format!("zzc{i}"), n.to_string())).collect();
        s.enter("zzc4", path);
        s.shell.crumbs.menu_open = true;
    }),
    ("crumbs-rename", |s| {
        s.enter("qQiPnYYC", vec![("prototypie".into(), "Prototypie".into()), ("qQiPnYYC".into(), "Header navbar".into())]);
        shell::crumbs::begin(s);
    }),
    ("already-running", |s| s.data.source = app::data::Source::Locked { pid: 4242 }),
    ("api-down", |s| s.server.state = app::server::ServerState::Failed("127.0.0.1:3124: Address already in use (os error 48)".into())),
    ("immersive-notch", |s| {
        s.enter("qQiPnYYC", vec![("prototypie".into(), "Prototypie".into()), ("qQiPnYYC".into(), "Header navbar".into())]);
        shell::immersive::scene_notch(s);
    }),
    ("project-dialog", |s| {
        let projects = (0..60).map(|i| features::nested::project::Project {
            id: format!("p{i}"),
            name: format!("Project {i:02}"),
            keywords: vec!["Rust".into()],
            source: Some(["private-projects.ttl", "Solid pod", "Folder"][i % 3].into()),
            ..Default::default()
        }).collect();
        features::nested::project::scene(s, projects);
    }),
    ("zoomed-out", |s| {
        s.canvas.camera.set([37.0, -11.0], 0.4);
    }),
];

/// Every `--snap` scene: the shell's, then each owner module's `SCENES` (add a module's list here
/// once, its scenes then come along by themselves).
fn scenes() -> &'static [Scene] {
    let all: Vec<Scene> = [SHELL_SCENES, inspector::scenes::SCENES, cards::fixture::SCENES, features::scenes::SCENES, features::agent_tasks::SCENES, canvas::minimap::SCENES].concat();
    Box::leak(all.into_boxed_slice())
}

/// Keys nobody focused took: fullscreen first (it's modal), then the features' overlays.
fn on_key(s: &mut AppState, ev: &KeyEvent, tx: &mut Tx) -> bool {
    fullscreen::on_key(s, ev, tx) || features::on_key(s, ev, tx) || canvas::nodes::on_key(s, ev, tx) || shell::crumbs::on_key(s, ev, tx)
        // Last: Esc leaves immersive mode only when nothing above took it.
        || shell::immersive::on_key(s, ev, tx)
}

fn main() {
    // `prototypie notes|queue|cards|status|skill|help`: the CLI, before any AppKit, GPU, store or runtime start
    // (spec-public/05 AN-CLI-201). It never opens a window or brings the app forward.
    if let Some(code) = cli::run() {
        std::process::exit(code);
    }
    // Another Prototypie holds the data dir: bring it forward instead of opening a second writer.
    if app::instance::hand_off() {
        return;
    }
    perf::start();
    // Rune's "Launching fast": device and uber pipeline on a thread while AppKit starts.
    rune::prewarm_gpu();
    let spec = AppSpec {
        scenes: scenes(),
        // Google Sans Flex (OFL, assets/fonts/OFL.txt), the reference app's UI font, for every view.
        fonts: &[include_bytes!("../assets/fonts/GoogleSansFlex.ttf")],
        families: ("Google Sans Flex", "ui-monospace"),
        on_key: Some(on_key),
        setup: Some(|s, wake| s.setup(wake)),
        poll: Some(|s, clock, tx| s.poll(clock, tx)), min_size: Some((720.0, 480.0)), ..AppSpec::new("Prototypie", (1280.0, 800.0), AppState::new, shell::view) };
    let app = App::new(spec);
    let handle = app.handle();
    app.titlebar(Titlebar::new(HEADER_H))
        .on_window(|h| {
            perf::mark("window");
            shell::set_window(h.clone())
        })
        // After the first frame: work that would compete with it (thumbnail webviews) may start.
        .on_ready(move |_| {
            perf::mark("first-frame");
            handle.update(|s: &mut AppState, _| s.first_frame_shown = true);
        })
        .on_close(|s, _| s.shutdown())
        .on_frame_drawn(|rt| crate::features::annotate::on_frame_drawn(rt))
        .run();
}
