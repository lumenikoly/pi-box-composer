use super::*;
use crate::widgets::{self, Icon};

fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(MUTED));
}

fn session_picker(ui: &mut egui::Ui, session: &mut Session) {
    let response = widgets::select_icon(
        ui,
        session.label(),
        if *session == Session::New {
            Icon::Plus
        } else {
            Icon::History
        },
    );
    egui::Popup::menu(&response)
        .width(response.rect.width())
        .show(|ui| {
            for value in [Session::New, Session::Continue, Session::Resume] {
                if ui.selectable_value(session, value, value.label()).clicked() {
                    ui.close();
                }
            }
        });
}

fn permission_picker(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut Option<CommandAccess>,
    inherited: Option<CommandAccess>,
) {
    let title = value.map_or_else(
        || {
            format!(
                "Из настроек: {}",
                inherited.unwrap_or_default().label().to_lowercase()
            )
        },
        |v| v.label().to_owned(),
    );
    ui.push_id(id, |ui| {
        let response = ui
            .allocate_ui(egui::vec2(260.0, 44.0), |ui| widgets::select(ui, &title))
            .inner;
        egui::Popup::menu(&response)
            .width(response.rect.width())
            .show(|ui| {
                if inherited.is_some() && ui.selectable_value(value, None, "Из настроек").clicked()
                {
                    ui.close();
                }
                for item in [
                    CommandAccess::Disabled,
                    CommandAccess::Ask,
                    CommandAccess::Allow,
                ] {
                    if ui
                        .selectable_value(value, Some(item), item.label())
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
    });
}

fn permission_rows(
    ui: &mut egui::Ui,
    write: &mut Option<CommandAccess>,
    commands: &mut Option<CommandAccess>,
    defaults: Option<(CommandAccess, CommandAccess)>,
) {
    egui::Grid::new("permissions").spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Чтение").on_hover_text("Файлы workspace, кроме запрещённых политикой путей, например .env.");
        ui.label("Разрешено");
        ui.end_row();
        ui.label("Запись").on_hover_text("Правки через edit и write. При запрете workspace монтируется только для чтения. Bash регулируется отдельно.");
        permission_picker(ui, "write", write, defaults.map(|v| v.0));
        ui.end_row();
        ui.label("Команды").on_hover_text("Bash может изменять файлы, если запись в workspace не запрещена.");
        permission_picker(ui, "commands", commands, defaults.map(|v| v.1));
        ui.end_row();
    });
}

fn permission_rules(ui: &mut egui::Ui, rules: &mut Vec<PermissionRule>) {
    ui.collapsing("Правила", |ui| {
        let mut remove = None;
        for (index, rule) in rules.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("target")
                        .selected_text(rule.target.label()).width(76.0).show_ui(ui, |ui| {
                            for target in [RuleTarget::Read, RuleTarget::Write, RuleTarget::Command] {
                                ui.selectable_value(&mut rule.target, target, target.label());
                            }
                        });
                    ui.add(egui::TextEdit::singleline(&mut rule.pattern).desired_width(130.0)
                        .hint_text(if rule.target == RuleTarget::Command { "git status*" } else { "/workspace/src/*" }))
                        .on_hover_text("Шаблон пути для инструментов чтения/правки или команды bash. Последнее совпавшее правило имеет приоритет. Запрет записи в workspace и встроенные ограничения сохраняются.");
                    egui::ComboBox::from_id_salt("action")
                        .selected_text(rule.action.label()).width(100.0).show_ui(ui, |ui| {
                            for action in [CommandAccess::Disabled, CommandAccess::Ask, CommandAccess::Allow] {
                                ui.selectable_value(&mut rule.action, action, action.label());
                            }
                        });
                    if widgets::icon_button(ui, Icon::Close, "Удалить правило").clicked() {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(index) = remove { rules.remove(index); }
        if ui.small_button("Добавить правило").clicked() {
            rules.push(PermissionRule::default());
        }
    });
}

impl Launcher {
    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("workspace_sidebar")
            .exact_width(236.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(18.0)
                    .stroke(Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.add_space(10.0);
                let busy = self.busy();
                ui.add_enabled_ui(!busy, |ui| {
                    if widgets::action(ui, Icon::Plus, "Открыть папку", true) {
                        self.pick_project();
                    }
                    ui.add_space(22.0);
                    label(ui, "Проекты");
                    if !self.saved.recent.is_empty() {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("Поиск…")
                                .desired_width(f32::INFINITY)
                                .margin(egui::vec2(9.0, 8.0)),
                        );
                    }
                    let query = self.search.to_lowercase();
                    let projects: Vec<_> = self
                        .saved
                        .recent
                        .iter()
                        .filter(|p| p.path.to_lowercase().contains(&query))
                        .cloned()
                        .collect();
                    egui::ScrollArea::vertical()
                        .id_salt("projects")
                        .max_height((ui.available_height() - 124.0).max(90.0))
                        .show(ui, |ui| {
                            if projects.is_empty() {
                                label(
                                    ui,
                                    if query.is_empty() {
                                        "Нет недавних папок"
                                    } else {
                                        "Ничего не найдено"
                                    },
                                );
                            }
                            for project in projects {
                                ui.push_id(&project.path, |ui| {
                                    let selected = normalized_path(Path::new(&project.path))
                                        == normalized_path(Path::new(&self.saved.project))
                                        && self.page == Page::Project;
                                    let (response, pin) = widgets::project_row(
                                        ui,
                                        &project_name(&project.path),
                                        &project.path,
                                        selected,
                                        project.pinned,
                                    );
                                    if pin {
                                        self.toggle_pin(&project.path);
                                    }
                                    if response.clicked() {
                                        self.choose_project(&project.path);
                                    }
                                    if response.double_clicked() {
                                        self.request_launch(ctx);
                                    }
                                    response.context_menu(|ui| {
                                        if ui.button("Открыть pi").clicked() {
                                            self.choose_project(&project.path);
                                            self.request_launch(ctx);
                                            ui.close();
                                        }
                                        if ui.button("Открыть папку сессий").clicked()
                                        {
                                            self.manage_sessions(&project.path, false);
                                            ui.close();
                                        }
                                        if ui.button("Удалить сессии…").clicked() {
                                            self.manage_sessions(&project.path, true);
                                            ui.close();
                                        }
                                        ui.separator();
                                        if ui
                                            .button(if project.pinned {
                                                "Открепить"
                                            } else {
                                                "Закрепить"
                                            })
                                            .clicked()
                                        {
                                            self.toggle_pin(&project.path);
                                            ui.close();
                                        }
                                        if ui.button("Убрать из списка").clicked() {
                                            self.saved.recent.retain(|p| p.path != project.path);
                                            self.dirty_at = Some(Instant::now());
                                            ui.close();
                                        }
                                    });
                                });
                            }
                        });
                });
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    let help = widgets::nav_button(ui, Icon::Help, "Помощь", false);
                    egui::Popup::menu(&help).show(|ui| {
                        if ui.button("Инструкция и команды").clicked() {
                            self.guide.open = true;
                            ui.close();
                        }
                        if ui.button("Диагностика").clicked() {
                            self.show_diagnostics = true;
                            ui.close();
                        }
                        ui.separator();
                        label(ui, "Ctrl+O — папка");
                        label(ui, "Ctrl+Enter — запуск");
                    });
                    ui.add_enabled_ui(!busy, |ui| {
                        if widgets::nav_button(
                            ui,
                            Icon::Settings,
                            "Настройки",
                            self.page != Page::Project,
                        )
                        .clicked()
                        {
                            self.page = Page::Preferences;
                            self.preferences_tab = 0;
                            self.pending_launch = false;
                            self.message.clear();
                        }
                    });
                    ui.add_space(8.0);
                    ui.separator();
                });
            });
    }

    fn toggle_pin(&mut self, path: &str) {
        if let Some(project) = self.saved.recent.iter_mut().find(|p| p.path == path) {
            project.pinned = !project.pinned;
        }
        self.saved.recent.sort_by_key(|p| !p.pinned);
        self.dirty_at = Some(Instant::now());
    }

    fn manage_sessions(&mut self, project: &str, delete: bool) {
        let result = (|| -> anyhow::Result<Option<String>> {
            let sessions =
                crate::sessions::Sessions::inspect(&self.saved.settings, Path::new(project))?;
            if !delete {
                sessions.open()?;
                return Ok(None);
            }
            if sessions.count() == 0 {
                return Ok(Some("У проекта нет сохранённых сессий.".into()));
            }
            let choice = rfd::MessageDialog::new()
                .set_title("Удалить сессии")
                .set_description(format!(
                    "Проект: {project}\nДвижок: {}\nСессий: {}\n\nЗакройте pi этого проекта перед удалением. История будет удалена безвозвратно. Файлы проекта и настройки останутся.",
                    self.saved.settings.engine.label(), sessions.count()
                ))
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::OkCancelCustom("Удалить".into(), "Отмена".into()))
                .show();
            // Windows without common-controls-v6 returns Ok for the custom OK button.
            if choice != rfd::MessageDialogResult::Ok
                && choice != rfd::MessageDialogResult::Custom("Удалить".into())
            {
                return Ok(None);
            }
            let count = sessions.delete()?;
            Ok(Some(format!("Удалено сессий: {count}.")))
        })();
        match result {
            Ok(Some(message)) => {
                self.message = message;
                self.error = false;
            }
            Ok(None) => {}
            Err(error) => {
                self.message = format!("{error:#}");
                self.error = true;
            }
        }
    }

    fn choose_project(&mut self, path: &str) {
        self.saved.remember_project_preferences();
        self.saved.select_project(path);
        self.project_input = self.saved.project.clone();
        self.page = Page::Project;
        self.pending_launch = false;
        self.message.clear();
        self.error = false;
        self.dirty_at = Some(Instant::now());
    }

    pub(super) fn accept_project_input(&mut self) {
        let path = self.project_input.clone();
        self.saved.remember_project_preferences();
        self.saved.select_project(&path);
        self.project_input = self.saved.project.clone();
        self.dirty_at = Some(Instant::now());
    }

    pub(super) fn request_launch(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        self.accept_project_input();
        if !Path::new(&self.saved.project).is_dir() {
            self.error = true;
            self.message = "Папка не найдена. Проверьте путь.".into();
            return;
        }
        if environment::is_configured(&self.saved.settings) {
            self.start(Job::Launch, ctx);
        } else {
            self.open_setup();
            self.pending_launch = true;
        }
    }

    pub(super) fn pick_project(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Где открыть pi?");
        if Path::new(&self.saved.project).is_dir() {
            dialog = dialog.set_directory(&self.saved.project);
        }
        if let Some(path) = dialog.pick_folder() {
            self.choose_project(&path.display().to_string());
        }
    }

    pub(super) fn project(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy();
        let ready = environment::is_configured(&self.saved.settings);
        ui.horizontal(|ui| {
            label(ui, "Проекты");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let model = if ready {
                    self.saved.settings.model.clone()
                } else {
                    "Подключить модель".into()
                };
                let response = ui.add_enabled(
                    !busy,
                    egui::Button::new(RichText::new(model).color(ACCENT)).frame(false),
                );
                if response
                    .on_hover_text(if ready {
                        "Настройки подключения. Доступность проверится при запуске."
                    } else {
                        "Настройте модель один раз для всех проектов"
                    })
                    .clicked()
                {
                    self.open_setup();
                    self.pending_launch = false;
                }
            });
        });
        ui.add_space(48.0);
        let empty = self.project_input.trim().is_empty();
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 12, TINT);
            widgets::icon(ui, rect.center(), Icon::Folder, ACCENT, 26.0);
            ui.add(
                egui::Label::new(
                    RichText::new(if empty {
                        "Открыть проект".into()
                    } else {
                        project_name(self.project_input.trim().trim_matches('"'))
                    })
                    .size(28.0)
                    .strong(),
                )
                .truncate(),
            );
        });
        ui.add_space(18.0);
        self.notice(ui);
        let before = (self.saved.permissions.clone(), self.saved.session);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 46.0).max(80.0);
                    let response = ui.add_sized(
                        [width, 44.0],
                        egui::TextEdit::singleline(&mut self.project_input)
                            .hint_text("Путь к папке проекта")
                            .margin(egui::vec2(12.0, 10.0)),
                    );
                    ui.painter().rect_stroke(
                        response.rect,
                        8,
                        Stroke::new(1.0_f32, if response.has_focus() { ACCENT } else { BORDER }),
                        egui::StrokeKind::Inside,
                    );
                    if response
                        .on_hover_text("Вставьте путь или перетащите сюда папку")
                        .lost_focus()
                    {
                        self.accept_project_input();
                    }
                    if widgets::icon_button(ui, Icon::Folder, "Выбрать папку · Ctrl+O").clicked()
                    {
                        self.pick_project();
                    }
                });
                if !empty {
                    ui.add_space(20.0);
                    let defaults = (self.saved.pi.write_access(), self.saved.pi.commands);
                    let permissions = self.saved.editable_permissions();
                    permission_rows(
                        ui,
                        &mut permissions.write,
                        &mut permissions.commands,
                        Some(defaults),
                    );
                    permission_rules(ui, &mut permissions.rules);
                    ui.add_space(12.0);
                    label(ui, "Сессия");
                    session_picker(ui, &mut self.saved.session);
                }
            });
            ui.add_space(24.0);
            let title = if busy {
                "Открываем pi…"
            } else if empty {
                "Выбрать папку"
            } else if !ready {
                "Настроить и открыть pi"
            } else {
                "Открыть pi"
            };
            if widgets::action(
                ui,
                if empty { Icon::Folder } else { Icon::Terminal },
                title,
                !busy,
            ) {
                if empty {
                    self.pick_project();
                } else {
                    self.request_launch(ctx);
                }
            }
            if busy {
                self.progress(ui);
            }
        });
        if before != (self.saved.permissions.clone(), self.saved.session) {
            self.saved.access = self.saved.effective_permissions().0;
            self.saved.remember_project_preferences();
            self.dirty_at = Some(Instant::now());
        }
    }

    fn settings_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, Icon::Back, "К проектам").clicked() && !self.busy()
            {
                self.page = Page::Project;
                self.pending_launch = false;
            }
            ui.label(RichText::new("Настройки").size(28.0).strong());
        });
        ui.add_space(20.0);
        let old = if self.page == Page::Setup {
            2
        } else {
            self.preferences_tab.min(1)
        };
        let mut tab = old;
        ui.add_enabled_ui(!self.busy(), |ui| {
            widgets::icon_segments(
                ui,
                &mut tab,
                &[
                    (Icon::Settings, "Основные"),
                    (Icon::Puzzle, "Плагины"),
                    (Icon::Plug, "Модель"),
                ],
            );
        });
        if tab != old {
            self.pending_launch = false;
            if tab == 2 {
                self.open_setup();
            } else {
                self.page = Page::Preferences;
                self.preferences_tab = tab;
            }
        }
        ui.add_space(20.0);
        self.notice(ui);
    }

    pub(super) fn preferences(&mut self, ui: &mut egui::Ui) {
        self.settings_header(ui);
        let before = self.saved.pi.clone();
        ui.add_enabled_ui(!self.busy(), |ui| {
            if self.preferences_tab == 0 {
                self.general_preferences(ui);
            } else {
                self.plugin_preferences(ui);
            }
        });
        if before != self.saved.pi {
            self.dirty_at = Some(Instant::now());
        }
    }

    fn general_preferences(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Разрешения").strong());
            ui.add_space(10.0);
            let mut write = Some(self.saved.pi.write_access());
            let mut commands = Some(self.saved.pi.commands);
            permission_rows(ui, &mut write, &mut commands, None);
            self.saved.pi.set_write_access(write.unwrap());
            self.saved.pi.commands = commands.unwrap();
            permission_rules(ui, &mut self.saved.pi.rules);
            ui.add_space(12.0);
            ui.collapsing("Подробнее о доступе", |ui| {
                secondary(ui, "pi видит папку проекта, своё состояние и выбранные плагины. Домашняя папка и SSH-ключи не подключаются. Сеть ограничена сервером модели.");
                secondary(ui, "В режиме просмотра папка доступна только для чтения. Секреты внутри неё доступны агенту. Настройки инструментов не ограничивают собственный код плагинов.");
            });
        });
        ui.add_space(16.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Для новых папок").strong())
                .on_hover_text("Знакомые проекты помнят выбранную сессию.");
            ui.add_space(10.0);
            label(ui, "Сессия");
            session_picker(ui, &mut self.saved.pi.default_session);
        });
        ui.add_space(16.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Интерфейс pi").strong());
            ui.add_space(10.0);
            let mut theme = usize::from(self.saved.pi.light_theme);
            widgets::segmented(ui, &mut theme, &["Тёмная тема", "Светлая тема"]);
            self.saved.pi.light_theme = theme == 1;
            ui.checkbox(&mut self.saved.pi.auto_compact, "Сжимать длинный контекст")
                .on_hover_text(
                    "pi автоматически делает резюме разговора при приближении к лимиту модели.",
                );
        });
        ui.add_space(16.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Проводник Windows").strong());
            ui.add_space(10.0);
            let mut enabled = self.saved.explorer_menu.unwrap_or(true);
            if ui.checkbox(&mut enabled, "«Открыть в pi» в меню папок")
                .on_hover_text("Для текущего пользователя. В Windows 11 — в «Показать дополнительные параметры».")
                .changed()
            {
                match crate::explorer::configure(&self.data_dir, enabled) {
                    Ok(()) => {
                        self.saved.explorer_menu = Some(enabled);
                        self.error = false;
                        self.message.clear();
                        self.persist();
                    }
                    Err(error) => {
                        self.error = true;
                        self.message = format!("{error:#}");
                    }
                }
            }
        });
    }

    fn plugin_preferences(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Контроль разрешений").strong()).on_hover_text("@gotgenes/pi-permission-system 41.0.0 — встроен и всегда включён");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| { ui.colored_label(ACCENT, "Встроен"); });
            });
            ui.add_space(12.0);
            ui.separator();
            let mut remove = None;
            for (index, plugin) in self.saved.pi.plugins.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::icon_button(ui, Icon::Close, "Убрать плагин из списка").clicked() { remove = Some(index); }
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.checkbox(&mut plugin.enabled, project_name(&plugin.path)).on_hover_text(&plugin.path);
                            });
                        });
                    });
                    if !Path::new(&plugin.path).is_dir() { ui.colored_label(RED, "Папка плагина недоступна"); }
                });
            }
            if let Some(index) = remove { self.saved.pi.plugins.remove(index); }
            ui.add_space(8.0);
            if widgets::nav_button(ui, Icon::Plus, "Добавить плагин", false).on_hover_text("Локальная папка с index.ts или index.js. Установите Linux-совместимые зависимости заранее.").clicked()
                && let Some(path) = rfd::FileDialog::new().set_title("Папка плагина с index.ts или index.js").pick_folder() {
                    match environment::validate_plugin(&path.display().to_string(), &self.saved.settings, &self.data_dir) {
                        Ok(path) => {
                            if !self.saved.pi.plugins.iter().any(|p| normalized_path(Path::new(&p.path)) == normalized_path(&path)) {
                                self.saved.pi.plugins.push(Plugin { path: path.display().to_string(), enabled: true });
                            }
                        },
                        Err(error) => { self.error = true; self.message = format!("{error:#}"); }
                    }
                }
        });
        ui.add_space(16.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.checkbox(&mut self.saved.pi.skills, "Навыки проекта").on_hover_text("Загружать .pi/skills из выбранной папки. Если папки нет, запуск продолжится без навыков.");
        });
    }

    pub(super) fn setup(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.settings_header(ui);
        if self.pending_launch {
            label(
                ui,
                &format!("После настройки: {}", project_name(&self.saved.project)),
            );
            ui.add_space(10.0);
        }
        let busy = self.busy();
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if busy {
                ui.label(RichText::new("Подключаем модель…").strong());
                let complete = self.steps.iter().filter(|s| **s == Status::Done).count();
                ui.add(egui::ProgressBar::new(complete as f32 / 7.0).fill(ACCENT));
                self.progress(ui);
            } else if self.stage == 2 {
                label(ui, "Модель");
                let response = widgets::select(ui, &self.saved.settings.model);
                egui::Popup::menu(&response)
                    .width(response.rect.width())
                    .show(|ui| {
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .show(ui, |ui| {
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
                    });
                ui.add_space(12.0);
                ui.collapsing("Лимиты", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Контекст");
                        ui.add(
                            egui::DragValue::new(&mut self.saved.settings.context_window)
                                .range(1..=2_000_000)
                                .speed(1024),
                        );
                        ui.label("Ответ");
                        ui.add(
                            egui::DragValue::new(&mut self.saved.settings.max_tokens)
                                .range(1..=2_000_000)
                                .speed(256),
                        );
                    });
                });
                ui.add_space(20.0);
                if primary(
                    ui,
                    if self.pending_launch {
                        "Сохранить и открыть pi"
                    } else {
                        "Сохранить"
                    },
                    !self.models.is_empty(),
                ) {
                    self.start(Job::Step(6), ctx);
                }
                if widgets::nav_button(ui, Icon::Back, "Изменить сервер", false).clicked()
                {
                    self.stage = 0;
                }
            } else {
                self.connection_form(ui, ctx);
            }
        });
    }

    fn connection_form(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let before = self.saved.settings.clone();
        label(ui, "Движок");
        let mut engine = usize::from(self.saved.settings.engine == Engine::Docker);
        widgets::segmented(ui, &mut engine, &["Podman", "Docker Desktop"]);
        self.saved.settings.engine = if engine == 0 {
            Engine::Podman
        } else {
            Engine::Docker
        };
        if self.saved.settings.engine != before.engine {
            self.saved.settings = environment::load_engine_profile(&self.saved.settings);
            self.token.zeroize();
            self.steps.fill(Status::Pending);
            self.models.clear();
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 104.0).max(140.0);
            ui.vertical(|ui| { ui.set_width(width); label(ui, "Сервер llama.cpp"); field(ui, &mut self.saved.settings.host, "IP или имя сервера", false).on_hover_text("На этом ПК: host.docker.internal (Docker) или host.containers.internal (Podman). В сети: IP сервера."); });
            ui.vertical(|ui| { ui.set_width(96.0); label(ui, "Порт"); ui.add_sized([96.0,44.0], egui::DragValue::new(&mut self.saved.settings.port).range(1..=65535)); });
        });
        ui.add_space(8.0);
        label(ui, "API-токен");
        field(ui, &mut self.token, "Сохранённый или новый токен", true)
            .on_hover_text("Пустое поле использует ранее сохранённый токен.");
        ui.add_space(12.0);
        ui.collapsing("Дополнительно", |ui| {
            label(ui, "Каталог окружения");
            field(ui, &mut self.saved.settings.root, "Папка окружения", false);
            if widgets::nav_button(ui, Icon::Plug, "Запустить движок", false).clicked()
            {
                self.start(Job::StartMachine, ctx);
            }
        });
        if before != self.saved.settings {
            self.steps.fill(Status::Pending);
            self.models.clear();
            self.dirty_at = Some(Instant::now());
        }
        ui.add_space(20.0);
        let validation = self.saved.settings.validate();
        if let Err(error) = &validation {
            ui.colored_label(RED, error.to_string());
        }
        if widgets::action(
            ui,
            Icon::Plug,
            "Подключить",
            validation.is_ok() && !self.busy(),
        ) {
            self.stage = 1;
            self.start(Job::Setup, ctx);
        }
    }
}
