use crate::{
    environment::{self, StepResult},
    model::*,
    process::{Cmd, Runner, log_text},
};
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Stroke};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

const BG: Color32 = Color32::from_rgb(246, 247, 250);
const TEXT: Color32 = Color32::from_rgb(32, 36, 49);
const MUTED: Color32 = Color32::from_rgb(111, 117, 134);
const ACCENT: Color32 = Color32::from_rgb(97, 76, 214);
const TINT: Color32 = Color32::from_rgb(240, 237, 255);
const BORDER: Color32 = Color32::from_rgb(226, 229, 237);
const GREEN: Color32 = Color32::from_rgb(37, 132, 96);
const RED: Color32 = Color32::from_rgb(180, 58, 67);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Project,
    Setup,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    Setup,
    Step(usize),
    Launch,
    StartMachine,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Running,
    Done,
    Failed,
}

enum Event {
    Log(String),
    Step(usize, Status),
    Models(Vec<String>),
    Finished(Result<Option<PathBuf>, String>),
}

const LOG_QUEUE_CAPACITY: usize = 256;
const LOG_HISTORY_ENTRIES: usize = 1500;

fn queue_log(tx: &SyncSender<Event>, line: &str, dropped: &AtomicUsize) {
    // Reader threads never block, even when the UI is stalled.
    if tx.try_send(Event::Log(log_text(line))).is_err() {
        dropped.fetch_add(1, Ordering::Relaxed);
    }
}

fn push_log(logs: &mut VecDeque<String>, line: &str) {
    // Count * per-record byte limit also bounds total retained bytes.
    if logs.len() >= LOG_HISTORY_ENTRIES {
        logs.pop_front();
    }
    logs.push_back(log_text(line));
}

pub struct Launcher {
    saved: Saved,
    data_dir: PathBuf,
    page: Page,
    stage: usize,
    steps: [Status; 7],
    token: String,
    models: Vec<String>,
    events: Option<Receiver<Event>>,
    active_job: Option<Job>,
    cancel: Arc<AtomicBool>,
    logs: VecDeque<String>,
    message: String,
    error: bool,
    show_diagnostics: bool,
    guide: crate::instructions::Guide,
    dirty_at: Option<Instant>,
    want_close: bool,
    started: Instant,
    smoke_test: bool,
    #[cfg(feature = "capture-ui")]
    capture_requested: bool,
}

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(14)
        .inner_margin(24.0)
}

fn secondary(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(MUTED));
}

fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    crate::widgets::button(ui, text, enabled)
}

fn field(ui: &mut egui::Ui, value: &mut String, hint: &str, password: bool) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 44.0],
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .password(password)
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(12.0, 10.0)),
    )
}

impl Launcher {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        data_dir: PathBuf,
        saved: Saved,
        smoke_test: bool,
    ) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.animation_time = 0.0;
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.extreme_bg_color = Color32::from_rgb(249, 250, 252);
        style.visuals.faint_bg_color = BG;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = TINT;
        style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
        style.visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.hovered.bg_fill = TINT;
        style.visuals.widgets.hovered.weak_bg_fill = TINT;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.active.bg_fill = TINT;
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(16.0, 10.0);
        style.spacing.interact_size.y = 34.0;
        for (kind, size) in [
            (egui::TextStyle::Body, 15.0),
            (egui::TextStyle::Button, 15.0),
            (egui::TextStyle::Heading, 26.0),
        ] {
            style.text_styles.insert(kind, FontId::proportional(size));
        }
        cc.egui_ctx.set_style(style);
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
            fonts
                .font_data
                .insert("segoe".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoe".into());
            if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisb.ttf") {
                fonts.font_data.insert(
                    "segoe-semibold".into(),
                    egui::FontData::from_owned(bytes).into(),
                );
                fonts.families.insert(
                    egui::FontFamily::Name("heading".into()),
                    vec!["segoe-semibold".into(), "segoe".into()],
                );
                cc.egui_ctx.style_mut(|style| {
                    style.text_styles.insert(
                        egui::TextStyle::Heading,
                        FontId::new(26.0, egui::FontFamily::Name("heading".into())),
                    );
                });
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        Self {
            saved,
            data_dir,
            page: Page::Project,
            stage: 0,
            steps: [Status::Pending; 7],
            token: String::new(),
            models: vec![],
            events: None,
            active_job: None,
            cancel: Arc::new(AtomicBool::new(false)),
            logs: VecDeque::new(),
            message: String::new(),
            error: false,
            show_diagnostics: false,
            guide: crate::instructions::Guide::default(),
            dirty_at: None,
            want_close: false,
            started: Instant::now(),
            smoke_test,
            #[cfg(feature = "capture-ui")]
            capture_requested: false,
        }
    }

    fn busy(&self) -> bool {
        self.events.is_some()
    }

    fn persist(&mut self) {
        if let Err(e) = self.saved.save(&self.data_dir.join("launcher.json")) {
            self.message = format!("Не удалось сохранить настройки: {e:#}");
            self.error = true;
        }
        self.dirty_at = None;
    }

    fn log(&mut self, line: String) {
        push_log(&mut self.logs, &line);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let events: Vec<Event> = self
            .events
            .as_ref()
            .map(|rx| rx.try_iter().take(500).collect())
            .unwrap_or_default();
        let mut next_job = None;
        for event in events {
            match event {
                Event::Log(line) => self.log(line),
                Event::Step(index, status) => self.steps[index] = status,
                Event::Models(models) => {
                    if !models.contains(&self.saved.settings.model) {
                        self.saved.settings.model = models[0].clone();
                    }
                    self.models = models;
                    self.dirty_at = Some(Instant::now());
                }
                Event::Finished(result) => {
                    self.events = None;
                    let job = self.active_job.take();
                    match result {
                        Ok(project) => {
                            self.message.clear();
                            self.error = false;
                            match job {
                                Some(Job::Step(0)) => {
                                    self.page = Page::Setup;
                                    self.stage = 1;
                                }
                                Some(Job::Setup | Job::Step(5)) => {
                                    self.page = Page::Setup;
                                    self.stage = 2;
                                }
                                Some(Job::Step(6)) => {
                                    self.page = Page::Project;
                                    self.message = "Настройки сохранены.".into();
                                }
                                Some(Job::StartMachine) => next_job = Some(Job::Step(0)),
                                _ => {}
                            }
                            if let Some(project) = project {
                                self.saved.remember(&project);
                                self.message = "Pi открыт в отдельном терминале.".into();
                                self.persist();
                            }
                        }
                        Err(error) => {
                            self.log(format!("ОШИБКА: {error}"));
                            self.message = error;
                            self.error = true;
                        }
                    }
                }
            }
        }
        if let Some(job) = next_job {
            self.start(job, ctx);
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if self
            .dirty_at
            .is_some_and(|time| time.elapsed() > Duration::from_millis(600))
        {
            self.persist();
        }
        if self.want_close && !self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn start(&mut self, job: Job, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        if !matches!(job, Job::StartMachine | Job::Step(0))
            && let Err(e) = self.saved.settings.validate()
        {
            self.message = e.to_string();
            self.error = true;
            return;
        }
        if job == Job::Step(6)
            && (self.steps[5] != Status::Done || !self.models.contains(&self.saved.settings.model))
        {
            self.message = "Сначала проверьте подключение и выберите модель.".into();
            self.error = true;
            return;
        }
        self.persist();
        let settings = self.saved.settings.clone();
        let project = self.saved.project.clone();
        let access = self.saved.access;
        let session = self.saved.session;
        let data_dir = self.data_dir.clone();
        let token = Zeroizing::new(if matches!(job, Job::Setup | Job::Step(4)) {
            std::mem::take(&mut self.token)
        } else {
            String::new()
        });
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();
        let (tx, rx) = mpsc::sync_channel(LOG_QUEUE_CAPACITY);
        self.events = Some(rx);
        self.active_job = Some(job);
        self.message.clear();
        self.error = false;
        if job == Job::Setup {
            self.steps = [Status::Pending; 7];
            self.models.clear();
        }
        if let Job::Step(index) = job {
            self.steps[index..].fill(Status::Pending);
            if index <= 5 {
                self.models.clear();
            }
        }
        let context = ctx.clone();
        thread::spawn(move || {
            let dropped_logs = Arc::new(AtomicUsize::new(0));
            let dropped = dropped_logs.clone();
            let log_tx = tx.clone();
            let log_ctx = context.clone();
            let runner = Runner {
                engine: settings.engine,
                log: Arc::new(move |line| {
                    queue_log(&log_tx, line, &dropped);
                    log_ctx.request_repaint();
                }),
                cancel,
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || -> anyhow::Result<Option<PathBuf>> {
                    match job {
                        Job::Setup => {
                            for index in 0..6 {
                                perform_step(index, &settings, &token, &runner, &tx)?;
                            }
                            Ok(None)
                        }
                        Job::Step(index) => {
                            perform_step(index, &settings, &token, &runner, &tx)?;
                            Ok(None)
                        }
                        Job::Launch => environment::launch(
                            &settings, &project, &data_dir, access, session, &runner,
                        )
                        .map(Some),
                        Job::StartMachine => {
                            runner.checked(&environment::start_engine_command(settings.engine))?;
                            Ok(None)
                        }
                    }
                },
            ));
            let result = match result {
                Ok(result) => result.map_err(|error| format!("{error:#}")),
                Err(_) => Err("Не удалось завершить операцию. Подробности в диагностике.".into()),
            };
            let dropped = dropped_logs.load(Ordering::Relaxed);
            if dropped != 0 {
                let _ = tx.send(Event::Log(format!(
                    "Журнал перегружен: пропущено записей: {dropped}."
                )));
            }
            let _ = tx.send(Event::Finished(result));
            context.request_repaint();
        });
    }

    pub fn open_instructions(&mut self) {
        self.guide.open = true;
    }

    pub fn open_setup(&mut self) {
        self.page = Page::Setup;
        self.stage = 0;
        self.message.clear();
        self.error = false;
    }

    fn page_title(&mut self, ui: &mut egui::Ui, title: &str) {
        ui.horizontal(|ui| {
            ui.heading(title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let more = ui.add(egui::Button::new("···").frame(false));
                egui::Popup::menu(&more).show(|ui| {
                    if crate::widgets::secondary(ui, "Диагностика", true).clicked() {
                        self.show_diagnostics = true;
                        ui.close();
                    }
                });
                if self.page == Page::Project
                    && ui
                        .add_enabled(!self.busy(), egui::Button::new("Настройки").frame(false))
                        .clicked()
                {
                    self.open_setup();
                }
            });
        });
    }

    fn notice(&mut self, ui: &mut egui::Ui) {
        if self.message.is_empty() {
            return;
        }
        let color = if self.error { RED } else { GREEN };
        let short: String = self.message.chars().take(220).collect();
        egui::Frame::new()
            .fill(if self.error {
                Color32::from_rgb(255, 241, 242)
            } else {
                Color32::from_rgb(235, 248, 241)
            })
            .corner_radius(10)
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.colored_label(color, &short);
                if self.error && crate::widgets::secondary(ui, "Подробности", true).clicked()
                {
                    self.show_diagnostics = true;
                }
            });
        ui.add_space(8.0);
    }

    fn project(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.page_title(ui, "Проект");
        ui.add_space(18.0);
        self.notice(ui);
        let ready = environment::is_configured(&self.saved.settings);
        let busy = self.busy();
        let before = (
            self.saved.project.clone(),
            self.saved.access,
            self.saved.session,
        );
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_enabled_ui(!busy, |ui| {
                ui.label("Папка проекта");
                if crate::widgets::folder(ui, &self.saved.project) {
                    self.pick_project();
                }
                ui.add_space(16.0);
                ui.label("Доступ к файлам");
                let mut selected = usize::from(self.saved.access == Access::ReadWrite);
                crate::widgets::segmented(
                    ui,
                    &mut selected,
                    &[Access::ReadOnly.label(), Access::ReadWrite.label()],
                );
                self.saved.access = if selected == 0 {
                    Access::ReadOnly
                } else {
                    Access::ReadWrite
                };
                if self.saved.access == Access::ReadWrite {
                    secondary(ui, "Изменения сохраняются в выбранной папке.");
                }
                ui.add_space(16.0);
                ui.label("Сессия");
                let response = crate::widgets::select(ui, self.saved.session.label());
                egui::Popup::menu(&response)
                    .width(response.rect.width())
                    .show(|ui| {
                        for session in [Session::New, Session::Continue, Session::Resume] {
                            if ui
                                .selectable_value(&mut self.saved.session, session, session.label())
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    });
            });
            ui.add_space(24.0);
            let title = if busy {
                "Подготовка запуска…"
            } else if !ready {
                "Настроить окружение"
            } else {
                "Запустить Pi"
            };
            if primary(
                ui,
                title,
                !busy && (!ready || !self.saved.project.is_empty()),
            ) {
                if ready {
                    self.start(Job::Launch, ctx);
                } else {
                    self.open_setup();
                }
            }
            if busy {
                self.progress(ui);
            }
        });
        if before
            != (
                self.saved.project.clone(),
                self.saved.access,
                self.saved.session,
            )
        {
            self.dirty_at = Some(Instant::now());
        }
        if !self.saved.recent.is_empty() {
            ui.add_space(24.0);
            ui.label(RichText::new("Недавние проекты").color(MUTED));
            let recent = self.saved.recent.clone();
            ui.add_enabled_ui(!busy, |ui| {
                for project in recent.iter().take(5) {
                    if crate::widgets::select(ui, &project_name(&project.path))
                        .on_hover_text(&project.path)
                        .clicked()
                    {
                        self.saved.project = project.path.clone();
                        self.saved.access = project.access;
                        self.dirty_at = Some(Instant::now());
                    }
                }
            });
        }
    }

    fn pick_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Папка проекта")
            .pick_folder()
        {
            self.saved.project = path.display().to_string();
            self.dirty_at = Some(Instant::now());
        }
    }

    fn setup(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("←  Назад").frame(false))
                .clicked()
            {
                if self.stage == 0 {
                    self.page = Page::Project;
                } else {
                    self.stage -= 1;
                }
                self.message.clear();
                self.error = false;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for i in (0..3).rev() {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(32.0, 4.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        2,
                        if i <= self.stage { ACCENT } else { BORDER },
                    );
                }
            });
        });
        ui.add_space(20.0);
        let title = match self.stage {
            0 => "Окружение",
            1 => "Сервер модели",
            _ => "Модель",
        };
        self.page_title(ui, title);

        ui.add_space(18.0);
        self.notice(ui);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            match self.stage {
                0 => self.engine_form(ui, ctx),
                1 if busy => {
                    ui.add_space(12.0);
                    ui.label("Подготовка окружения");

                    ui.add_space(14.0);
                    let completed = self.steps[..6]
                        .iter()
                        .filter(|s| **s == Status::Done)
                        .count();
                    ui.add(
                        egui::ProgressBar::new(completed as f32 / 6.0)
                            .fill(ACCENT)
                            .corner_radius(4),
                    );
                    self.progress(ui);
                }
                1 => self.server_form(ui, ctx),
                _ => {
                    let response = crate::widgets::select(ui, &self.saved.settings.model);
                    egui::Popup::menu(&response)
                        .width(response.rect.width())
                        .show(|ui| {
                            for model in &self.models {
                                if ui
                                    .selectable_value(
                                        &mut self.saved.settings.model,
                                        model.clone(),
                                        model,
                                    )
                                    .clicked()
                                {
                                    self.dirty_at = Some(Instant::now());
                                    ui.close();
                                }
                            }
                        });
                    ui.add_space(10.0);
                    ui.collapsing("Лимиты модели", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Контекст");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.saved.settings.context_window)
                                        .range(1..=2_000_000)
                                        .speed(1024),
                                )
                                .changed()
                            {
                                self.dirty_at = Some(Instant::now());
                            }
                            ui.label("Ответ");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.saved.settings.max_tokens)
                                        .range(1..=2_000_000)
                                        .speed(256),
                                )
                                .changed()
                            {
                                self.dirty_at = Some(Instant::now());
                            }
                        });
                    });
                    ui.add_space(16.0);
                    if primary(
                        ui,
                        if busy {
                            "Сохраняем…"
                        } else {
                            "Сохранить"
                        },
                        !busy && !self.models.is_empty(),
                    ) {
                        self.start(Job::Step(6), ctx);
                    }
                }
            }
        });
        ui.add_space(16.0);
        if crate::widgets::secondary_full(ui, "Инструкция и команды", true).clicked()
        {
            self.guide.open = true;
            self.guide.tab = usize::from(self.stage != 0);
        }
    }

    fn engine_form(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy();
        let before = self.saved.settings.engine;
        ui.add_enabled_ui(!busy, |ui| {
            let mut selected = usize::from(before == Engine::Docker);
            crate::widgets::segmented(ui, &mut selected, &["Podman", "Docker Desktop"]);
            self.saved.settings.engine = if selected == 0 {
                Engine::Podman
            } else {
                Engine::Docker
            };
        });
        if self.saved.settings.engine != before {
            self.saved.settings = environment::load_engine_profile(&self.saved.settings);
            self.steps.fill(Status::Pending);
            self.models.clear();
            self.token.zeroize();
            self.message.clear();
            self.error = false;
            self.dirty_at = Some(Instant::now());
        }
        let docker = self.saved.settings.engine == Engine::Docker;
        ui.add_space(12.0);
        secondary(
            ui,
            if docker {
                "Docker Desktop · Linux containers"
            } else {
                "Podman · rootless Machine"
            },
        );
        ui.add_space(20.0);
        if primary(
            ui,
            if busy {
                "Проверка…"
            } else {
                "Проверить и продолжить"
            },
            !busy,
        ) {
            self.start(Job::Step(0), ctx);
        }
        if busy {
            self.progress(ui);
        }
        ui.add_space(10.0);
        if crate::widgets::secondary_full(
            ui,
            if docker {
                "Запустить Docker Desktop"
            } else {
                "Запустить Podman Machine"
            },
            !busy,
        )
        .clicked()
        {
            self.start(Job::StartMachine, ctx);
        }
    }

    fn server_form(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let before = self.saved.settings.clone();
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 104.0).max(180.0);
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label("Адрес сервера");
                field(ui, &mut self.saved.settings.host, "192.168.1.195", false);
            });
            ui.vertical(|ui| {
                ui.set_width(96.0);
                ui.label("Порт");
                ui.add_sized(
                    [96.0, 44.0],
                    egui::DragValue::new(&mut self.saved.settings.port)
                        .range(1..=65535)
                        .speed(1),
                );
            });
        });
        ui.add_space(10.0);
        ui.label(RichText::new("API-токен").strong());
        field(ui, &mut self.token, "Введите токен llama.cpp", true);
        secondary(
            ui,
            if self.saved.settings.engine == Engine::Docker {
                "Пусто — использовать токен существующего прокси."
            } else {
                "Пусто — использовать секрет Podman."
            },
        );
        ui.add_space(10.0);
        ui.collapsing("Дополнительно", |ui| {
            ui.label("Где хранить окружение");
            field(
                ui,
                &mut self.saved.settings.root,
                "Каталог окружения",
                false,
            );
            if crate::widgets::secondary(ui, "Выбрать каталог…", true).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_title("Каталог окружения")
                    .pick_folder()
            {
                self.saved.settings.root = path.display().to_string();
            }
        });
        if self.saved.settings != before {
            self.steps[1..].fill(Status::Pending);
            self.models.clear();
            self.dirty_at = Some(Instant::now());
        }
        ui.add_space(18.0);
        if primary(ui, "Подключить", true) {
            self.start(Job::Setup, ctx);
        }
    }

    fn progress(&mut self, ui: &mut egui::Ui) {
        let text = match self.active_job {
            Some(Job::Launch) => "Проверяем папку и подключение…",
            Some(Job::StartMachine) => "Запускаем движок…",
            _ => match self.steps.iter().position(|s| *s == Status::Running) {
                Some(0) => "Проверяем движок…",
                Some(1) => "Готовим настройки…",
                Some(2) => "Скачиваем и собираем образ Pi…",
                Some(3) => "Настраиваем соединение…",
                Some(4) => "Запускаем прокси…",
                Some(5) => "Проверяем модель…",
                _ => "Сохраняем настройки…",
            },
        };
        ui.horizontal(|ui| {
            ui.spinner();
            secondary(ui, text);
        });
        ui.horizontal(|ui| {
            if crate::widgets::secondary(ui, "Подробности", true).clicked() {
                self.show_diagnostics = true;
            }
            if crate::widgets::secondary(ui, "Отменить", true).clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
    }

    fn diagnostics(&mut self, ctx: &egui::Context) {
        let mut open = self.show_diagnostics;
        egui::Window::new("Диагностика").open(&mut open).default_size([740.0, 480.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if crate::widgets::secondary(ui, "Копировать журнал", true).clicked() { ctx.copy_text(self.logs.iter().cloned().collect::<Vec<_>>().join("\n")); }
                if crate::widgets::secondary(ui, "Очистить журнал", true).clicked() { self.logs.clear(); }
            });
            if self.logs.is_empty() { secondary(ui, "Здесь появятся команды и их вывод."); }
            egui::ScrollArea::both().max_height(280.0).stick_to_bottom(true).show(ui, |ui| {
                for line in &self.logs { ui.label(RichText::new(line).monospace().size(12.0)); }
            });
            ui.separator();
            if crate::widgets::secondary(ui, "Открыть инструкцию и команды", true).clicked() { self.guide.open = true; self.guide.tab = 1; }
            ui.collapsing("Доступ к проекту", |ui| {
                ui.label("В контейнер передаются выбранная папка и отдельное состояние Pi. Файлы .env внутри проекта доступны контейнеру: используйте рабочую копию без секретов.");
                ui.label("Режим чтения запрещает запись в проект, но оставляет рабочие /tmp и /state. Проверка сети относится только к указанному серверу llama.cpp.");
                if !self.saved.project.is_empty() {
                    let args = environment::run_args(&self.saved.settings, Path::new(&self.saved.project), self.saved.access, self.saved.session);
                    if crate::widgets::secondary(ui, "Копировать команду запуска", true).clicked() { ctx.copy_text(Cmd::new(args).preview(self.saved.settings.engine)); }
                }
            });
        });
        self.show_diagnostics = open;
    }
}

fn project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.into())
}

fn perform_step(
    index: usize,
    settings: &Settings,
    token: &str,
    runner: &Runner,
    tx: &SyncSender<Event>,
) -> anyhow::Result<()> {
    let _ = tx.send(Event::Step(index, Status::Running));
    match environment::execute_step(index, settings, token, runner) {
        Ok(result) => {
            if let StepResult::Models(models) = result {
                let _ = tx.send(Event::Models(models));
            }
            let _ = tx.send(Event::Step(index, Status::Done));
            Ok(())
        }
        Err(error) => {
            let _ = tx.send(Event::Step(index, Status::Failed));
            Err(error)
        }
    }
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        #[cfg(feature = "capture-ui")]
        self.capture_preview(ctx);
        self.poll(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.busy() {
            self.cancel.store(true, Ordering::Relaxed);
            self.want_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.smoke_test {
            ctx.request_repaint_after(Duration::from_millis(100));
            if self.started.elapsed() > Duration::from_secs(3) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("main").show(ui, |ui| {
                let width = ui.available_width().min(560.0);
                let margin = ((ui.available_width() - width) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(margin);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.add_space(32.0);
                        match self.page {
                            Page::Project => self.project(ui, ctx),
                            Page::Setup => self.setup(ui, ctx),
                        }
                        ui.add_space(24.0);
                    });
                });
            });
        });
        if self.show_diagnostics {
            self.diagnostics(ctx);
        }
        let available = std::array::from_fn(|index| {
            !self.busy() && (index == 0 || self.steps[..index].iter().all(|s| *s == Status::Done))
        });
        let completed = self.steps.map(|status| status == Status::Done);
        if let Some(index) = self
            .guide
            .show(ctx, &self.saved.settings, &available, &completed)
        {
            self.start(Job::Step(index), ctx);
        }
        if self.dirty_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(650));
        }
    }

    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.token.zeroize();
        self.cancel.store(true, Ordering::Relaxed);
        if self.dirty_at.is_some() {
            self.persist();
        }
    }
}

#[cfg(feature = "capture-ui")]
impl Launcher {
    fn capture_preview(&mut self, ctx: &egui::Context) {
        let Some(path) = std::env::var_os("PI_LAUNCHER_CAPTURE_TO") else {
            return;
        };
        if !self.capture_requested && self.started.elapsed() > Duration::from_millis(700) {
            self.capture_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let captured = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = captured {
            let bytes: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_array())
                .collect();
            if let Err(error) = image::save_buffer(
                std::path::Path::new(&path),
                &bytes,
                image.width() as u32,
                image.height() as u32,
                image::ColorType::Rgba8,
            ) {
                eprintln!("UI capture failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flooded_logs_are_bounded_and_completion_is_reliable() {
        let (tx, rx) = mpsc::sync_channel(LOG_QUEUE_CAPACITY);
        let dropped = AtomicUsize::new(0);
        let line = "x".repeat(crate::process::MAX_LOG_BYTES * 2);
        for _ in 0..LOG_QUEUE_CAPACITY + 20 {
            queue_log(&tx, &line, &dropped);
        }
        assert_eq!(dropped.load(Ordering::Relaxed), 20);
        let done = thread::spawn(move || {
            tx.send(Event::Models(vec!["model".into()])).unwrap();
            tx.send(Event::Finished(Ok(None))).unwrap();
        });
        for _ in 0..LOG_QUEUE_CAPACITY {
            let Event::Log(line) = rx.recv().unwrap() else {
                panic!("expected queued log")
            };
            assert!(line.len() <= crate::process::MAX_LOG_BYTES);
        }
        assert!(
            matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Event::Models(models) if models == ["model"])
        );
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Finished(Ok(None))
        ));
        done.join().unwrap();
    }

    #[test]
    fn history_limits_entries_and_bytes_including_errors() {
        let mut history = VecDeque::new();
        let error = "ошибка".repeat(crate::process::MAX_LOG_BYTES);
        for _ in 0..LOG_HISTORY_ENTRIES + 1 {
            push_log(&mut history, &error);
        }
        assert_eq!(history.len(), LOG_HISTORY_ENTRIES);
        assert!(
            history
                .iter()
                .all(|line| line.len() <= crate::process::MAX_LOG_BYTES)
        );
        assert!(
            history.iter().map(String::len).sum::<usize>()
                <= LOG_HISTORY_ENTRIES * crate::process::MAX_LOG_BYTES
        );
    }
}
