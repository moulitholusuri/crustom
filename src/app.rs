use std::path::PathBuf;

use eframe::egui;
use egui::{Align, Align2, FontFamily, FontId, Layout, Margin, RichText, Sense, Stroke, Vec2};

use crate::auth::AuthMode;
use crate::db::{self, Storage};
use crate::theme;
use crate::ui;
use crate::ui::login::AuthState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Import,
    Invoices,
    People,
    Sms,
    Settings,
    Developer,
}

pub struct CrustomApp {
    screen: Screen,
    signed_in: bool,
    username: Option<String>,
    auth_mode: AuthMode,
    auth: AuthState,
    storage: Option<Storage>,
    startup_error: Option<String>,
    db_path: PathBuf,
    import_state: ui::import::ImportState,
    rest_state: ui::import::ImportState,
    invoices_state: ui::invoices::InvoicesState,
    people_state: ui::people::PeopleState,
    sms_state: ui::sms::SmsState,
    dashboard_state: ui::dashboard::DashboardState,
    settings_state: ui::settings::SettingsState,
}

impl CrustomApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx, theme::ThemeMode::Dark);

        let db_path = db::default_path();
        let mut app = Self {
            screen: Screen::Dashboard,
            signed_in: false,
            username: None,
            auth_mode: AuthMode::Login,
            auth: AuthState::default(),
            storage: None,
            startup_error: None,
            db_path,
            import_state: ui::import::ImportState::default(),
            rest_state: ui::import::ImportState::default(),
            invoices_state: ui::invoices::InvoicesState::default(),
            people_state: ui::people::PeopleState::default(),
            sms_state: ui::sms::SmsState::default(),
            dashboard_state: ui::dashboard::DashboardState::default(),
            settings_state: ui::settings::SettingsState::default(),
        };
        app.boot();
        if let Some(storage) = &app.storage {
            let mode = theme::ThemeMode::from_setting(
                storage
                    .get_setting(crate::models::THEME_KEY)
                    .ok()
                    .flatten()
                    .as_deref(),
            );
            theme::apply(&cc.egui_ctx, mode);
        }
        app
    }

    /// Open the database and decide between first-run setup and sign-in.
    fn boot(&mut self) {
        self.storage = None;
        self.startup_error = None;

        let storage = match Storage::open(&self.db_path) {
            Ok(storage) => storage,
            Err(error) => {
                self.startup_error = Some(format!("Could not open the local database: {error}"));
                return;
            }
        };

        match storage.user_count() {
            Ok(0) => {
                self.auth_mode = AuthMode::Setup;
                self.auth.username.clear();
                self.auth.focus_requested = false;
            }
            Ok(_) => {
                self.auth_mode = AuthMode::Login;
                if self.auth.username.is_empty()
                    && let Ok(Some(username)) = storage.get_setting("last_username")
                {
                    self.auth.username = username;
                }
            }
            Err(error) => {
                self.startup_error = Some(format!("Could not read the local account: {error}"));
                return;
            }
        }

        self.storage = Some(storage);
    }
}

impl eframe::App for CrustomApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(error) = self.startup_error.clone() {
            if ui::fatal::show(ui, &error, &self.db_path) {
                self.boot();
            }
            return;
        }

        if !self.signed_in {
            let signed_in = {
                let Some(storage) = self.storage.as_ref() else {
                    return;
                };
                ui::login::show(ui, self.auth_mode, &mut self.auth, storage)
            };

            if let Some(username) = signed_in {
                self.signed_in = true;
                self.username = Some(username);
                self.screen = Screen::Dashboard;
                self.auth_mode = AuthMode::Login;
                self.auth.error = None;
                self.auth.focus_requested = false;
            }
            return;
        }

        let mut screen = self.screen;
        let mut sign_out = false;
        let current_user = self
            .username
            .clone()
            .unwrap_or_else(|| "Account".to_owned());

        ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num1) {
                screen = Screen::Dashboard;
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num2) {
                screen = Screen::Import;
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num3) {
                screen = Screen::Invoices;
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num4) {
                screen = Screen::People;
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num5) {
                screen = Screen::Sms;
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num6) {
                screen = Screen::Settings;
            }

            if input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::L,
            ) {
                sign_out = true;
            }
        });

        egui::Panel::left("sidebar")
            .exact_size(248.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::sidebar())
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 20,
                        bottom: 16,
                    }),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui::logo(ui, 38.0);
                    ui.add_space(2.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(
                            RichText::new("Crustom")
                                .size(18.0)
                                .strong()
                                .color(theme::text()),
                        );
                        ui.label(
                            RichText::new("BILLING INTELLIGENCE")
                                .size(8.5)
                                .strong()
                                .color(theme::faint()),
                        );
                    });
                });

                ui.add_space(30.0);
                ui::caps_label(ui, "Workspace");
                ui.add_space(8.0);

                let nav = [
                    (Screen::Dashboard, "Overview"),
                    (Screen::Import, "Import data"),
                    (Screen::Invoices, "Details of the bills"),
                    (Screen::People, "People"),
                    (Screen::Sms, "Messages"),
                    (Screen::Settings, "Settings"),
                ];

                ui.spacing_mut().item_spacing.y = 3.0;
                for (target, label) in nav {
                    let active = screen == target
                        || (target == Screen::Settings && screen == Screen::Developer);
                    if ui::nav_item(ui, label, active).clicked() {
                        screen = target;
                    }
                }

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.label(
                        RichText::new("v0.13.0  ·  Phase 7")
                            .size(10.5)
                            .color(theme::faint()),
                    );
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    theme::inset_frame().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let initial = current_user
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string())
                                .unwrap_or_else(|| "?".to_owned());
                            let (avatar, _) =
                                ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                            ui.painter()
                                .circle_filled(avatar.center(), 15.0, theme::accent_soft());
                            ui.painter().circle_stroke(
                                avatar.center(),
                                15.0,
                                Stroke::new(1.0, theme::accent().gamma_multiply(0.35)),
                            );
                            ui.painter().text(
                                avatar.center(),
                                Align2::CENTER_CENTER,
                                initial,
                                FontId::new(13.0, FontFamily::Proportional),
                                theme::accent(),
                            );

                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(
                                    RichText::new(&current_user)
                                        .size(12.0)
                                        .strong()
                                        .color(theme::text()),
                                );
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    let (dot, _) =
                                        ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
                                    ui.painter()
                                        .circle_filled(dot.center(), 2.5, theme::green());
                                    ui.label(
                                        RichText::new("Local account")
                                            .size(10.0)
                                            .color(theme::faint()),
                                    );
                                });
                            });
                        });
                    });

                    ui.add_space(10.0);
                    if ui::nav_item(ui, "Sign out", false).clicked() {
                        sign_out = true;
                    }
                    ui.spacing_mut().item_spacing.y = 10.0;
                });
            });

        let storage = self.storage.as_ref();
        let import_state = &mut self.import_state;
        let rest_state = &mut self.rest_state;
        let invoices_state = &mut self.invoices_state;
        let people_state = &mut self.people_state;
        let sms_state = &mut self.sms_state;
        let dashboard_state = &mut self.dashboard_state;
        let settings_state = &mut self.settings_state;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::bg())
                    .inner_margin(Margin::same(28)),
            )
            .show(ui, |ui| match screen {
                Screen::Dashboard => {
                    if let Some(storage) = storage {
                        ui::dashboard::show(ui, &mut screen, storage, dashboard_state);
                    }
                }
                Screen::Import => {
                    if let Some(storage) = storage {
                        ui::import::show(ui, import_state, storage, &mut screen);
                    }
                }
                Screen::Invoices => {
                    if let Some(storage) = storage {
                        ui::invoices::show(ui, &mut screen, storage, invoices_state);
                    }
                }
                Screen::People => {
                    if let Some(storage) = storage {
                        ui::people::show(ui, &mut screen, storage, people_state);
                    }
                }
                Screen::Sms => {
                    if let Some(storage) = storage {
                        ui::sms::show(ui, &mut screen, storage, sms_state);
                    }
                }
                Screen::Settings => {
                    if let Some(storage) = storage {
                        ui::settings::show(ui, &current_user, storage, settings_state, &mut screen);
                    }
                }
                Screen::Developer => {
                    if let Some(storage) = storage {
                        ui::import::show_rest(ui, rest_state, storage, &mut screen);
                    }
                }
            });

        self.screen = screen;
        if sign_out {
            self.signed_in = false;
            self.screen = Screen::Dashboard;
            self.username = None;
            self.auth_mode = AuthMode::Login;
            self.auth.prepare_for_login(current_user);
            self.import_state = ui::import::ImportState::default();
            self.rest_state = ui::import::ImportState::default();
            self.invoices_state = ui::invoices::InvoicesState::default();
            self.people_state = ui::people::PeopleState::default();
            self.sms_state = ui::sms::SmsState::default();
            self.dashboard_state = ui::dashboard::DashboardState::default();
            self.settings_state = ui::settings::SettingsState::default();
        }
    }
}
