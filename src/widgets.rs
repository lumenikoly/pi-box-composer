use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};

const INK: Color32 = Color32::from_rgb(32, 36, 49);
const QUIET: Color32 = Color32::from_rgb(116, 122, 137);
const LINE: Color32 = Color32::from_rgb(222, 225, 233);
const ACCENT: Color32 = Color32::from_rgb(97, 76, 214);
const HOVER: Color32 = Color32::from_rgb(249, 248, 255);

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
            Color32::from_rgb(212, 207, 235)
        } else if response.is_pointer_button_down_on() {
            Color32::from_rgb(75, 56, 182)
        } else if response.hovered() {
            Color32::from_rgb(111, 90, 227)
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

pub fn secondary_full(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    secondary_button(ui, text, enabled, true)
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
            Color32::from_rgb(234, 230, 252)
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
    ui.painter().text(
        rect.left_center() + Vec2::new(14.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(15.0),
        INK,
    );
    chevron(ui, rect);
    response
}

pub fn folder(ui: &mut Ui, path: &str) -> bool {
    let title = if path.is_empty() {
        "Выбрать папку"
    } else {
        path
    };
    let (rect, response) = surface(ui, 48.0, title);
    ui.painter().rect(
        rect,
        9,
        if response.hovered() {
            HOVER
        } else {
            Color32::WHITE
        },
        Stroke::new(1.0_f32, if response.hovered() { ACCENT } else { LINE }),
        StrokeKind::Inside,
    );
    let p = rect.left_center() + Vec2::new(14.0, -7.0);
    let points = [
        (0.0, 0.0),
        (6.0, 0.0),
        (8.0, 3.0),
        (18.0, 3.0),
        (18.0, 14.0),
        (0.0, 14.0),
        (0.0, 0.0),
    ]
    .into_iter()
    .map(|(x, y)| p + Vec2::new(x, y))
    .collect();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.4_f32, QUIET)));
    let text_rect = Rect::from_min_max(
        rect.min + Vec2::new(44.0, 0.0),
        rect.max - Vec2::new(88.0, 0.0),
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.into(),
        FontId::proportional(15.0),
        if path.is_empty() { QUIET } else { INK },
    );
    job.wrap.max_width = text_rect.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().with_clip_rect(text_rect).galley(
        text_rect.left_center() - Vec2::new(0.0, galley.size().y / 2.0),
        galley,
        INK,
    );
    ui.painter().text(
        rect.right_center() - Vec2::new(14.0, 0.0),
        Align2::RIGHT_CENTER,
        "Обзор…",
        FontId::proportional(15.0),
        ACCENT,
    );
    response.on_hover_text(title).clicked()
}

pub fn segmented(ui: &mut Ui, selected: &mut usize, labels: &[&str]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 9, Color32::from_rgb(240, 241, 245));
    let width = (rect.width() - 6.0) / labels.len() as f32;
    for (index, text) in labels.iter().enumerate() {
        let item = Rect::from_min_size(
            rect.min + Vec2::new(3.0 + index as f32 * width, 3.0),
            Vec2::new(width, 38.0),
        );
        let response = ui.interact(item, ui.id().with(("segment", index)), Sense::click());
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
