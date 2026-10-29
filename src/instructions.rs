use crate::model::Engine;

pub struct Instruction {
    pub title: String,
    pub explanation: String,
    pub condition: Option<String>,
    pub snippet: Option<Snippet>,
    pub link: Option<(&'static str, &'static str)>,
}

pub struct Snippet {
    pub label: String,
    pub text: String,
}

impl Instruction {
    pub fn note(title: &str, explanation: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            explanation: explanation.into(),
            condition: None,
            snippet: None,
            link: None,
        }
    }
    pub fn command(title: &str, explanation: impl Into<String>, text: impl Into<String>) -> Self {
        let mut item = Self::note(title, explanation);
        item.snippet = Some(Snippet {
            label: "PowerShell".into(),
            text: text.into(),
        });
        item
    }
    pub fn when(mut self, condition: &str) -> Self {
        self.condition = Some(condition.into());
        self
    }
    pub fn file(
        title: &str,
        explanation: impl Into<String>,
        path: String,
        content: String,
    ) -> Self {
        let mut item = Self::note(title, explanation);
        item.snippet = Some(Snippet {
            label: path,
            text: content,
        });
        item
    }
}

pub fn preparation_sections(engine: Engine) -> &'static [&'static str] {
    match engine {
        Engine::Podman => &[
            "Установка",
            "Создание Machine",
            "Rootless-режим",
            "Запуск и проверка",
        ],
        Engine::Docker => &[
            "Установка",
            "Linux containers",
            "Запуск и проверка",
            "Выбор подключения",
        ],
    }
}

pub fn preparation(engine: Engine, section: usize) -> Vec<Instruction> {
    match (engine, section) {
        (Engine::Podman, 0) => {
            let mut item = Instruction::note("Установите Podman Desktop", "Откройте страницу установки для Windows. Установите приложение и Podman CLI. После установки перезапустите pi-box-composer, чтобы он увидел обновлённый PATH.");
            item.link = Some(("Открыть страницу установки", "https://podman-desktop.io/docs/installation/windows-install"));
            vec![item]
        }
        (Engine::Podman, 1) => vec![
            Instruction::command("Посмотрите список Machine", "Выполните в PowerShell. Если в списке уже есть Machine, повторно создавать её не нужно.", "podman machine list"),
            Instruction::command("Создайте Machine", "Подготавливает Linux VM для контейнеров. Дождитесь завершения загрузки и создания VM.", "podman machine init").when("Только если список Machine пуст"),
        ],
        (Engine::Podman, 2) => vec![
            Instruction::note("Выберите rootless-режим", "pi-box-composer требует rootless Podman. Если проверка сообщает rootful, выполните команды ниже по порядку. Они изменяют режим существующей Machine; работающие в ней контейнеры будут остановлены."),
            Instruction::command("1. Остановите Machine", "Освобождает VM для изменения режима. Если Machine уже остановлена, переходите к следующей команде.", "podman machine stop").when("Если Machine запущена"),
            Instruction::command("2. Переключите режим", "Отключает rootful-режим для выбранной по умолчанию Machine.", "podman machine set --rootful=false"),
            Instruction::command("3. Запустите Machine", "Запускает VM после изменения режима. Затем откройте «Настройки → Модель» и нажмите «Подключить».", "podman machine start"),
        ],
        (Engine::Podman, 3) => vec![
            Instruction::command("Запустите Machine", "Выполняйте после создания Machine или если она была остановлена.", "podman machine start").when("Если Machine остановлена"),
            Instruction::command("Проверьте rootless", "Ожидаемый ответ — true. Если получен false, откройте раздел «Rootless-режим».", "podman info --format '{{.Host.Security.Rootless}}'"),
        ],
        (Engine::Docker, 0) => {
            let mut item = Instruction::note("Установите Docker Desktop", "Откройте страницу установки для Windows. Выполните шаги установщика для WSL 2 или Hyper-V. Запустите Docker Desktop и дождитесь готовности движка.");
            item.link = Some(("Открыть страницу установки", "https://docs.docker.com/desktop/setup/install/windows-install/"));
            vec![item]
        }
        (Engine::Docker, 1) => vec![
            Instruction::note("Включите Linux containers", "В меню Docker Desktop выберите «Switch to Linux containers», если такой пункт доступен. Если меню предлагает «Switch to Windows containers», Linux-режим уже включён."),
            Instruction::command("Проверьте тип контейнеров", "Ожидаемый ответ — linux. pi-box-composer не запускает окружение в режиме Windows containers.", "docker info --format '{{.OSType}}'"),
        ],
        (Engine::Docker, 2) => vec![
            Instruction::command("Запустите Docker Desktop", "Ожидает запуска приложения до 60 секунд. Если команда desktop недоступна, откройте Docker Desktop через меню «Пуск».", "docker desktop start --timeout 60").when("Если Docker Desktop ещё не запущен"),
            Instruction::command("Проверьте движок", "Команда должна показать сведения о сервере без ошибки подключения. Затем нажмите «Подключить» в pi-box-composer.", "docker info"),
        ],
        (Engine::Docker, 3) => vec![
            Instruction::command("Посмотрите подключения", "Звёздочкой отмечен текущий context. Для pi-box-composer нужно подключение к локальному Docker Desktop с Linux containers.", "docker context ls"),
            Instruction::command("Выберите Docker Desktop", "Переключает context для всех последующих команд Docker CLI. После переключения повторите проверку в мастере.", "docker context use desktop-linux").when("Только если desktop-linux есть в списке и текущий context другой"),
        ],
        _ => vec![],
    }
}

#[derive(Default)]
pub struct Guide {
    pub open: bool,
    pub tab: usize,
    selected: usize,
    engine: Engine,
    copied: Option<(eframe::egui::Id, std::time::Instant)>,
}

impl Guide {
    pub fn show(
        &mut self,
        ctx: &eframe::egui::Context,
        settings: &crate::model::Settings,
        available: &[bool; 7],
        completed: &[bool; 7],
    ) -> Option<usize> {
        use eframe::egui::{self, Align, Color32, Layout, RichText};
        if !self.open {
            return None;
        }
        if self.engine != settings.engine {
            self.engine = settings.engine;
            self.selected = 0;
        }
        let mut open = self.open;
        let mut action = None;
        let bounds = ctx.content_rect().shrink(20.0);
        let inner_size = (bounds.size() - egui::vec2(28.0, 64.0)).max(egui::vec2(500.0, 340.0));
        egui::Window::new(format!("Инструкция · {}", settings.engine.label()))
            .id(egui::Id::new("instruction_guide"))
            .open(&mut open)
            .fade_in(false)
            .collapsible(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(246, 247, 250))
                    .corner_radius(12)
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(226, 229, 237)))
                    .inner_margin(12.0),
            )
            .default_pos(bounds.min)
            .default_size(inner_size.min(egui::vec2(960.0, 680.0)))
            .min_size(inner_size.min(egui::vec2(660.0, 400.0)))
            .max_size(inner_size)
            .constrain_to(bounds)
            .show(ctx, |ui| {
                let old_tab = self.tab;
                crate::widgets::segmented(ui, &mut self.tab, &["Установка движка", "Окружение Pi"]);
                if old_tab != self.tab {
                    self.selected = 0;
                }
                ui.add_space(14.0);
                let titles: Vec<&str> = if self.tab == 0 {
                    preparation_sections(settings.engine).to_vec()
                } else {
                    crate::environment::STEPS
                        .iter()
                        .map(|(title, _)| *title)
                        .collect()
                };
                self.selected = self.selected.min(titles.len().saturating_sub(1));
                let height = ui.available_height().max(340.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(184.0, height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(184.0);
                            for (index, title) in titles.iter().enumerate() {
                                let prefix = if self.tab == 1 && completed[index] {
                                    "✓".into()
                                } else {
                                    (index + 1).to_string()
                                };
                                if ui
                                    .add_sized(
                                        [184.0, 40.0],
                                        egui::Button::new(format!("{prefix}  {title}"))
                                            .fill(if index == self.selected {
                                                Color32::from_rgb(240, 237, 255)
                                            } else {
                                                Color32::TRANSPARENT
                                            })
                                            .stroke(egui::Stroke::NONE)
                                            .corner_radius(8)
                                            .wrap(),
                                    )
                                    .clicked()
                                {
                                    self.selected = index;
                                }
                            }
                        },
                    );
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(titles[self.selected]).size(20.0));
                        ui.add_space(6.0);
                        let items = if self.tab == 0 {
                            preparation(settings.engine, self.selected)
                        } else {
                            crate::environment::step_instructions(self.selected, settings)
                        };
                        egui::ScrollArea::vertical()
                            .id_salt(("guide_content", self.tab, self.selected))
                            .max_height((height - 100.0).max(220.0))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (index, item) in items.iter().enumerate() {
                                    ui.push_id((self.tab, self.selected, index), |ui| {
                                        self.instruction_card(ui, ctx, item);
                                    });
                                    ui.add_space(10.0);
                                }
                            });
                        if self.tab == 1 {
                            ui.add_space(12.0);
                            let title = if completed[self.selected] {
                                "Повторить шаг в launcher"
                            } else {
                                "Выполнить шаг в launcher"
                            };
                            if crate::widgets::secondary(ui, title, available[self.selected])
                                .clicked()
                            {
                                action = Some(self.selected);
                            }
                        }
                    });
                });
            });
        self.open = open;
        action
    }

    fn instruction_card(
        &mut self,
        ui: &mut eframe::egui::Ui,
        ctx: &eframe::egui::Context,
        item: &Instruction,
    ) {
        use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke};
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(226, 229, 237)))
            .corner_radius(12)
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    // Reserve the copy button before the title so neither can push the other out.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(snippet) = &item.snippet {
                            let id = ui.id().with("copy");
                            let copied = self.copied.is_some_and(|(last, time)| {
                                last == id && time.elapsed().as_secs_f32() < 1.5
                            });
                            if ui
                                .add(
                                    egui::Button::new(if copied {
                                        "Скопировано ✓"
                                    } else {
                                        "Копировать"
                                    })
                                    .fill(Color32::WHITE)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(226, 229, 237)))
                                    .corner_radius(8)
                                    .min_size(egui::vec2(116.0, 32.0)),
                                )
                                .clicked()
                            {
                                ctx.copy_text(snippet.text.clone());
                                self.copied = Some((id, std::time::Instant::now()));
                                ctx.request_repaint_after(std::time::Duration::from_millis(1550));
                            }
                        }
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.label(RichText::new(&item.title).strong());
                        });
                    });
                });
                ui.add(egui::Label::new(&item.explanation).wrap());
                if let Some(condition) = &item.condition {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(255, 247, 228))
                        .corner_radius(6)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(condition).color(Color32::from_rgb(134, 95, 29)),
                                )
                                .wrap(),
                            );
                        });
                }
                if let Some((label, url)) = item.link {
                    ui.hyperlink_to(label, url);
                }
                if let Some(snippet) = &item.snippet {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&snippet.label)
                                .size(13.0)
                                .color(Color32::from_rgb(111, 117, 134)),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&snippet.label);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(245, 246, 250))
                        .corner_radius(8)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::both()
                                .id_salt("code")
                                .max_height(220.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&snippet.text).monospace().size(13.0),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend)
                                        .selectable(true),
                                    );
                                });
                        });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    #[test]
    fn preparation_commands_are_individual_and_conditions_are_separate() {
        for engine in [Engine::Podman, Engine::Docker] {
            for section in 0..preparation_sections(engine).len() {
                let items = preparation(engine, section);
                assert!(!items.is_empty());
                for item in items {
                    assert!(!item.explanation.is_empty());
                    if let Some(code) = item.snippet {
                        assert_eq!(code.text.lines().count(), 1);
                        assert!(!code.text.contains('#'));
                    }
                }
            }
        }
        assert!(preparation(Engine::Podman, 1)[1].condition.is_some());
    }

    #[test]
    fn copy_button_copies_only_its_command() {
        let ctx = egui::Context::default();
        let mut guide = Guide::default();
        let item = Instruction::command(
            "Check engine",
            "Explanation must not enter clipboard",
            "docker info",
        );
        let pos = egui::pos2(940.0, 44.0);
        let mut render = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        guide.instruction_card(ui, ctx, &item);
                    });
                },
            )
        };
        render(vec![]);
        render(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        let output = render(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(output.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "docker info")));
        assert!(guide.copied.is_some());
    }

    #[test]
    fn guide_window_renders_command_text_within_viewport() {
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.animation_time = 0.0);
        let mut guide = Guide {
            open: true,
            engine: Engine::Docker,
            selected: 1,
            ..Guide::default()
        };
        let settings = crate::model::Settings {
            engine: Engine::Docker,
            ..Default::default()
        };
        let mut found = false;
        for _ in 0..4 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 680.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    guide.show(ctx, &settings, &[false; 7], &[false; 7]);
                },
            );
            found |= output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("docker info")));
        }
        assert!(found, "guide should paint the command in its window");
    }
}
