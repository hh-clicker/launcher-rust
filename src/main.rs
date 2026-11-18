#![windows_subsystem = "windows"]
mod ui;
use anyhow::{Context, Result};
use hh_clicker_launcher::{
    config::{Config, Layout},
    platform::{self, InstanceGuard},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn launch() -> Result<()> {
    let started = Instant::now();
    let mut offline = false;
    let mut data = None;
    let mut quit_after = None;
    let mut settings = false;
    let mut scale = None;
    let mut close_during_start = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--offline" => offline = true,
            "--data-dir" => data = Some(PathBuf::from(args.next().context("Нет --data-dir")?)),
            "--quit-after" => {
                quit_after = Some(Duration::from_secs(
                    args.next().context("Нет --quit-after")?.parse()?,
                ))
            }
            "--settings" => settings = true,
            "--qa-close-during-start" => close_during_start = true,
            "--scale" => scale = Some(args.next().context("Нет --scale")?.parse::<f32>()?),
            _ => anyhow::bail!("Неизвестный параметр: {arg}"),
        }
    }
    if (quit_after.is_some() || scale.is_some()) && !offline {
        anyhow::bail!("Параметры автоматической GUI-проверки требуют --offline");
    }
    if let Ok(preview) = std::env::var("HH_RUST_UI_PREVIEW") {
        if !offline || data.is_none() || !ui::PREVIEW_CASES.contains(&preview.as_str()) {
            anyhow::bail!(
                "UI preview требует --offline, явный --data-dir и известное имя состояния"
            );
        }
    }
    if close_during_start && (data.is_none() || offline) {
        anyhow::bail!("Live QA requires explicit --data-dir and real environment");
    }
    if scale.is_some_and(|v| !(0.75..=3.0).contains(&v)) {
        anyhow::bail!("Масштаб вне диапазона 0.75–3");
    }
    let root = std::env::current_exe()?
        .parent()
        .context("Каталог EXE недоступен")?
        .to_path_buf();
    if let Some(path) = &data {
        if !path.is_absolute() {
            anyhow::bail!("--data-dir должен быть абсолютным путём");
        }
    }
    let mut layout = Layout::new(root, data);
    std::fs::create_dir_all(&layout.data)?;
    // Lock the canonical data path before creating identity, avoiding a first-launch race.
    use sha2::{Digest, Sha256};
    let key = format!(
        "{:x}",
        Sha256::digest(
            layout
                .data
                .canonicalize()?
                .to_string_lossy()
                .to_lowercase()
                .as_bytes()
        )
    );
    let existing_title = std::fs::read(layout.data.join("config.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok())
        .map(|c| format!("HH Clicker — Rust portable · {}", c.instance.simple()))
        .unwrap_or_else(|| "HH Clicker — Rust portable".into());
    let _path_guard = match InstanceGuard::acquire(&key, &existing_title)? {
        Some(g) => g,
        None => return Ok(()),
    };
    layout.payload = hh_clicker_launcher::bundle::ensure(&layout.root)?;
    let config = layout.load()?;
    let fonts = hh_clicker_launcher::fonts::definitions(
        &layout
            .payload
            .parent()
            .context("Resource directory")?
            .join("fonts.dat"),
    )?;
    let title = format!("HH Clicker — Rust portable · {}", config.instance.simple());
    let _instance_guard = match InstanceGuard::acquire(&config.instance.to_string(), &title)? {
        Some(g) => g,
        None => return Ok(()),
    };
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/hh-clicker.png"))?;
    let size = [960.0 * scale.unwrap_or(1.0), 700.0 * scale.unwrap_or(1.0)];
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size)
            .with_resizable(false)
            .with_decorations(false)
            .with_icon(icon),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "HH Clicker Rust",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(ui::Launcher::new(
                cc,
                layout,
                config,
                offline,
                settings,
                quit_after,
                scale,
                started,
                close_during_start,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Не удалось открыть окно: {e}"))
}
fn main() {
    if let Err(e) = launch() {
        platform::startup_error(&format!("{e:#}"));
    }
}
