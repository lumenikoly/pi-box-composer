#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod environment;
mod explorer;
mod instructions;
mod model;
mod process;
mod sessions;
mod widgets;

use anyhow::{Context, Result, bail};
use eframe::egui;
use std::path::PathBuf;

fn run() -> Result<()> {
    let mut data_dir = model::default_data_dir();
    let mut smoke_test = false;
    let mut start_setup = false;
    let mut instructions = false;
    let mut project = None;
    let mut preferences = false;
    let mut open_project = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                data_dir = PathBuf::from(args.next().context("После --data-dir нужен путь")?);
            }
            "--smoke-test" => smoke_test = true,
            "--setup" => start_setup = true,
            "--instructions" => instructions = true,
            "--settings" => preferences = true,
            "--open" => {
                project = Some(args.next().context("После --open нужен путь к папке")?);
                open_project = true;
            }
            "--project" => {
                project = Some(args.next().context("После --project нужен путь к папке")?)
            }
            value if !value.starts_with('-') && project.is_none() => {
                project = Some(value.to_owned())
            }
            _ => bail!("Неизвестный аргумент: {arg}"),
        }
    }
    if !data_dir.is_absolute() {
        bail!("--data-dir должен быть абсолютным путём");
    }
    let config_path = data_dir.join("launcher.json");
    let mut saved = model::Saved::load(&config_path)?;
    if !config_path.exists() {
        saved.settings.root = data_dir.join("environment").display().to_string();
    }
    if let Some(project) = project {
        let path = std::path::absolute(project).context("Не удалось разрешить путь к проекту")?;
        saved.select_project(&path.display().to_string());
    }
    let window_size = [1080.0, 820.0];
    #[cfg(feature = "capture-ui")]
    let window_size = std::env::var("PI_LAUNCHER_CAPTURE_SIZE")
        .ok()
        .and_then(|value| {
            value
                .split_once('x')
                .and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))
        })
        .filter(|[w, h]| w.is_finite() && h.is_finite() && *w >= 860.0 && *h >= 660.0)
        .unwrap_or(window_size);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_min_inner_size([860.0, 660.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Glow,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "pi-box-composer",
        options,
        Box::new(move |cc| {
            let mut app = app::Launcher::new(cc, data_dir, saved, smoke_test);
            if start_setup {
                app.open_setup();
            }
            if instructions {
                app.open_instructions();
            }
            if preferences {
                app.open_preferences();
            }
            if open_project && !smoke_test {
                app.open_project(&cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Не удалось открыть окно: {e}"))
}

fn main() {
    if let Err(error) = run() {
        rfd::MessageDialog::new()
            .set_title("pi-box-composer")
            .set_description(format!("{error:#}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
