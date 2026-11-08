use eframe::egui;
use egui::{Align, CornerRadius, Layout, Margin, RichText, Stroke, Vec2};

use crate::theme;
use crate::ui;

/// Shown when the local database cannot be opened. Returns `true` on retry.
pub fn show(ui: &mut egui::Ui, error: &str, path: &std::path::Path) -> bool {
    let mut retry = false;

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::bg()))
        .show(ui, |ui| {
            let canvas = ui.max_rect();
            ui.painter().circle_filled(
                egui::pos2(canvas.center().x, canvas.top() + 80.0),
                300.0,
                egui::Color32::from_rgba_unmultiplied(0xF8, 0x71, 0x71, 8),
            );

            ui.add_space(((canvas.height() - 480.0) * 0.5).max(24.0));
            ui.vertical_centered(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(560.0, 0.0),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui::logo(ui, 44.0);
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new("Your workspace couldn't open")
                                .size(24.0)
                                .strong()
                                .color(theme::text()),
                        );
                        ui.label(
                            RichText::new(
                                "Crustom could not access its local database. Your data has not been changed.",
                            )
                            .size(12.5)
                            .color(theme::muted()),
                        );
                        ui.add_space(20.0);

                        theme::card_frame()
                            .inner_margin(Margin::same(26))
                            .show(ui, |ui| {
                                ui.set_width(480.0);
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    egui::Frame::new()
                                        .fill(theme::field())
                                        .stroke(Stroke::new(1.0, theme::red().gamma_multiply(0.35)))
                                        .corner_radius(CornerRadius::same(8))
                                        .inner_margin(Margin::symmetric(12, 10))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(
                                                RichText::new(error)
                                                    .size(11.0)
                                                    .color(theme::red()),
                                            );
                                        });

                                    ui.add_space(12.0);
                                    ui.label(
                                        RichText::new("Database location")
                                            .size(10.5)
                                            .color(theme::faint()),
                                    );
                                    ui.label(
                                        RichText::new(path.display().to_string())
                                            .size(10.5)
                                            .color(theme::muted())
                                            .monospace(),
                                    );

                                    ui.add_space(16.0);
                                    if ui::primary_button(
                                        ui,
                                        "Retry",
                                        ui.available_width(),
                                    )
                                    .clicked()
                                    {
                                        retry = true;
                                    }

                                    ui.add_space(8.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(
                                                "Check folder permissions or available disk space, then try again.",
                                            )
                                            .size(10.0)
                                            .color(theme::faint()),
                                        );
                                    });
                                });
                            });
                    },
                );
            });
        });

    retry
}
