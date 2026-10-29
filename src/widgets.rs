use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};

const INK: Color32 = Color32::from_rgb(29, 38, 43);
const QUIET: Color32 = Color32::from_rgb(101, 114, 121);
const LINE: Color32 = Color32::from_rgb(224, 230, 230);
const ACCENT: Color32 = Color32::from_rgb(22, 112, 100);
const HOVER: Color32 = Color32::from_rgb(240, 248, 244);

#[derive(Clone, Copy)]
pub enum Icon {
    Folder,
    Plus,
    Settings,
    Plug,
    Terminal,
    History,
    Back,
    Help,
    Close,
    Puzzle,
}

pub fn icon(ui: &Ui, center: Pos2, kind: Icon, color: Color32, size: f32) {
    let stroke = Stroke::new(1.6_f32, color);
    let point = |x: f32, y: f32| center + Vec2::new(x, y) * (size / 20.0);
    let line = |points: &[(f32, f32)]| {
        ui.painter().add(egui::Shape::line(
            points.iter().map(|(x, y)| point(*x, *y)).collect(),
            stroke,
        ));
    };
    match kind {
        Icon::Folder => line(&[
            (-8., -6.),
            (-2., -6.),
            (0., -3.),
            (8., -3.),
            (8., 7.),
            (-8., 7.),
            (-8., -6.),
        ]),
        Icon::Plus => {
            line(&[(-7., 0.), (7., 0.)]);
            line(&[(0., -7.), (0., 7.)]);
        }
        Icon::Close => {
            line(&[(-5., -5.), (5., 5.)]);
            line(&[(5., -5.), (-5., 5.)]);
        }
        Icon::Back => {
            line(&[(-7., 0.), (7., 0.)]);
            line(&[(-1., -6.), (-7., 0.), (-1., 6.)]);
        }
        Icon::Terminal => {
            line(&[(-8., -7.), (8., -7.), (8., 7.), (-8., 7.), (-8., -7.)]);
            line(&[(-5., -3.), (-2., 0.), (-5., 3.)]);
            line(&[(1., 3.), (5., 3.)]);
        }
        Icon::History => {
            ui.painter().circle_stroke(center, size * 0.39, stroke);
            line(&[(0., -5.), (0., 0.), (4., 2.)]);
        }
        Icon::Plug => {
            line(&[(-5., -8.), (-5., -3.)]);
            line(&[(5., -8.), (5., -3.)]);
            line(&[
                (-7., -3.),
                (7., -3.),
                (7., 1.),
                (3., 5.),
                (-3., 5.),
                (-7., 1.),
                (-7., -3.),
            ]);
            line(&[(0., 5.), (0., 9.)]);
        }
        Icon::Settings => {
            ui.painter().circle_stroke(center, size * 0.3, stroke);
            ui.painter().circle_stroke(center, size * 0.11, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                let d = Vec2::angled(a);
                ui.painter()
                    .line_segment([center + d * size * 0.30, center + d * size * 0.45], stroke);
            }
        }
        Icon::Help => {
            ui.painter().circle_stroke(center, size * 0.42, stroke);
            ui.painter().text(
                center,
                Align2::CENTER_CENTER,
                "?",
                FontId::proportional(size * 0.8),
                color,
            );
        }
        Icon::Puzzle => {
            line(&[
                (-7., -7.),
                (-2., -7.),
                (-2., -9.),
                (2., -9.),
                (2., -7.),
                (7., -7.),
                (7., -2.),
                (4., -2.),
                (4., 2.),
                (7., 2.),
                (7., 7.),
                (-7., 7.),
                (-7., -7.),
            ]);
        }
    }
}

pub fn icon_button(ui: &mut Ui, kind: Icon, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 8, HOVER);
    }
    icon(ui, rect.center(), kind, INK, 19.0);
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn nav_button(ui: &mut Ui, kind: Icon, label: &str, selected: bool) -> Response {
    let (rect, response) = surface(ui, 42.0, label);
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            8,
            if selected {
                Color32::from_rgb(231, 244, 239)
            } else {
                HOVER
            },
        );
    }
    icon(
        ui,
        rect.left_center() + Vec2::new(18.0, 0.0),
        kind,
        if selected { ACCENT } else { QUIET },
        18.0,
    );
    clipped_text(
        ui,
        label,
        Rect::from_min_max(
            rect.min + Vec2::new(38.0, 0.0),
            rect.max - Vec2::new(10.0, 0.0),
        ),
        15.0,
        INK,
    );
    response
}

pub fn action(ui: &mut Ui, kind: Icon, label: &str, enabled: bool) -> bool {
    let start = ui.cursor().min;
    let clicked = button(ui, label, enabled);
    icon(
        ui,
        start + Vec2::new(24.0, 22.0),
        kind,
        Color32::WHITE,
        19.0,
    );
    clicked
}

pub fn icon_segments(ui: &mut Ui, selected: &mut usize, items: &[(Icon, &str)]) {
    let (rect, group) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 9, Color32::from_rgb(238, 241, 242));
    let width = (rect.width() - 6.0) / items.len() as f32;
    for (index, (kind, label)) in items.iter().enumerate() {
        let item = Rect::from_min_size(
            rect.min + Vec2::new(3.0 + index as f32 * width, 3.0),
            Vec2::new(width, 38.0),
        );
        let response = ui.interact(item, group.id.with(index), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                *selected == index,
                *label,
            )
        });
        if response.clicked() {
            *selected = index;
        }
        if *selected == index || response.has_focus() {
            ui.painter().rect(
                item,
                7,
                Color32::WHITE,
                Stroke::new(1.0_f32, if response.has_focus() { ACCENT } else { LINE }),
                StrokeKind::Inside,
            );
        }
        let color = if *selected == index { ACCENT } else { QUIET };
        icon(
            ui,
            item.left_center() + Vec2::new(20.0, 0.0),
            *kind,
            color,
            18.0,
        );
        clipped_text(
            ui,
            label,
            Rect::from_min_max(
                item.min + Vec2::new(39.0, 0.0),
                item.max - Vec2::new(8.0, 0.0),
            ),
            14.0,
            color,
        );
    }
}

fn surface(ui: &mut Ui, height: f32, name: &str) -> (Rect, Response) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(3.0),
            11,
            Stroke::new(2.0_f32, ACCENT),
            StrokeKind::Outside,
        );
    }
    (rect, response)
}

pub fn button(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        let (rect, response) = surface(ui, 44.0, text);
        let fill = if !ui.is_enabled() {
            Color32::from_rgb(174, 199, 192)
        } else if response.is_pointer_button_down_on() {
            Color32::from_rgb(17, 88, 79)
        } else if response.hovered() {
            Color32::from_rgb(27, 128, 113)
        } else {
            ACCENT
        };
        ui.painter().rect_filled(rect, 9, fill);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(15.0),
            Color32::WHITE,
        );
        response.clicked()
    })
    .inner
}

fn chevron(ui: &Ui, rect: Rect) {
    let center = Pos2::new(rect.right() - 18.0, rect.center().y);
    ui.painter().add(egui::Shape::line(
        vec![
            center + Vec2::new(-4.0, -2.0),
            center + Vec2::new(0.0, 2.0),
            center + Vec2::new(4.0, -2.0),
        ],
        Stroke::new(1.5_f32, QUIET),
    ));
}

pub fn secondary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    secondary_button(ui, text, enabled, false)
}

fn secondary_button(ui: &mut Ui, text: &str, enabled: bool, full: bool) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        let font = FontId::proportional(15.0);
        let color = if ui.is_enabled() { INK } else { QUIET };
        let galley = ui.painter().layout_no_wrap(text.into(), font, color);
        let width = if full {
            ui.available_width()
        } else {
            (galley.size().x + 32.0).min(ui.available_width())
        };
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(width, if full { 44.0 } else { 36.0 }),
            Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text)
        });
        let active = ui.is_enabled();
        let fill = if !active {
            Color32::from_rgb(245, 246, 249)
        } else if response.is_pointer_button_down_on() {
            Color32::from_rgb(225, 240, 234)
        } else if response.hovered() {
            HOVER
        } else {
            Color32::WHITE
        };
        let line = if active && (response.hovered() || response.has_focus()) {
            ACCENT
        } else {
            LINE
        };
        ui.painter().rect(
            rect,
            9,
            fill,
            Stroke::new(1.0_f32, line),
            StrokeKind::Inside,
        );
        ui.painter()
            .with_clip_rect(rect.shrink2(Vec2::new(10.0, 3.0)))
            .galley(rect.center() - galley.size() / 2.0, galley, color);
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                11,
                Stroke::new(1.5_f32, ACCENT),
                StrokeKind::Outside,
            );
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
}

pub fn select(ui: &mut Ui, text: &str) -> Response {
    select_with_icon(ui, text, None)
}

pub fn select_icon(ui: &mut Ui, text: &str, kind: Icon) -> Response {
    select_with_icon(ui, text, Some(kind))
}

fn select_with_icon(ui: &mut Ui, text: &str, kind: Option<Icon>) -> Response {
    let (rect, response) = surface(ui, 44.0, text);
    ui.painter().rect(
        rect,
        9,
        if response.hovered() {
            HOVER
        } else {
            Color32::WHITE
        },
        Stroke::new(1.0_f32, LINE),
        StrokeKind::Inside,
    );
    clipped_text(
        ui,
        text,
        Rect::from_min_max(
            rect.min + Vec2::new(if kind.is_some() { 42.0 } else { 14.0 }, 0.0),
            rect.max - Vec2::new(34.0, 0.0),
        ),
        15.0,
        INK,
    );
    if let Some(kind) = kind {
        icon(
            ui,
            rect.left_center() + Vec2::new(21.0, 0.0),
            kind,
            QUIET,
            18.0,
        );
    }
    chevron(ui, rect);
    response
}

fn clipped_text(ui: &Ui, title: &str, text_rect: Rect, size: f32, color: Color32) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(title.into(), FontId::proportional(size), color);
    job.wrap.max_width = text_rect.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().with_clip_rect(text_rect).galley(
        text_rect.left_center() - Vec2::new(0.0, galley.size().y / 2.0),
        galley,
        color,
    );
}

pub fn project_row(
    ui: &mut Ui,
    title: &str,
    path: &str,
    selected: bool,
    pinned: bool,
) -> (Response, bool) {
    let (rect, response) = surface(ui, 42.0, title);
    ui.painter().rect_filled(
        rect,
        8,
        if selected {
            Color32::from_rgb(231, 244, 239)
        } else if response.hovered() {
            HOVER
        } else {
            Color32::TRANSPARENT
        },
    );
    let inner = Rect::from_min_max(
        rect.min + Vec2::new(36.0, 9.0),
        rect.max - Vec2::new(8.0, 9.0),
    );
    icon(
        ui,
        rect.left_center() + Vec2::new(17.0, 0.0),
        Icon::Folder,
        if selected { ACCENT } else { QUIET },
        17.0,
    );
    clipped_text(
        ui,
        title,
        Rect::from_min_size(inner.min, Vec2::new(inner.width() - 24.0, 24.0)),
        15.0,
        if selected { ACCENT } else { INK },
    );
    let pin_rect = Rect::from_min_size(rect.right_top() + Vec2::new(-32.0, 4.0), Vec2::splat(28.0));
    let pin = ui.interact(pin_rect, response.id.with("pin"), Sense::click());
    pin.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            if pinned {
                "Открепить проект"
            } else {
                "Закрепить проект"
            },
        )
    });
    ui.painter().text(
        pin_rect.center(),
        Align2::CENTER_CENTER,
        if pinned { "★" } else { "☆" },
        FontId::proportional(18.0),
        if pinned || pin.hovered() {
            ACCENT
        } else {
            QUIET
        },
    );
    let clicked = pin
        .on_hover_text(if pinned {
            "Открепить"
        } else {
            "Закрепить"
        })
        .clicked();
    (
        response.on_hover_text(format!(
            "{path}\nДвойной щелчок — открыть pi\nПравая кнопка — закрепить или убрать"
        )),
        clicked,
    )
}

pub fn segmented(ui: &mut Ui, selected: &mut usize, labels: &[&str]) {
    let (rect, group) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 9, Color32::from_rgb(240, 241, 245));
    let width = (rect.width() - 6.0) / labels.len() as f32;
    for (index, text) in labels.iter().enumerate() {
        let item = Rect::from_min_size(
            rect.min + Vec2::new(3.0 + index as f32 * width, 3.0),
            Vec2::new(width, 38.0),
        );
        let response = ui.interact(item, group.id.with(index), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                index == *selected,
                *text,
            )
        });
        if response.clicked() {
            *selected = index;
        }
        if index == *selected {
            ui.painter().rect(
                item,
                7,
                Color32::WHITE,
                Stroke::new(1.0_f32, LINE),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter()
                .rect_filled(item, 7, Color32::from_rgb(232, 233, 240));
        }
        if response.has_focus() {
            ui.painter()
                .rect_stroke(item, 7, Stroke::new(2.0_f32, ACCENT), StrokeKind::Inside);
        }
        ui.painter().text(
            item.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(15.0),
            if index == *selected { INK } else { QUIET },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, mut render: impl FnMut(&mut Ui)) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render(ui);
                });
            },
        );
    }

    fn pointer(pos: Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn disabled_primary_does_not_trigger_and_enabled_one_does() {
        for enabled in [false, true] {
            let ctx = egui::Context::default();
            let mut clicked = false;
            let pos = Pos2::new(80.0, 25.0);
            frame(&ctx, vec![], |ui| {
                clicked |= button(ui, "Run", enabled);
            });
            frame(&ctx, pointer(pos, true), |ui| {
                clicked |= button(ui, "Run", enabled);
            });
            frame(&ctx, pointer(pos, false), |ui| {
                clicked |= button(ui, "Run", enabled);
            });
            assert_eq!(clicked, enabled);
        }
    }

    #[test]
    fn dropdown_supports_keyboard_activation() {
        let ctx = egui::Context::default();
        let mut clicked = false;
        frame(&ctx, vec![], |ui| {
            select(ui, "Session").request_focus();
        });
        frame(
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            |ui| {
                clicked = select(ui, "Session").clicked();
            },
        );
        assert!(clicked);
    }

    #[test]
    fn multiple_segment_groups_do_not_share_clicks_or_selection() {
        let ctx = egui::Context::default();
        let mut first = 0;
        let mut second = 0;
        let bottom = Pos2::new(450.0, 80.0);
        for events in [vec![], pointer(bottom, true), pointer(bottom, false)] {
            frame(&ctx, events, |ui| {
                segmented(ui, &mut first, &["Ask", "Allow"]);
                segmented(ui, &mut second, &["New", "Continue"]);
            });
        }
        assert_eq!((first, second), (0, 1));
        let top = Pos2::new(450.0, 25.0);
        for events in [pointer(top, true), pointer(top, false)] {
            frame(&ctx, events, |ui| {
                segmented(ui, &mut first, &["Ask", "Allow"]);
                segmented(ui, &mut second, &["New", "Continue"]);
            });
        }
        assert_eq!((first, second), (1, 1));
    }

    #[test]
    fn permission_segment_changes_only_after_activation() {
        let ctx = egui::Context::default();
        let mut selected = 0;
        let pos = Pos2::new(450.0, 25.0);
        frame(&ctx, vec![], |ui| {
            segmented(ui, &mut selected, &["Read", "Write"]);
        });
        frame(&ctx, vec![egui::Event::PointerMoved(pos)], |ui| {
            segmented(ui, &mut selected, &["Read", "Write"]);
        });
        assert_eq!(selected, 0);
        frame(&ctx, pointer(pos, true), |ui| {
            segmented(ui, &mut selected, &["Read", "Write"]);
        });
        frame(&ctx, pointer(pos, false), |ui| {
            segmented(ui, &mut selected, &["Read", "Write"]);
        });
        assert_eq!(selected, 1);
    }
}
