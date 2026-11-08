use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui;
use egui::{Align, Layout, Margin, RichText};

use crate::app::Screen;
use crate::db::Storage;
use crate::import::normalize;
use crate::models::{
    BASE_CURRENCY_KEY, DATE_FORMAT_KEY, DATE_FORMATS, TARGET_ANNUAL_KEY, TARGET_MONTHLY_KEY,
    TARGET_QUARTERLY_KEY, THEME_KEY, format_date,
};
use crate::sms::{self, FAST2SMS_API_KEY, SMS_ENABLED_KEY, SMS_METHOD_KEY};
use crate::theme;
use crate::ui;

#[derive(Default)]
pub struct SettingsState {
    loaded: bool,
    monthly: String,
    quarterly: String,
    annual: String,
    base_currency: String,
    date_format: String,
    theme: theme::ThemeMode,
    status: Option<(String, bool)>,
    confirm_reset: bool,
    sms_enabled: bool,
    sms_method: sms::SmsMethod,
    sms_api_key: String,
    sms_show_key: bool,
    sms_test_number: String,
    sms_test_job: Option<Receiver<Result<String, String>>>,
    sms_status: Option<(String, bool)>,
}

pub fn show(
    ui: &mut egui::Ui,
    username: &str,
    storage: &Storage,
    state: &mut SettingsState,
    screen: &mut Screen,
) {
    if !state.loaded {
        state.monthly = load_target(storage, TARGET_MONTHLY_KEY);
        state.quarterly = load_target(storage, TARGET_QUARTERLY_KEY);
        state.annual = load_target(storage, TARGET_ANNUAL_KEY);
        state.base_currency = storage.base_currency();
        state.date_format = storage.date_format();
        state.theme = theme::ThemeMode::from_setting(
            storage.get_setting(THEME_KEY).ok().flatten().as_deref(),
        );
        state.sms_enabled = sms::sms_enabled(storage);
        state.sms_method = sms::sms_method(storage);
        state.sms_api_key = storage
            .get_setting(FAST2SMS_API_KEY)
            .ok()
            .flatten()
            .unwrap_or_default();
        state.loaded = true;
    }

    let mut clear_sms_test = false;
    if let Some(receiver) = &state.sms_test_job {
        match receiver.try_recv() {
            Ok(Ok(request_id)) => {
                state.sms_status = Some((format!("Test SMS sent (request {request_id})."), true));
                clear_sms_test = true;
            }
            Ok(Err(error)) => {
                state.sms_status = Some((format!("Test SMS failed: {error}"), false));
                clear_sms_test = true;
            }
            Err(TryRecvError::Empty) => {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
            Err(TryRecvError::Disconnected) => {
                state.sms_status = Some((
                    "The test SMS request stopped unexpectedly.".to_owned(),
                    false,
                ));
                clear_sms_test = true;
            }
        }
    }
    if clear_sms_test {
        state.sms_test_job = None;
    }

    let invoice_count = storage.invoice_count().unwrap_or(0);
    let import_count = storage.import_batch_count().unwrap_or(0);
    let db_path = storage.path().display().to_string();

    let mut open_folder = false;
    let mut export_backup = false;
    let mut reset_confirmed = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui::page_header(ui, "Settings", "Workspace, targets and local data", |ui| {
                ui::pill(ui, "WORKSPACE · PHASE 7", theme::green());
            });

            ui.add_space(18.0);

            if let Some((message, success)) = &state.status {
                ui::banner(
                    ui,
                    message,
                    if *success { theme::green() } else { theme::red() },
                );
                ui.add_space(12.0);
            }

            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_heading(
                    ui,
                    "Account",
                    "Username-only access to this local workspace.",
                );
                ui.add_space(12.0);

                theme::inset_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    setting_row(
                        ui,
                        "Signed in as",
                        "This username identifies your local workspace",
                        |ui| {
                            ui.label(
                                RichText::new(username)
                                    .size(13.0)
                                    .strong()
                                    .color(theme::text()),
                            );
                            ui::pill(ui, "USERNAME ONLY", theme::green());
                        },
                    );
                    row_divider(ui);
                    setting_row(
                        ui,
                        "Access",
                        "No password is stored or required",
                        |ui| {
                            ui::pill(ui, "PASSWORDLESS", theme::green());
                        },
                    );
                    row_divider(ui);
                    setting_row(
                        ui,
                        "Session",
                        "Held in memory and cleared when you sign out",
                        |ui| {
                            ui::pill(ui, "LOCAL", theme::green());
                        },
                    );
                });
            });

            ui.add_space(16.0);

            ui.columns(2, |cols| {
                theme::card_frame().show(&mut cols[0], |ui| {
                    ui.set_width(ui.available_width());
                    card_heading(
                        ui,
                        "Revenue targets",
                        "The monthly target drives the progress chart.",
                    );
                    ui.add_space(12.0);

                    target_edit(
                        ui,
                        "Monthly",
                        "Progress is measured against this",
                        &mut state.monthly,
                    );
                    ui.add_space(8.0);
                    target_edit(
                        ui,
                        "Quarterly",
                        "Stored for quarterly reviews",
                        &mut state.quarterly,
                    );
                    ui.add_space(8.0);
                    target_edit(
                        ui,
                        "Annual",
                        "Stored for annual planning",
                        &mut state.annual,
                    );

                    ui.add_space(12.0);
                    let mut save = false;
                    ui.horizontal(|ui| {
                        if ui::secondary_button(ui, "Save targets", 132.0).clicked() {
                            save = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new("Applied to the dashboard currency")
                                    .size(10.0)
                                    .color(theme::faint()),
                            );
                        });
                    });

                    if save {
                        match save_targets(storage, state) {
                            Ok(()) => {
                                state.status = Some((
                                    "Targets saved. The dashboard is updated.".to_owned(),
                                    true,
                                ));
                            }
                            Err(error) => {
                                state.status = Some((error, false));
                            }
                        }
                    }

                    ui.add_space(10.0);
                    theme::inset_frame().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(
                                "Targets apply to the currency selected on the dashboard and are prorated by days for the chosen period. No FX conversion is performed.",
                            )
                            .size(10.5)
                            .color(theme::muted()),
                        );
                    });
                });

                theme::card_frame().show(&mut cols[1], |ui| {
                    ui.set_width(ui.available_width());
                    card_heading(
                        ui,
                        "Preferences",
                        "How billing data is presented across the app.",
                    );
                    ui.add_space(12.0);

                    let mut theme_changed = false;
                    setting_row(
                        ui,
                        "Theme",
                        "Applied across the entire application",
                        |ui| {
                            let previous = state.theme;
                            egui::ComboBox::from_id_salt("theme_mode")
                                .selected_text(state.theme.label())
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    for mode in theme::ThemeMode::ALL {
                                        ui.selectable_value(
                                            &mut state.theme,
                                            mode,
                                            mode.label(),
                                        );
                                    }
                                });
                            theme_changed = previous != state.theme;
                        },
                    );
                    if theme_changed {
                        theme::apply(ui.ctx(), state.theme);
                        match storage
                            .set_setting(THEME_KEY, state.theme.setting_value())
                        {
                            Ok(()) => {
                                state.status = Some((
                                    format!(
                                        "Theme set to {}.",
                                        state.theme.label()
                                    ),
                                    true,
                                ));
                            }
                            Err(error) => {
                                state.status = Some((
                                    format!("Could not save the theme: {error}"),
                                    false,
                                ));
                            }
                        }
                    }

                    row_divider(ui);

                    let mut currency_changed = false;
                    setting_row(
                        ui,
                        "Default import currency",
                        "Used when a source row has no currency",
                        |ui| {
                            let previous = state.base_currency.clone();
                            egui::ComboBox::from_id_salt("base_currency")
                                .selected_text(state.base_currency.clone())
                                .width(92.0)
                                .show_ui(ui, |ui| {
                                    for currency in [
                                        "INR", "USD", "EUR", "GBP", "JPY", "CHF", "AUD",
                                        "CAD",
                                    ] {
                                        ui.selectable_value(
                                            &mut state.base_currency,
                                            currency.to_owned(),
                                            currency,
                                        );
                                    }
                                });
                            currency_changed = previous != state.base_currency;
                        },
                    );
                    if currency_changed {
                        match storage.set_setting(
                            BASE_CURRENCY_KEY,
                            &state.base_currency,
                        ) {
                            Ok(()) => {
                                state.status = Some((
                                    format!(
                                        "Default import currency set to {}.",
                                        state.base_currency
                                    ),
                                    true,
                                ));
                            }
                            Err(error) => {
                                state.status = Some((
                                    format!("Could not save the currency: {error}"),
                                    false,
                                ));
                            }
                        }
                    }

                    row_divider(ui);

                    let mut date_changed = false;
                    setting_row(
                        ui,
                        "Date format",
                        "Used in the ledger and bill forms",
                        |ui| {
                            let previous = state.date_format.clone();
                            egui::ComboBox::from_id_salt("date_format")
                                .selected_text(state.date_format.clone())
                                .width(130.0)
                                .show_ui(ui, |ui| {
                                    for (label, _) in DATE_FORMATS {
                                        ui.selectable_value(
                                            &mut state.date_format,
                                            label.to_owned(),
                                            label,
                                        );
                                    }
                                });
                            date_changed = previous != state.date_format;
                        },
                    );
                    if date_changed {
                        match storage.set_setting(DATE_FORMAT_KEY, &state.date_format) {
                            Ok(()) => {
                                state.status = Some((
                                    format!(
                                        "Date format set to {}. Sample: {}",
                                        state.date_format,
                                        format_date(
                                            chrono::Local::now().date_naive(),
                                            &state.date_format
                                        )
                                    ),
                                    true,
                                ));
                            }
                            Err(error) => {
                                state.status = Some((
                                    format!("Could not save the date format: {error}"),
                                    false,
                                ));
                            }
                        }
                    }

                    ui.add_space(12.0);
                    theme::inset_frame().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(
                                "Theme changes apply immediately and are stored on this device.",
                            )
                            .size(10.5)
                            .color(theme::faint()),
                        );
                    });
                });
            });

            ui.add_space(16.0);

            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        card_heading(
                            ui,
                            "SMS notifications",
                            "Notify the bill customer when a status changes, automatically or from Messages.",
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if state.sms_enabled {
                            ui::pill(ui, "ON", theme::green());
                        } else {
                            ui::pill(ui, "OFF", theme::faint());
                        }
                    });
                });

                ui.add_space(12.0);
                ui.checkbox(
                    &mut state.sms_enabled,
                    RichText::new("Send an SMS when a bill status changes")
                        .size(11.5)
                        .color(theme::text()),
                );

                ui.add_space(10.0);
                ui.label(
                    RichText::new("Delivery method")
                        .size(11.0)
                        .color(theme::muted()),
                );
                ui.horizontal(|ui| {
                    for method in sms::SmsMethod::ALL {
                        ui.radio_value(
                            &mut state.sms_method,
                            method,
                            RichText::new(method.label())
                                .size(11.0)
                                .color(theme::text()),
                        );
                    }
                });
                ui.label(
                    RichText::new(state.sms_method.description())
                        .size(9.5)
                        .color(theme::faint()),
                );

                let mut save_sms = false;
                let mut test_sms = false;
                let mut test_link = false;

                if state.sms_method == sms::SmsMethod::Fast2Sms {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Fast2SMS API key")
                            .size(11.0)
                            .color(theme::muted()),
                    );
                    let key_ready = !state.sms_api_key.trim().is_empty();
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.sms_api_key)
                                .hint_text("Your Fast2SMS authorization key")
                                .password(!state.sms_show_key)
                                .desired_width(320.0)
                                .margin(Margin::symmetric(12, 9)),
                        );
                        ui.checkbox(
                            &mut state.sms_show_key,
                            RichText::new("Show")
                                .size(10.5)
                                .color(theme::faint()),
                        );
                        if !key_ready {
                            ui.label(
                                RichText::new("Required")
                                    .size(9.5)
                                    .color(theme::amber()),
                            );
                        }
                    });
                    ui.label(
                        RichText::new(
                            "Stored locally. The key is only sent to Fast2SMS when delivering a message.",
                        )
                        .size(9.5)
                        .color(theme::faint()),
                    );
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui::secondary_button(ui, "Save SMS settings", 152.0).clicked() {
                        save_sms = true;
                    }
                    if state.sms_method == sms::SmsMethod::Fast2Sms {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Test to")
                                .size(10.5)
                                .color(theme::faint()),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut state.sms_test_number)
                                .hint_text("+91 98765 43210")
                                .desired_width(152.0)
                                .margin(Margin::symmetric(10, 7)),
                        );
                        if ui::secondary_button(ui, "Send test SMS", 128.0).clicked() {
                            test_sms = true;
                        }
                        if state.sms_test_job.is_some() {
                            ui.add(egui::Spinner::new().size(16.0));
                        }
                    } else {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Test to")
                                .size(10.5)
                                .color(theme::faint()),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut state.sms_test_number)
                                .hint_text("+91 98765 43210")
                                .desired_width(152.0)
                                .margin(Margin::symmetric(10, 7)),
                        );
                        if ui::secondary_button(ui, "Open test message", 152.0).clicked() {
                            test_link = true;
                        }
                    }
                });

                if test_link {
                    if state.sms_test_number.trim().is_empty() {
                        state.sms_status = Some((
                            "Enter a test mobile number.".to_owned(),
                            false,
                        ));
                    } else {
                        match sms::open_messages_link(
                            state.sms_test_number.trim(),
                            &format!(
                                "{}: test message - SMS notifications are working.",
                                sms::SMS_HEADER
                            ),
                        ) {
                            Ok(()) => {
                                state.sms_status = Some((
                                    "Opened Messages with the test text. Press Send to deliver it."
                                        .to_owned(),
                                    true,
                                ));
                            }
                            Err(error) => {
                                state.sms_status = Some((
                                    format!("Could not open the test message: {error}"),
                                    false,
                                ));
                            }
                        }
                    }
                }

                if test_sms {
                    if state.sms_api_key.trim().is_empty() {
                        state.sms_status = Some((
                            "Enter a Fast2SMS API key before testing.".to_owned(),
                            false,
                        ));
                    } else if state.sms_test_number.trim().is_empty() {
                        state.sms_status = Some((
                            "Enter a test mobile number.".to_owned(),
                            false,
                        ));
                    } else if state.sms_test_job.is_none() {
                        let key = state.sms_api_key.trim().to_owned();
                        let number = state.sms_test_number.trim().to_owned();
                        let (sender, receiver) = std::sync::mpsc::channel();
                        state.sms_test_job = Some(receiver);
                        state.sms_status = None;
                        std::thread::spawn(move || {
                            let result = sms::send_test_sms(&key, &number);
                            let _ = sender.send(result);
                        });
                    }
                }

                if save_sms {
                    let needs_key = state.sms_enabled
                        && state.sms_method == sms::SmsMethod::Fast2Sms
                        && state.sms_api_key.trim().is_empty();

                    if needs_key {
                        state.sms_status = Some((
                            "Add a Fast2SMS API key before enabling automatic SMS."
                                .to_owned(),
                            false,
                        ));
                    } else {
                        let result = storage
                            .set_setting(
                                SMS_ENABLED_KEY,
                                if state.sms_enabled { "true" } else { "false" },
                            )
                            .and_then(|()| {
                                storage.set_setting(SMS_METHOD_KEY, state.sms_method.key())
                            })
                            .and_then(|()| {
                                storage.set_setting(
                                    FAST2SMS_API_KEY,
                                    state.sms_api_key.trim(),
                                )
                            });
                        match result {
                            Ok(()) => {
                                state.sms_status = Some((
                                    if state.sms_enabled {
                                        format!(
                                            "SMS notifications enabled via {}.",
                                            state.sms_method.label()
                                        )
                                    } else {
                                        "SMS notifications disabled.".to_owned()
                                    },
                                    true,
                                ));
                            }
                            Err(error) => {
                                state.sms_status = Some((
                                    format!("Could not save SMS settings: {error}"),
                                    false,
                                ));
                            }
                        }
                    }
                }

                if let Some((message, success)) = &state.sms_status {
                    ui.add_space(8.0);
                    ui::banner(
                        ui,
                        message,
                        if *success { theme::green() } else { theme::red() },
                    );
                }

                ui.add_space(10.0);
                let footer = match state.sms_method {
                    sms::SmsMethod::Fast2Sms => {
                        "Fast2SMS Quick SMS route · standard SMS charges apply · the bill must have a valid Indian mobile number."
                    }
                    sms::SmsMethod::MessagesLink => {
                        "Opens the macOS Messages app · an iPhone with Text Message Forwarding must be signed in · you press Send for each alert."
                    }
                };
                ui.label(
                    RichText::new(footer)
                        .size(9.5)
                        .color(theme::faint()),
                );
            });

            ui.add_space(16.0);

            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_heading(
                    ui,
                    "Developer settings",
                    "Integrations for advanced billing workflows.",
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("REST API import")
                                .size(12.5)
                                .strong()
                                .color(theme::text()),
                        );
                        ui.label(
                            RichText::new(
                                "Fetch JSON from an OData service or another HTTP endpoint, map its columns and import it into the ledger.",
                            )
                            .size(10.5)
                            .color(theme::muted()),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::secondary_button(ui, "Open REST API import", 168.0).clicked() {
                            *screen = Screen::Developer;
                        }
                    });
                });
            });

            ui.add_space(16.0);

            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_heading(
                    ui,
                    "Data & storage",
                    "Everything is stored locally in a single SQLite database.",
                );
                ui.add_space(12.0);

                theme::inset_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(
                                RichText::new("Database location")
                                    .size(12.5)
                                    .color(theme::text()),
                            );
                            ui.label(
                                RichText::new(&db_path)
                                    .size(10.5)
                                    .color(theme::muted())
                                    .monospace(),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui::pill(ui, "SQLITE · LOCAL", theme::green());
                        });
                    });
                    row_divider(ui);
                    setting_row(ui, "Bills stored", "Billing documents", |ui| {
                        ui.label(
                            RichText::new(invoice_count.to_string())
                                .size(13.0)
                                .strong()
                                .color(theme::text()),
                        );
                    });
                    row_divider(ui);
                    setting_row(ui, "Import batches", "Files processed", |ui| {
                        ui.label(
                            RichText::new(import_count.to_string())
                                .size(13.0)
                                .strong()
                                .color(theme::text()),
                        );
                    });
                });

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if ui::secondary_button(ui, "Open data folder", 130.0).clicked() {
                        open_folder = true;
                    }
                    ui.add_space(4.0);
                    if ui::secondary_button(ui, "Export backup (JSON)", 158.0).clicked() {
                        export_backup = true;
                    }
                    ui.add_space(4.0);
                    if ui::danger_button(ui, "Reset workspace", 138.0).clicked() {
                        state.confirm_reset = true;
                    }
                });

                if state.confirm_reset {
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(theme::red().gamma_multiply(0.08))
                        .stroke(egui::Stroke::new(
                            1.0,
                            theme::red().gamma_multiply(0.35),
                        ))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("Delete all billing data?")
                                    .size(13.0)
                                    .strong()
                                    .color(theme::red()),
                            );
                            ui.label(
                                RichText::new(
                                    "This removes every bill and import-history entry. Your account and settings are kept. This cannot be undone.",
                                )
                                .size(10.5)
                                .color(theme::muted()),
                            );
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui::danger_button(ui, "Delete billing data", 160.0).clicked() {
                                    reset_confirmed = true;
                                }
                                ui.add_space(6.0);
                                if ui::secondary_button(ui, "Cancel", 88.0).clicked() {
                                    state.confirm_reset = false;
                                }
                            });
                        });
                }

                ui.add_space(10.0);
                ui.label(
                    RichText::new(
                        "Backups contain bills, import history, people notes and settings. Account credentials are never included.",
                    )
                    .size(10.0)
                    .color(theme::faint()),
                );
            });

            ui.add_space(16.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        RichText::new("Crustom v0.14.0")
                            .size(11.5)
                            .strong()
                            .color(theme::text()),
                    );
                    ui.label(
                        RichText::new("billing intelligence · Rust, egui and SQLite")
                            .size(10.0)
                            .color(theme::faint()),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui::pill(ui, "PHASE 7 OF 7", theme::faint());
                });
            });

            ui.add_space(24.0);
        });

    if open_folder {
        match open_data_folder(storage.path()) {
            Ok(()) => {
                state.status = Some(("Opened the data folder.".to_owned(), true));
            }
            Err(error) => {
                state.status = Some((format!("Could not open the folder: {error}"), false));
            }
        }
    }

    if export_backup {
        match export_backup_json(storage, username) {
            Ok(Some(path)) => {
                state.status = Some((format!("Backup written to {}.", path.display()), true));
            }
            Ok(None) => {}
            Err(error) => {
                state.status = Some((format!("Could not create the backup: {error}"), false));
            }
        }
    }

    if reset_confirmed {
        state.confirm_reset = false;
        match storage.clear_workspace() {
            Ok(()) => {
                state.status = Some((
                    "Workspace cleared. All bills and import history were deleted.".to_owned(),
                    true,
                ));
            }
            Err(error) => {
                state.status = Some((format!("Could not clear the workspace: {error}"), false));
            }
        }
    }
}

fn card_heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.spacing_mut().item_spacing.y = 3.0;
    ui.label(
        RichText::new(title)
            .size(14.5)
            .strong()
            .color(theme::text()),
    );
    ui.label(RichText::new(subtitle).size(11.0).color(theme::muted()));
}

fn row_divider(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);
}

fn setting_row(ui: &mut egui::Ui, label: &str, detail: &str, trailing: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(label).size(12.5).color(theme::text()));
            ui.label(RichText::new(detail).size(10.5).color(theme::faint()));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            trailing(ui);
        });
    });
}

fn target_edit(ui: &mut egui::Ui, label: &str, hint: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(label).size(12.5).color(theme::text()));
            ui.label(RichText::new(hint).size(10.5).color(theme::faint()));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width(116.0)
                    .margin(Margin::symmetric(10, 8)),
            );
        });
    });
}

fn load_target(storage: &Storage, key: &str) -> String {
    storage
        .get_setting(key)
        .ok()
        .flatten()
        .and_then(|value| value.parse::<i64>().ok())
        .map(cents_to_input)
        .unwrap_or_else(|| "0.00".to_owned())
}

fn cents_to_input(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.unsigned_abs();
    format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
}

fn parse_target(input: &str, label: &str) -> Result<i64, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(0);
    }

    let cents =
        normalize::parse_amount(input).map_err(|error| format!("{label} target: {error}"))?;
    if cents < 0 {
        return Err(format!("{label} target cannot be negative."));
    }
    Ok(cents)
}

fn save_targets(storage: &Storage, state: &SettingsState) -> Result<(), String> {
    let monthly = parse_target(&state.monthly, "Monthly")?;
    let quarterly = parse_target(&state.quarterly, "Quarterly")?;
    let annual = parse_target(&state.annual, "Annual")?;

    storage
        .set_setting(TARGET_MONTHLY_KEY, &monthly.to_string())
        .map_err(|error| format!("Could not save the monthly target: {error}"))?;
    storage
        .set_setting(TARGET_QUARTERLY_KEY, &quarterly.to_string())
        .map_err(|error| format!("Could not save the quarterly target: {error}"))?;
    storage
        .set_setting(TARGET_ANNUAL_KEY, &annual.to_string())
        .map_err(|error| format!("Could not save the annual target: {error}"))?;

    Ok(())
}

fn open_data_folder(db_path: &Path) -> Result<(), String> {
    let folder = db_path.parent().unwrap_or(db_path);
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");

    command
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn export_backup_json(
    storage: &Storage,
    username: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    let content = build_backup(storage, username)?;
    let default_name = format!(
        "crustom-backup-{}.json",
        chrono::Local::now().format("%Y%m%d")
    );
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export Crustom backup")
        .set_file_name(default_name)
        .add_filter("JSON backup", &["json"])
        .save_file()
    else {
        return Ok(None);
    };

    std::fs::write(&path, content).map_err(|error| error.to_string())?;
    Ok(Some(path))
}

fn build_backup(storage: &Storage, username: &str) -> Result<String, String> {
    use serde_json::json;

    let invoices = storage.all_invoices().map_err(|error| error.to_string())?;
    let imports = storage
        .recent_imports(10_000)
        .map_err(|error| error.to_string())?;
    let settings = storage.all_settings().map_err(|error| error.to_string())?;
    let people = storage
        .all_person_notes()
        .map_err(|error| error.to_string())?;
    let extensions = storage
        .all_person_extensions()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|(customer, count)| (customer.to_lowercase(), count))
        .collect::<BTreeMap<_, _>>();

    let invoice_values: Vec<serde_json::Value> = invoices
        .iter()
        .map(|invoice| {
            json!({
                "invoice_no": invoice.invoice_no,
                "customer": invoice.customer,
                "customer_phone": invoice.customer_phone,
                "issue_date": invoice.issue_date.to_string(),
                "due_date": invoice.due_date.map(|date| date.to_string()),
                "amount_cents": invoice.amount_cents,
                "currency": invoice.currency,
                "status": invoice.status.as_str(),
                "source": invoice.source,
                "source_file": invoice.source_file,
                "imported_at": invoice.imported_at.to_rfc3339(),
            })
        })
        .collect();

    let import_values: Vec<serde_json::Value> = imports
        .iter()
        .map(|batch| {
            json!({
                "filename": batch.filename,
                "format": batch.format,
                "rows_total": batch.rows_total,
                "rows_imported": batch.rows_imported,
                "rows_skipped": batch.rows_skipped,
                "imported_at": batch.imported_at.to_rfc3339(),
            })
        })
        .collect();

    let settings_value: serde_json::Map<String, serde_json::Value> = settings
        .into_iter()
        .filter(|(key, _)| key != FAST2SMS_API_KEY)
        .map(|(key, value)| (key, serde_json::Value::String(value)))
        .collect();

    let people_values: Vec<serde_json::Value> = people
        .into_iter()
        .map(|(customer, notes)| {
            let extension_count = extensions
                .get(&customer.to_lowercase())
                .copied()
                .unwrap_or(0);
            json!({
                "customer": customer,
                "notes": notes,
                "extension_count": extension_count,
            })
        })
        .collect();

    let backup = json!({
        "app": "Crustom",
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "account": username,
        "invoices": invoice_values,
        "import_history": import_values,
        "people": people_values,
        "settings": settings_value,
    });

    serde_json::to_string_pretty(&backup).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_targets() {
        assert_eq!(parse_target("", "Monthly").unwrap(), 0);
        assert_eq!(parse_target("1,234.56", "Monthly").unwrap(), 123_456);
        assert_eq!(parse_target("1.234,56", "Monthly").unwrap(), 123_456);
        assert!(parse_target("-5", "Monthly").is_err());
        assert_eq!(cents_to_input(123_456), "1234.56");
        assert_eq!(cents_to_input(0), "0.00");
    }

    #[test]
    fn saves_all_targets() {
        let storage = Storage::open_in_memory().expect("database opens");
        let state = SettingsState {
            loaded: true,
            monthly: "1,234.56".to_owned(),
            quarterly: "5000".to_owned(),
            annual: "".to_owned(),
            ..Default::default()
        };

        save_targets(&storage, &state).expect("targets save");
        assert_eq!(
            storage.get_setting(TARGET_MONTHLY_KEY).unwrap().as_deref(),
            Some("123456")
        );
        assert_eq!(
            storage
                .get_setting(TARGET_QUARTERLY_KEY)
                .unwrap()
                .as_deref(),
            Some("500000")
        );
        assert_eq!(
            storage.get_setting(TARGET_ANNUAL_KEY).unwrap().as_deref(),
            Some("0")
        );
    }

    #[test]
    fn builds_a_backup_without_account_credentials() {
        let document = crate::import::load_sample().expect("sample loads");
        let mapping = crate::import::auto_mapping(document.headers());
        let plan = crate::import::build_plan(&document, &mapping, "USD").expect("plan builds");
        let storage = Storage::open_in_memory().expect("database opens");
        storage
            .insert_invoices(
                &plan.candidates,
                document.file_name(),
                "csv",
                plan.rows_total,
            )
            .expect("import succeeds");

        storage
            .set_setting(FAST2SMS_API_KEY, "super-secret-key")
            .unwrap();

        let invoice_id = storage.recent_invoices(1).unwrap()[0].id;
        storage.extend_invoice_due_date(invoice_id, 30).unwrap();

        let backup = build_backup(&storage, "ravi").expect("backup builds");
        assert!(backup.contains("\"invoices\""));
        assert!(backup.contains("\"account\": \"ravi\""));
        assert!(backup.contains("\"extension_count\": 1"));
        assert!(!backup.contains("password_hash"));
        assert!(!backup.contains("$argon2"));
        assert!(!backup.contains("super-secret-key"));
        assert!(!backup.contains("fast2sms_api_key"));
    }

    #[test]
    fn renders_settings_screen() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = SettingsState::default();
        let mut screen = Screen::Settings;
        egui::__run_test_ui(|ui| {
            show(ui, "ravi", &storage, &mut state, &mut screen);
        });
    }

    #[test]
    fn renders_reset_confirmation() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = SettingsState {
            loaded: true,
            confirm_reset: true,
            ..Default::default()
        };
        let mut screen = Screen::Settings;
        egui::__run_test_ui(|ui| {
            show(ui, "ravi", &storage, &mut state, &mut screen);
        });
    }
}
