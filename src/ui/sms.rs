//! Dedicated SMS composer with editable pending/paid/due templates.

use eframe::egui;
use egui::{Align, Layout, Margin, RichText};

use crate::app::Screen;
use crate::db::Storage;
use crate::models::{Invoice, InvoiceStatus, format_amount};
use crate::notes;
use crate::sms;
use crate::theme;
use crate::ui;

pub struct SmsState {
    query: String,
    selected: Option<i64>,
    loaded_selection: Option<i64>,
    body: String,
    include_items: bool,
    templates: sms::SmsTemplates,
    editing_templates: bool,
    loaded: bool,
    notice: Option<(String, bool)>,
}

impl Default for SmsState {
    fn default() -> Self {
        Self {
            query: String::new(),
            selected: None,
            loaded_selection: None,
            body: String::new(),
            include_items: true,
            templates: sms::SmsTemplates::default(),
            editing_templates: false,
            loaded: false,
            notice: None,
        }
    }
}

pub fn show(ui: &mut egui::Ui, screen: &mut Screen, storage: &Storage, state: &mut SmsState) {
    if !state.loaded {
        state.templates = sms::load_templates(storage);
        state.loaded = true;
    }

    let invoices = match storage.all_invoices() {
        Ok(invoices) => invoices,
        Err(error) => {
            ui::banner(ui, &format!("Could not load bills: {error}"), theme::red());
            return;
        }
    };
    let date_format = storage.date_format();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui::page_header(
                ui,
                "Messages",
                "Send an itemised SMS through the Mac Messages app",
                |ui| {
                    ui::pill(ui, "CRSTM · MESSAGES", theme::green());
                },
            );

            ui.add_space(18.0);

            if let Some((message, success)) = &state.notice {
                ui::banner(
                    ui,
                    message,
                    if *success {
                        theme::green()
                    } else {
                        theme::red()
                    },
                );
                ui.add_space(12.0);
            }

            let recipients: Vec<&Invoice> = invoices
                .iter()
                .filter(|invoice| invoice.customer_phone.is_some())
                .collect();

            if recipients.is_empty() {
                empty_state(ui, screen);
                return;
            }

            let query = state.query.trim().to_lowercase();
            let visible: Vec<&Invoice> = recipients
                .iter()
                .copied()
                .filter(|invoice| {
                    query.is_empty()
                        || invoice.customer.to_lowercase().contains(&query)
                        || invoice.invoice_no.to_lowercase().contains(&query)
                        || invoice
                            .customer_phone
                            .as_deref()
                            .is_some_and(|phone| phone.contains(&query))
                })
                .collect();

            if state
                .selected
                .is_none_or(|selected| !visible.iter().any(|invoice| invoice.id == selected))
            {
                state.selected = visible.first().map(|invoice| invoice.id);
                state.loaded_selection = None;
            }

            let selected = state
                .selected
                .and_then(|id| visible.iter().find(|invoice| invoice.id == id).copied());

            if let Some(invoice) = selected
                && state.loaded_selection != Some(invoice.id)
            {
                state.body = sms::render_template(
                    state.templates.template_for(invoice.status),
                    invoice,
                    &date_format,
                );
                state.loaded_selection = Some(invoice.id);
                state.notice = None;
            }

            let total_width = ui.available_width();
            let gap = 16.0;
            let left_width = (total_width * 0.30).clamp(240.0, 310.0);
            let right_width = (total_width - left_width - gap).max(320.0);

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(left_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        theme::card_frame().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("Recipients")
                                    .size(14.5)
                                    .strong()
                                    .color(theme::text()),
                            );
                            ui.label(
                                RichText::new("Bills with a customer phone number.")
                                    .size(10.0)
                                    .color(theme::muted()),
                            );
                            ui.add_space(10.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut state.query)
                                    .hint_text("Search customer, bill or phone…")
                                    .desired_width(f32::INFINITY)
                                    .margin(Margin::symmetric(10, 7)),
                            );
                            ui.add_space(8.0);

                            egui::ScrollArea::vertical()
                                .max_height(520.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    if visible.is_empty() {
                                        ui.label(
                                            RichText::new("No recipient matches that search.")
                                                .size(10.5)
                                                .color(theme::faint()),
                                        );
                                        return;
                                    }

                                    for invoice in &visible {
                                        let is_selected = state.selected == Some(invoice.id);
                                        let title = format!(
                                            "{}  ·  {}",
                                            invoice.customer,
                                            format_amount(invoice.amount_cents, &invoice.currency)
                                        );
                                        if ui
                                            .selectable_label(
                                                is_selected,
                                                RichText::new(title).size(11.5).color(
                                                    if is_selected {
                                                        theme::text()
                                                    } else {
                                                        theme::muted()
                                                    },
                                                ),
                                            )
                                            .clicked()
                                        {
                                            state.selected = Some(invoice.id);
                                        }
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {} · {}",
                                                invoice.invoice_no,
                                                format_phone(
                                                    invoice.customer_phone.as_deref().unwrap_or("")
                                                ),
                                                invoice.status.label()
                                            ))
                                            .size(9.0)
                                            .color(theme::faint()),
                                        );
                                        ui.add_space(5.0);
                                    }
                                });
                        });
                    },
                );

                ui.add_space(gap);

                ui.allocate_ui_with_layout(
                    egui::Vec2::new(right_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        if let Some(invoice) = selected {
                            composer(ui, state, storage, invoice, &date_format);
                        } else {
                            theme::card_frame().show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.vertical_centered(|ui| {
                                    ui.add_space(26.0);
                                    ui.label(
                                        RichText::new("Select a recipient")
                                            .size(14.0)
                                            .strong()
                                            .color(theme::text()),
                                    );
                                    ui.add_space(26.0);
                                });
                            });
                        }
                    },
                );
            });

            ui.add_space(24.0);
        });
}

fn composer(
    ui: &mut egui::Ui,
    state: &mut SmsState,
    storage: &Storage,
    invoice: &Invoice,
    date_format: &str,
) {
    let items = storage
        .person_notes(&invoice.customer)
        .ok()
        .flatten()
        .map(|notes| notes::parse(&notes))
        .unwrap_or_default();
    let items_text = notes::format_for_message(&items, &invoice.currency);

    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&invoice.customer)
                        .size(16.0)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {} · {}",
                        invoice.invoice_no,
                        format_phone(invoice.customer_phone.as_deref().unwrap_or("")),
                        format_amount(invoice.amount_cents, &invoice.currency)
                    ))
                    .size(10.5)
                    .color(theme::muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui::status_pill(ui, invoice.status);
            });
        });

        ui.add_space(12.0);

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Template").size(10.5).color(theme::faint()));
            for (status, label) in [
                (InvoiceStatus::Pending, "Pending"),
                (InvoiceStatus::Paid, "Paid"),
                (InvoiceStatus::Overdue, "Due"),
            ] {
                if ui::secondary_button(ui, label, 82.0).clicked() {
                    state.body = sms::render_template(
                        state.templates.template_for(status),
                        invoice,
                        date_format,
                    );
                    state.notice = None;
                }
            }
            if ui::secondary_button(
                ui,
                if state.editing_templates {
                    "Close templates"
                } else {
                    "Edit templates"
                },
                142.0,
            )
            .clicked()
            {
                state.editing_templates = !state.editing_templates;
            }
        });

        if state.editing_templates {
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Predefined texts · use {customer}, {amount}, {status} and {due} placeholders.",
                )
                .size(9.5)
                .color(theme::faint()),
            );
            ui.add_space(6.0);
            template_field(ui, "Pending", &mut state.templates.pending);
            template_field(ui, "Paid", &mut state.templates.paid);
            template_field(ui, "Due", &mut state.templates.due);
            if ui::secondary_button(ui, "Save templates", 132.0).clicked() {
                match sms::save_templates(storage, &state.templates) {
                    Ok(()) => {
                        state.notice = Some(("Predefined SMS texts saved.".to_owned(), true));
                    }
                    Err(error) => {
                        state.notice =
                            Some((format!("Could not save predefined texts: {error}"), false));
                    }
                }
            }
        }

        ui.add_space(12.0);
        ui.label(
            RichText::new("Message")
                .size(12.5)
                .strong()
                .color(theme::text()),
        );
        ui.horizontal(|ui| {
            ui::pill(ui, sms::SMS_HEADER, theme::accent());
            ui.label(
                RichText::new("Added automatically to every outgoing message.")
                    .size(9.5)
                    .color(theme::faint()),
            );
        });
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::multiline(&mut state.body)
                .hint_text("Write the message body…")
                .desired_width(f32::INFINITY)
                .desired_rows(6)
                .margin(Margin::symmetric(12, 10)),
        );

        ui.add_space(8.0);
        ui.checkbox(
            &mut state.include_items,
            RichText::new("Append saved items and prices as a numbered list")
                .size(10.5)
                .color(theme::text()),
        );

        let items_for_message = if state.include_items {
            items_text.as_deref()
        } else {
            None
        };
        let preview = sms::compose_message(&state.body, items_for_message);

        ui.add_space(10.0);
        ui.label(
            RichText::new("Preview")
                .size(10.0)
                .strong()
                .color(theme::faint()),
        );
        theme::inset_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(RichText::new(&preview).size(10.5).color(theme::text()));
                });
        });

        ui.add_space(10.0);
        let can_send = !state.body.trim().is_empty();
        ui.horizontal(|ui| {
            if ui::primary_button_enabled(ui, "Send", 136.0, can_send).clicked()
                && let Some(phone) = &invoice.customer_phone
            {
                match sms::open_messages_link(phone, &preview) {
                    Ok(()) => {
                        state.notice = Some((
                            format!(
                                "Opened Messages for {}. Press Send there to deliver the SMS.",
                                format_phone(phone)
                            ),
                            true,
                        ));
                    }
                    Err(error) => {
                        state.notice = Some((format!("Could not open Messages: {error}"), false));
                    }
                }
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Opens the Mac Messages app; you press Send there.")
                    .size(9.5)
                    .color(theme::faint()),
            );
        });
    });
}

fn template_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).size(10.5).color(theme::muted()));
    ui.add(
        egui::TextEdit::multiline(value)
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .margin(Margin::symmetric(10, 8)),
    );
    ui.add_space(4.0);
}

fn format_phone(phone: &str) -> String {
    if let Some(national) = phone.strip_prefix("+91")
        && national.len() == 10
    {
        return format!("+91 {} {}", &national[..5], &national[5..]);
    }
    phone.to_owned()
}

fn empty_state(ui: &mut egui::Ui, screen: &mut Screen) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.add_space(26.0);
            ui.label(
                RichText::new("No bills with a phone number")
                    .size(17.0)
                    .strong()
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(
                    "Add an Indian mobile number to a bill with Edit, then return here to send an SMS.",
                )
                .size(11.0)
                .color(theme::muted()),
            );
            ui.add_space(14.0);
            if ui::primary_button(ui, "Open bills", 152.0).clicked() {
                *screen = Screen::Invoices;
            }
            ui.add_space(26.0);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;

    fn seeded_storage() -> Storage {
        let document = import::load_sample().expect("sample loads");
        let mapping = import::auto_mapping(document.headers());
        let plan = import::build_plan(&document, &mapping, "INR").expect("plan builds");
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
    }

    #[test]
    fn renders_messages_tab() {
        let storage = seeded_storage();
        let mut state = SmsState::default();
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }

    #[test]
    fn renders_messages_composer_for_a_phone_number() {
        let document = import::load_sample().expect("sample loads");
        let mapping = import::auto_mapping(document.headers());
        let plan = import::build_plan(&document, &mapping, "INR").expect("plan builds");
        let storage = Storage::open_in_memory().expect("database opens");

        let mut invoice = plan.candidates[0].clone();
        invoice.customer_phone = Some("+919876543210".to_owned());
        invoice.source = "manual".to_owned();
        invoice.source_file = None;
        storage.insert_manual_invoice(&invoice).unwrap();
        storage
            .set_person_notes(
                &invoice.customer,
                "Software licence | 125000\nSupport | 25000",
            )
            .unwrap();

        let mut state = SmsState::default();
        let mut screen = Screen::Dashboard;
        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });

        assert!(state.selected.is_some());
        assert!(state.body.contains("Dear"));
        assert!(state.body.contains(&invoice.customer));
    }

    #[test]
    fn renders_empty_messages_tab() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = SmsState::default();
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }
}
