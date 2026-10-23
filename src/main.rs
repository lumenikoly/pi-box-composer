#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod environment;
mod instructions;
mod model;
mod process;
mod widgets;

use anyhow::{Context, Result, bail};
use eframe::egui;
use std::path::PathBuf;

fn run() -> Result<()> {
    let mut data_dir = model::default_data_dir();
    let mut smoke_test = false;
    let mut start_setup = false;
    let mut instructions = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                data_dir = PathBuf::from(args.next().context("После --data-dir нужен путь")?);
            }
            "--smoke-test" => smoke_test = true,
            "--setup" => start_setup = true,
            "--instructions" => instructions = true,
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 680.0])
            .with_min_inner_size([720.0, 600.0]),
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
