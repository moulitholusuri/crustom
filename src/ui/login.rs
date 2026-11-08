//! Username-only setup and sign-in for the local workspace.

use eframe::egui;
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Vec2};

use crate::auth::{self, AuthMode};
use crate::db::Storage;
use crate::theme;
use crate::ui;

#[derive(Default)]
pub struct AuthState {
    pub username: String,
    pub error: Option<String>,
    pub focus_requested: bool,
}

impl AuthState {
    pub fn prepare_for_login(&mut self, username: String) {
        self.username = username;
        self.error = None;
        self.focus_requested = false;
    }
}

/// Show setup or username-only sign-in. Returns the signed-in username on success.
pub fn show(
    ui: &mut egui::Ui,
    mode: AuthMode,
    state: &mut AuthState,
    storage: &Storage,
) -> Option<String> {
    let is_setup = mode == AuthMode::Setup;
    let mut success = None;

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::bg()))
        .show(ui, |ui| {
            let canvas = ui.max_rect();
            let painter = ui.painter();

            painter.circle_filled(
                egui::pos2(canvas.center().x, canvas.top() + 30.0),
                330.0,
                egui::Color32::from_rgba_unmultiplied(0x4C, 0x8D, 0xFF, 10),
            );
            painter.circle_filled(
                egui::pos2(canvas.right() - 80.0, canvas.bottom() - 20.0),
                220.0,
                egui::Color32::from_rgba_unmultiplied(0x34, 0xD3, 0x99, 7),
            );

            let content_height = 500.0;
            ui.add_space(((canvas.height() - content_height) * 0.5).max(16.0));

            ui.vertical_centered(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(440.0, 0.0),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui::logo(ui, 46.0);
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new(if is_setup {
                                "Create your local account"
                            } else {
                                "Welcome back"
                            })
                            .size(25.0)
                            .strong()
                            .color(theme::text()),
                        );
                        ui.label(
                            RichText::new(if is_setup {
                                "Choose a username for this billing workspace."
                            } else {
                                "Enter your username to open the workspace."
                            })
                            .size(12.5)
                            .color(theme::muted()),
                        );
                        ui.add_space(20.0);

                        theme::card_frame()
                            .inner_margin(Margin::same(28))
                            .show(ui, |ui| {
                                ui.set_width(360.0);
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    ui.spacing_mut().item_spacing.y = 6.0;

                                    field_label(ui, "Username");
                                    let username = ui.add(
                                        egui::TextEdit::singleline(&mut state.username)
                                            .hint_text(if is_setup {
                                                "Choose a username"
                                            } else {
                                                "Your username"
                                            })
                                            .desired_width(f32::INFINITY)
                                            .margin(Margin::symmetric(12, 10)),
                                    );
                                    if is_setup {
                                        ui.label(
                                            RichText::new(
                                                "3–32 characters · letters, numbers, . _ -",
                                            )
                                            .size(10.0)
                                            .color(theme::faint()),
                                        );
                                    }

                                    ui.add_space(10.0);

                                    if let Some(error) = &state.error {
                                        error_banner(ui, error, theme::red());
                                        ui.add_space(8.0);
                                    }

                                    let has_username = !state.username.trim().is_empty();
                                    if ui::primary_button_enabled(
                                        ui,
                                        if is_setup { "Create account" } else { "Open workspace" },
                                        ui.available_width(),
                                        has_username,
                                    )
                                    .clicked()
                                    {
                                        submit(storage, is_setup, state, &mut success);
                                    }

                                    if ui.input(|input| input.key_pressed(egui::Key::Enter))
                                        && has_username
                                    {
                                        submit(storage, is_setup, state, &mut success);
                                    }

                                    ui.add_space(8.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(
                                                "No password required · stored only on this device",
                                            )
                                            .size(10.0)
                                            .color(theme::faint()),
                                        );
                                    });

                                    if !state.focus_requested {
                                        username.request_focus();
                                        state.focus_requested = true;
                                    }
                                });
                            });

                        ui.add_space(14.0);
                        ui::pill(ui, "USERNAME-ONLY ACCESS", theme::green());
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "This local workspace opens with your username. The database is protected by macOS account permissions.",
                            )
                            .size(10.5)
                            .color(theme::faint()),
                        );
                    },
                );
            });
        });

    success
}

fn submit(storage: &Storage, is_setup: bool, state: &mut AuthState, success: &mut Option<String>) {
    state.error = None;

    let result = if is_setup {
        auth::create_account(storage, &state.username)
    } else {
        auth::sign_in(storage, &state.username)
    };

    match result {
        Ok(username) => {
            state.username = username.clone();
            state.error = None;
            *success = Some(username);
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(theme::muted()));
}

fn error_banner(ui: &mut egui::Ui, message: &str, color: egui::Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.10))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.0, color);
                ui.label(RichText::new(message).size(10.5).color(color));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_setup_and_sign_in_screens() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = AuthState::default();

        egui::__run_test_ui(|ui| {
            show(ui, AuthMode::Setup, &mut state, &storage);
        });

        egui::__run_test_ui(|ui| {
            show(ui, AuthMode::Login, &mut state, &storage);
        });
    }

    #[test]
    fn signs_in_an_existing_username() {
        let storage = Storage::open_in_memory().expect("database opens");
        auth::create_account(&storage, "ravi").expect("account created");

        let mut state = AuthState {
            username: "RAVI".to_owned(),
            ..Default::default()
        };
        let mut success = None;
        submit(&storage, false, &mut state, &mut success);

        assert_eq!(success.as_deref(), Some("ravi"));
        assert!(state.error.is_none());
    }
}
