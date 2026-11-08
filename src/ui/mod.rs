pub mod dashboard;
pub mod entry;
pub mod fatal;
pub mod import;
pub mod invoices;
pub mod login;
pub mod people;
pub mod settings;
pub mod sms;

use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Layout, Pos2, Rect,
    Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme;

// ── Buttons ──────────────────────────────────────────────────────────────
/// Filled call-to-action button that matches the brand accent.
pub fn primary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    primary_button_enabled(ui, label, width, true)
}

/// Like [`primary_button`], but can be rendered disabled.
pub fn primary_button_enabled(ui: &mut Ui, label: &str, width: f32, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 42.0), sense);

    let fill = if !enabled {
        theme::card_raised()
    } else if response.is_pointer_button_down_on() {
        theme::accent_dark()
    } else if response.hovered() {
        theme::accent_hover()
    } else {
        theme::accent()
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(14.0, FontFamily::Proportional),
        if enabled {
            theme::ink()
        } else {
            theme::faint()
        },
    );

    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A destructive action button.
pub fn danger_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 38.0), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        theme::red().gamma_multiply(0.8)
    } else if response.hovered() {
        theme::red().gamma_multiply(0.9)
    } else {
        theme::red()
    };
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(12.5, FontFamily::Proportional),
        theme::ink(),
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Outlined, lower-emphasis button.
pub fn secondary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 38.0), Sense::click());
    let (fill, stroke, text) = if response.is_pointer_button_down_on() {
        (theme::card_hover(), theme::border(), theme::text())
    } else if response.hovered() {
        (
            theme::card_raised(),
            theme::accent().gamma_multiply(0.6),
            theme::text(),
        )
    } else {
        (Color32::TRANSPARENT, theme::border(), theme::muted())
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(9), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(9),
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(13.0, FontFamily::Proportional),
        text,
    );

    response.on_hover_cursor(CursorIcon::PointingHand)
}

// ── Navigation ───────────────────────────────────────────────────────────
pub fn nav_item(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 42.0), Sense::click());

    let fill = if selected {
        theme::accent_soft()
    } else if response.hovered() {
        theme::card_raised()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);

    if selected {
        let bar = Rect::from_min_size(
            Pos2::new(rect.left() + 1.0, rect.top() + 10.0),
            Vec2::new(3.0, rect.height() - 20.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), theme::accent());
    }

    ui.painter().text(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(14.0, FontFamily::Proportional),
        if selected {
            theme::text()
        } else {
            theme::muted()
        },
    );

    response.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn caps_label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.0)
            .color(theme::faint())
            .strong(),
    );
}

// ── Branding ─────────────────────────────────────────────────────────────
pub fn logo(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let radius = CornerRadius::same((size * 0.28) as u8);
    painter.rect_filled(rect, radius, theme::accent());
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::accent_hover().gamma_multiply(0.5)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "C",
        FontId::new(size * 0.52, FontFamily::Proportional),
        theme::ink(),
    );
}

// ── Layout primitives ────────────────────────────────────────────────────
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(
                RichText::new(title)
                    .size(26.0)
                    .strong()
                    .color(theme::text()),
            );
            ui.label(RichText::new(subtitle).size(13.0).color(theme::muted()));
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            right(ui);
        });
    });
}

pub fn pill(ui: &mut Ui, text: &str, fg: Color32) -> Response {
    let font = FontId::new(10.0, FontFamily::Proportional);
    let width = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font.clone(), fg))
        .size()
        .x
        + 18.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());

    ui.painter()
        .rect_filled(rect, CornerRadius::same(11), fg.gamma_multiply(0.14));
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, font, fg);

    response
}

pub fn status_color(status: crate::models::InvoiceStatus) -> Color32 {
    use crate::models::InvoiceStatus;
    match status {
        InvoiceStatus::Paid => theme::green(),
        InvoiceStatus::Pending => theme::amber(),
        InvoiceStatus::Overdue => theme::red(),
    }
}

pub fn status_pill(ui: &mut Ui, status: crate::models::InvoiceStatus) -> Response {
    pill(
        ui,
        status.label().to_uppercase().as_str(),
        status_color(status),
    )
}

pub fn stat_card(
    ui: &mut Ui,
    label: &str,
    value: &str,
    hint: &str,
    accent: Color32,
    hint_color: Color32,
) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(86.0);

        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.5, accent);
            ui.label(
                RichText::new(label)
                    .size(10.5)
                    .strong()
                    .color(theme::muted()),
            );
        });

        ui.add_space(5.0);
        let value_size = if value.chars().count() > 13 {
            22.0
        } else {
            27.0
        };
        ui.label(
            RichText::new(value)
                .size(value_size)
                .strong()
                .color(theme::text()),
        );
        ui.add_space(1.0);
        ui.label(RichText::new(hint).size(11.0).color(hint_color));
    });
}

/// A colored message strip used for errors, warnings and confirmations.
pub fn banner(ui: &mut Ui, message: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.10))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.0, color);
                ui.label(RichText::new(message).size(11.0).color(color));
            });
        });
}

pub fn dashed_rect(painter: &egui::Painter, rect: Rect, color: Color32, dash: f32, gap: f32) {
    dashed_line(painter, rect.left_top(), rect.right_top(), color, dash, gap);
    dashed_line(
        painter,
        rect.right_top(),
        rect.right_bottom(),
        color,
        dash,
        gap,
    );
    dashed_line(
        painter,
        rect.right_bottom(),
        rect.left_bottom(),
        color,
        dash,
        gap,
    );
    dashed_line(
        painter,
        rect.left_bottom(),
        rect.left_top(),
        color,
        dash,
        gap,
    );
}

fn dashed_line(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32, dash: f32, gap: f32) {
    let delta = to - from;
    let length = delta.length();
    if length <= f32::EPSILON {
        return;
    }
    let dir = delta / length;
    let stroke = Stroke::new(1.2, color);
    let mut t = 0.0;
    while t < length {
        let end = (t + dash).min(length);
        painter.line_segment([from + dir * t, from + dir * end], stroke);
        t += dash + gap;
    }
}
