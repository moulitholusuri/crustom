use std::cmp::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui;
use egui::{
    Align, CornerRadius, Layout, Margin, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::app::Screen;
use crate::db::Storage;
use crate::models::{Invoice, InvoiceStatus, format_amount, format_date};
use crate::notes;
use crate::sms;
use crate::theme;
use crate::ui;
use crate::ui::entry::InvoiceForm;

#[derive(Default)]
pub struct InvoicesState {
    query: String,
    status: Option<InvoiceStatus>,
    sort: InvoiceSort,
    editing: Option<EditingInvoice>,
    confirm_delete: Option<DeleteTarget>,
    sms_compose: Option<SmsCompose>,
    notice: Option<(String, bool)>,
    sms_jobs: Vec<SmsJob>,
}

struct EditingInvoice {
    id: i64,
    original_status: InvoiceStatus,
    form: InvoiceForm,
}

struct DeleteTarget {
    id: i64,
    invoice_no: String,
}

struct SmsCompose {
    customer: String,
    invoice_no: String,
    phone: Option<String>,
    message: String,
    error: Option<String>,
}

impl SmsCompose {
    fn from_invoice(invoice: &Invoice, storage: &Storage) -> Self {
        let items = storage
            .person_notes(&invoice.customer)
            .ok()
            .flatten()
            .map(|notes| notes::parse(&notes))
            .unwrap_or_default();
        let items_text = notes::format_for_message(&items, &invoice.currency);

        Self {
            customer: invoice.customer.clone(),
            invoice_no: invoice.invoice_no.clone(),
            phone: invoice.customer_phone.clone(),
            message: sms::status_message(
                invoice.amount_cents,
                &invoice.currency,
                invoice.status,
                items_text.as_deref(),
            ),
            error: None,
        }
    }
}

struct StatusChange {
    id: i64,
    invoice_no: String,
    customer: String,
    customer_phone: Option<String>,
    amount_cents: i64,
    currency: String,
    new_status: InvoiceStatus,
}

struct SmsJob {
    receiver: Receiver<Result<String, String>>,
    phone: String,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum InvoiceSort {
    #[default]
    Newest,
    Oldest,
    AmountHigh,
    AmountLow,
    Customer,
    Status,
}

impl InvoiceSort {
    const ALL: [Self; 6] = [
        Self::Newest,
        Self::Oldest,
        Self::AmountHigh,
        Self::AmountLow,
        Self::Customer,
        Self::Status,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest first",
            Self::Oldest => "Oldest first",
            Self::AmountHigh => "Amount high → low",
            Self::AmountLow => "Amount low → high",
            Self::Customer => "Customer A–Z",
            Self::Status => "Status",
        }
    }
}

pub fn show(ui: &mut egui::Ui, screen: &mut Screen, storage: &Storage, state: &mut InvoicesState) {
    poll_sms_jobs(ui.ctx(), state);

    let date_format = storage.date_format();
    let mut export_requested = false;
    let mut export_txt_requested = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui::page_header(
                ui,
                "Details of the bills",
                "Every billing document in one searchable ledger",
                |ui| {
                    if ui::secondary_button(ui, "Import billing data", 168.0).clicked() {
                        *screen = Screen::Import;
                    }
                    ui.add_space(2.0);
                    if ui::secondary_button(ui, "Export CSV", 118.0).clicked() {
                        export_requested = true;
                    }
                    ui.add_space(2.0);
                    if ui::secondary_button(ui, "Export TXT", 112.0).clicked() {
                        export_txt_requested = true;
                    }
                    ui.add_space(2.0);
                    ui::pill(ui, "BILLING LEDGER", theme::green());
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

            let invoices = match storage.all_invoices() {
                Ok(invoices) => invoices,
                Err(error) => {
                    ui::banner(
                        ui,
                        &format!("Could not load bills: {error}"),
                        theme::red(),
                    );
                    return;
                }
            };

            let query = state.query.trim().to_lowercase();
            let mut filtered: Vec<&Invoice> = invoices
                .iter()
                .filter(|invoice| {
                    (query.is_empty()
                        || invoice.invoice_no.to_lowercase().contains(&query)
                        || invoice.customer.to_lowercase().contains(&query)
                        || invoice
                            .customer_phone
                            .as_ref()
                            .is_some_and(|phone| phone.to_lowercase().contains(&query)))
                        && state.status.is_none_or(|status| invoice.status == status)
                })
                .collect();

            filtered.sort_by(|left, right| compare(left, right, state.sort));
            let matching = filtered.len();
            let visible: Vec<&Invoice> = filtered.into_iter().take(500).collect();

            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.query)
                            .hint_text("Search bill serial number or customer…")
                            .desired_width(280.0)
                            .margin(Margin::symmetric(12, 9)),
                    );

                    egui::ComboBox::from_id_salt("bill_status_filter")
                        .selected_text(
                            state
                                .status
                                .map(InvoiceStatus::label)
                                .unwrap_or("All statuses"),
                        )
                        .width(118.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.status, None, "All statuses");
                            for status in [
                                InvoiceStatus::Paid,
                                InvoiceStatus::Pending,
                                InvoiceStatus::Overdue,
                            ] {
                                ui.selectable_value(
                                    &mut state.status,
                                    Some(status),
                                    status.label(),
                                );
                            }
                        });

                    egui::ComboBox::from_id_salt("bill_sort")
                        .selected_text(state.sort.label())
                        .width(136.0)
                        .show_ui(ui, |ui| {
                            for sort in InvoiceSort::ALL {
                                ui.selectable_value(&mut state.sort, sort, sort.label());
                            }
                        });

                    if (!state.query.is_empty() || state.status.is_some())
                        && ui
                            .link(RichText::new("Clear").size(10.5).color(theme::muted()))
                            .clicked()
                    {
                        state.query.clear();
                        state.status = None;
                    }
                });

                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let count = if matching == invoices.len() {
                            format!("{} ROWS", invoices.len())
                        } else {
                            format!("{} OF {} ROWS", matching, invoices.len())
                        };
                        ui.label(
                            RichText::new(count)
                                .size(10.0)
                                .strong()
                                .color(theme::faint()),
                        );
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(4.0);

                if visible.is_empty() {
                    empty_state(ui, screen, state, !invoices.is_empty());
                    return;
                }

                let mut edit_request = None;
                let mut delete_request = None;
                let mut status_request = None;
                let mut sms_request = None;
                let mut extend_request = None;

                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::Grid::new("bill_table")
                            .num_columns(7)
                            .striped(true)
                            .spacing([18.0, 9.0])
                            .show(ui, |ui| {
                                for (index, label) in [
                                    "BILL SERIAL NO.", "CUSTOMER", "ISSUED", "DUE", "AMOUNT", "STATUS",
                                    "ACTIONS",
                                ]
                                .iter()
                                .enumerate()
                                {
                                    if index == 4 {
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(*label)
                                                        .size(9.5)
                                                        .strong()
                                                        .color(theme::faint()),
                                                );
                                            },
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new(*label)
                                                .size(9.5)
                                                .strong()
                                                .color(theme::faint()),
                                        );
                                    }
                                }
                                ui.end_row();

                                for invoice in &visible {
                                    let number = ui.label(
                                        RichText::new(&invoice.invoice_no)
                                            .size(10.5)
                                            .monospace()
                                            .color(theme::text()),
                                    );
                                    if let Some(source) = &invoice.source_file {
                                        number.on_hover_text(format!(
                                            "Imported from {source}\nSaved {}",
                                            invoice
                                                .imported_at
                                                .with_timezone(&chrono::Local)
                                                .format("%d %b %Y · %H:%M")
                                        ));
                                    } else {
                                        number.on_hover_text(format!("Source: {}", invoice.source));
                                    }

                                    ui.vertical(|ui| {
                                        let customer_name = truncate(&invoice.customer, 28);
                                        let customer_label = ui.label(
                                            RichText::new(customer_name)
                                                .size(11.0)
                                                .color(theme::text()),
                                        );
                                        if invoice.customer.chars().count() > 28
                                            || invoice.customer_phone.is_some()
                                        {
                                            let mut tooltip = invoice.customer.clone();
                                            if let Some(phone) = &invoice.customer_phone {
                                                tooltip.push('\n');
                                                tooltip.push_str(&format_phone(phone));
                                            }
                                            customer_label.on_hover_text(tooltip);
                                        }
                                        if let Some(phone) = &invoice.customer_phone {
                                            ui.label(
                                                RichText::new(format_phone(phone))
                                                    .size(9.0)
                                                    .color(theme::faint()),
                                            );
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format_date(
                                            invoice.issue_date,
                                            &date_format,
                                        ))
                                        .size(10.5)
                                        .color(theme::muted()),
                                    );
                                    ui.label(
                                        RichText::new(
                                            invoice
                                                .due_date
                                                .map(|date| format_date(date, &date_format))
                                                .unwrap_or_else(|| "—".to_owned()),
                                        )
                                        .size(10.5)
                                        .color(
                                            if invoice.status == InvoiceStatus::Overdue {
                                                theme::red()
                                            } else {
                                                theme::muted()
                                            },
                                        ),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format_amount(
                                                invoice.amount_cents,
                                                &invoice.currency,
                                            ))
                                            .size(11.0)
                                            .strong()
                                            .color(theme::text()),
                                        );
                                    });
                                    let mut chosen_status = invoice.status;
                                    egui::ComboBox::from_id_salt(format!(
                                        "bill_status_{}",
                                        invoice.id
                                    ))
                                    .selected_text(
                                        RichText::new(chosen_status.label())
                                            .size(10.0)
                                            .strong()
                                            .color(ui::status_color(chosen_status)),
                                    )
                                    .width(98.0)
                                    .show_ui(ui, |ui| {
                                        for status in [
                                            InvoiceStatus::Pending,
                                            InvoiceStatus::Paid,
                                            InvoiceStatus::Overdue,
                                        ] {
                                            ui.selectable_value(
                                                &mut chosen_status,
                                                status,
                                                status.label(),
                                            );
                                        }
                                    });
                                    if chosen_status != invoice.status {
                                        status_request = Some(StatusChange {
                                            id: invoice.id,
                                            invoice_no: invoice.invoice_no.clone(),
                                            customer: invoice.customer.clone(),
                                            customer_phone: invoice.customer_phone.clone(),
                                            amount_cents: invoice.amount_cents,
                                            currency: invoice.currency.clone(),
                                            new_status: chosen_status,
                                        });
                                    }

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        if ui
                                            .link(
                                                RichText::new("SMS")
                                                    .size(9.5)
                                                    .color(theme::green()),
                                            )
                                            .on_hover_text(
                                                "Compose an SMS for this customer in Messages",
                                            )
                                            .clicked()
                                        {
                                            sms_request = Some(SmsCompose::from_invoice(invoice, storage));
                                        }
                                        if ui
                                            .link(
                                                RichText::new("Extend")
                                                    .size(9.5)
                                                    .color(theme::amber()),
                                            )
                                            .on_hover_text(
                                                "Add 30 days to the due date and count an extension for this person",
                                            )
                                            .clicked()
                                        {
                                            extend_request = Some(invoice.id);
                                        }
                                        if ui
                                            .link(
                                                RichText::new("Edit")
                                                    .size(9.5)
                                                    .color(theme::accent()),
                                            )
                                            .clicked()
                                        {
                                            edit_request = Some(EditingInvoice {
                                                id: invoice.id,
                                                original_status: invoice.status,
                                                form: InvoiceForm::from_invoice(invoice),
                                            });
                                        }
                                        if ui
                                            .link(
                                                RichText::new("Delete")
                                                    .size(9.5)
                                                    .color(theme::red()),
                                            )
                                            .clicked()
                                        {
                                            delete_request = Some(DeleteTarget {
                                                id: invoice.id,
                                                invoice_no: invoice.invoice_no.clone(),
                                            });
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });

                if let Some(editing) = edit_request {
                    state.editing = Some(editing);
                    state.notice = None;
                }
                if let Some(target) = delete_request {
                    state.confirm_delete = Some(target);
                    state.notice = None;
                }
                if let Some(compose) = sms_request {
                    state.sms_compose = Some(compose);
                    state.notice = None;
                }
                if let Some(id) = extend_request {
                    match storage.extend_invoice_due_date(id, 30) {
                        Ok(Some((_customer, new_due))) => {
                            state.notice = Some((
                                format!(
                                    "Due date extended by 30 days to {}.",
                                    format_date(new_due, &date_format)
                                ),
                                true,
                            ));
                        }
                        Ok(None) => {
                            state.notice =
                                Some(("The bill no longer exists.".to_owned(), false));
                        }
                        Err(error) => {
                            state.notice = Some((
                                format!("Could not extend the due date: {error}"),
                                false,
                            ));
                        }
                    }
                }
                if let Some(change) = status_request {
                    match storage.set_invoice_status(change.id, change.new_status) {
                        Ok(true) => {
                            state.notice = Some((
                                format!(
                                    "Bill {} is now {}.",
                                    change.invoice_no,
                                    change.new_status.label()
                                ),
                                true,
                            ));
                            queue_status_sms(
                                storage,
                                state,
                                &change.customer,
                                change.customer_phone.as_deref(),
                                change.amount_cents,
                                &change.currency,
                                change.new_status,
                            );
                        }
                        Ok(false) => {
                            state.notice =
                                Some(("The bill no longer exists.".to_owned(), false));
                        }
                        Err(error) => {
                            state.notice =
                                Some((format!("Could not change the status: {error}"), false));
                        }
                    }
                }

                if matching > visible.len() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "Showing the first {} of {matching} matching bills.",
                            visible.len()
                        ))
                        .size(10.0)
                        .color(theme::faint()),
                    );
                }
            });

            ui.add_space(24.0);
        });

    edit_modal(ui.ctx(), state, storage);
    delete_modal(ui.ctx(), state, storage);
    sms_modal(ui.ctx(), state);

    if export_requested {
        export_invoices(storage, state);
    }
    if export_txt_requested {
        export_invoices_text(storage, state, &date_format);
    }
}

fn poll_sms_jobs(ctx: &egui::Context, state: &mut InvoicesState) {
    if state.sms_jobs.is_empty() {
        return;
    }

    let mut pending = false;
    let mut result_notice = None;
    let mut keep = Vec::new();

    for job in state.sms_jobs.drain(..) {
        match job.receiver.try_recv() {
            Ok(Ok(_)) => {
                result_notice = Some((format!("SMS sent to {}.", job.phone), true));
            }
            Ok(Err(error)) => {
                result_notice = Some((format!("SMS to {} failed: {error}", job.phone), false));
            }
            Err(TryRecvError::Empty) => {
                pending = true;
                keep.push(job);
            }
            Err(TryRecvError::Disconnected) => {
                result_notice = Some(("SMS delivery stopped unexpectedly.".to_owned(), false));
            }
        }
    }

    state.sms_jobs = keep;
    if let Some(notice) = result_notice {
        state.notice = Some(notice);
    }
    if pending {
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn queue_status_sms(
    storage: &Storage,
    state: &mut InvoicesState,
    customer: &str,
    phone: Option<&str>,
    amount_cents: i64,
    currency: &str,
    status: InvoiceStatus,
) {
    if !sms::sms_enabled(storage) {
        return;
    }

    let Some(phone) = phone else {
        state.notice = Some((
            "Status saved, but this bill has no customer phone number, so no SMS was sent."
                .to_owned(),
            false,
        ));
        return;
    };

    let phone = phone.to_owned();
    let items = storage
        .person_notes(customer)
        .ok()
        .flatten()
        .map(|notes| notes::parse(&notes))
        .unwrap_or_default();
    let items_text = notes::format_for_message(&items, currency);
    let message = sms::status_message(amount_cents, currency, status, items_text.as_deref());
    let display_phone = format_phone(&phone);

    if sms::sms_method(storage) == sms::SmsMethod::MessagesLink {
        match sms::open_messages_link(&phone, &message) {
            Ok(()) => {
                state.notice = Some((
                    format!(
                        "Status saved. Opened Messages for {display_phone} — review the text and press Send."
                    ),
                    true,
                ));
            }
            Err(error) => {
                state.notice = Some((
                    format!("Status saved, but the SMS link could not be opened: {error}"),
                    false,
                ));
            }
        }
        return;
    }

    let Some(config) = sms::load_config(storage) else {
        state.notice = Some((
            "Status saved, but Fast2SMS is not configured. Add an API key in Settings.".to_owned(),
            false,
        ));
        return;
    };

    let (sender, receiver) = std::sync::mpsc::channel();
    state.sms_jobs.push(SmsJob {
        receiver,
        phone: display_phone.clone(),
    });
    state.notice = Some((
        format!("Status saved. Sending SMS to {display_phone}…"),
        true,
    ));

    std::thread::spawn(move || {
        let result = sms::send_status_sms(&config, &phone, &message);
        let _ = sender.send(result);
    });
}

fn edit_modal(ctx: &egui::Context, state: &mut InvoicesState, storage: &Storage) {
    let Some(mut editing) = state.editing.take() else {
        return;
    };

    let mut open = true;
    let mut save = false;
    let mut cancel = false;

    egui::Window::new("Edit bill")
        .collapsible(false)
        .resizable(false)
        .default_width(620.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!("Editing {}", editing.form.invoice_no))
                    .size(11.0)
                    .color(theme::muted()),
            );
            ui.add_space(8.0);
            save = editing.form.show(ui, "edit_bill", "Save changes");
            ui.add_space(4.0);
            if ui::secondary_button(ui, "Cancel", 100.0).clicked() {
                cancel = true;
            }
        });

    if !open || cancel {
        return;
    }

    if save {
        match editing.form.build() {
            Ok(invoice) => match storage.update_invoice(editing.id, &invoice) {
                Ok(()) => {
                    let status_changed = invoice.status != editing.original_status;
                    state.notice = Some(("Bill updated.".to_owned(), true));
                    if status_changed {
                        queue_status_sms(
                            storage,
                            state,
                            &invoice.customer,
                            invoice.customer_phone.as_deref(),
                            invoice.amount_cents,
                            &invoice.currency,
                            invoice.status,
                        );
                    }
                    return;
                }
                Err(error) => {
                    editing.form.error = Some(format!("Could not update the bill: {error}"));
                }
            },
            Err(error) => editing.form.error = Some(error),
        }
    }

    state.editing = Some(editing);
}

fn delete_modal(ctx: &egui::Context, state: &mut InvoicesState, storage: &Storage) {
    let Some(target) = state.confirm_delete.take() else {
        return;
    };

    let mut open = true;
    let mut delete = false;
    let mut cancel = false;

    egui::Window::new("Delete bill")
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!(
                    "Delete bill {} from the ledger?",
                    target.invoice_no
                ))
                .size(12.5)
                .color(theme::text()),
            );
            ui.label(
                RichText::new("This cannot be undone.")
                    .size(10.5)
                    .color(theme::muted()),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui::danger_button(ui, "Delete bill", 138.0).clicked() {
                    delete = true;
                }
                ui.add_space(6.0);
                if ui::secondary_button(ui, "Cancel", 88.0).clicked() {
                    cancel = true;
                }
            });
        });

    if !open || cancel {
        return;
    }

    if delete {
        match storage.delete_invoice(target.id) {
            Ok(true) => {
                state.notice = Some(("Bill deleted.".to_owned(), true));
            }
            Ok(false) => {
                state.notice = Some(("The bill no longer exists.".to_owned(), false));
            }
            Err(error) => {
                state.notice = Some((format!("Could not delete the bill: {error}"), false));
            }
        }
        return;
    }

    state.confirm_delete = Some(target);
}

fn sms_modal(ctx: &egui::Context, state: &mut InvoicesState) {
    let Some(mut compose) = state.sms_compose.take() else {
        return;
    };

    let mut open = true;
    let mut send = false;
    let mut cancel = false;

    egui::Window::new("Send SMS")
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!("{}  ·  {}", compose.customer, compose.invoice_no))
                    .size(12.5)
                    .strong()
                    .color(theme::text()),
            );
            ui.add_space(4.0);

            match &compose.phone {
                Some(phone) => {
                    ui.label(
                        RichText::new(format!("To: {}", format_phone(phone)))
                            .size(11.0)
                            .color(theme::muted()),
                    );
                }
                None => {
                    ui.label(
                        RichText::new(
                            "No customer phone number on file. Add one with Edit before sending.",
                        )
                        .size(10.5)
                        .color(theme::red()),
                    );
                }
            }

            ui.add_space(10.0);
            ui.label(RichText::new("Message").size(11.0).color(theme::muted()));
            ui.add(
                egui::TextEdit::multiline(&mut compose.message)
                    .desired_width(ui.available_width())
                    .desired_rows(4)
                    .margin(Margin::symmetric(12, 10)),
            );
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Opens the Mac Messages app. Press Send there to deliver.")
                        .size(9.5)
                        .color(theme::faint()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} characters", compose.message.chars().count()))
                            .size(9.0)
                            .color(theme::faint()),
                    );
                });
            });

            if let Some(error) = &compose.error {
                ui.add_space(8.0);
                ui::banner(ui, error, theme::red());
            }

            ui.add_space(12.0);
            let can_send = compose.phone.is_some() && !compose.message.trim().is_empty();
            ui.horizontal(|ui| {
                if ui::primary_button_enabled(ui, "Send", 110.0, can_send).clicked() {
                    send = true;
                }
                ui.add_space(6.0);
                if ui::secondary_button(ui, "Cancel", 88.0).clicked() {
                    cancel = true;
                }
            });
        });

    if !open || cancel {
        return;
    }

    if send {
        let Some(phone) = &compose.phone else {
            return;
        };
        match sms::open_messages_link(phone, compose.message.trim()) {
            Ok(()) => {
                state.notice = Some((
                    format!(
                        "Opened Messages for {}. Press Send there to deliver the SMS.",
                        format_phone(phone)
                    ),
                    true,
                ));
                return;
            }
            Err(error) => {
                compose.error = Some(format!("Could not open Messages: {error}"));
            }
        }
    }

    state.sms_compose = Some(compose);
}

fn export_invoices(storage: &Storage, state: &mut InvoicesState) {
    let invoices = match storage.all_invoices() {
        Ok(invoices) => invoices,
        Err(error) => {
            state.notice = Some((format!("Could not load bills: {error}"), false));
            return;
        }
    };

    if invoices.is_empty() {
        state.notice = Some(("There are no bills to export.".to_owned(), false));
        return;
    }

    let Some(path) = rfd::FileDialog::new()
        .set_title("Export bills")
        .set_file_name("crustom-bills.csv")
        .add_filter("CSV", &["csv"])
        .save_file()
    else {
        return;
    };

    match write_csv(&path, &invoices) {
        Ok(()) => {
            state.notice = Some((
                format!("Exported {} bills to {}.", invoices.len(), path.display()),
                true,
            ));
        }
        Err(error) => {
            state.notice = Some((format!("Could not export bills: {error}"), false));
        }
    }
}

fn export_invoices_text(storage: &Storage, state: &mut InvoicesState, date_format: &str) {
    let invoices = match storage.all_invoices() {
        Ok(invoices) => invoices,
        Err(error) => {
            state.notice = Some((format!("Could not load bills: {error}"), false));
            return;
        }
    };

    if invoices.is_empty() {
        state.notice = Some(("There are no bills to export.".to_owned(), false));
        return;
    }

    let Some(path) = rfd::FileDialog::new()
        .set_title("Export bills as text")
        .set_file_name("crustom-bills.txt")
        .add_filter("Text", &["txt"])
        .save_file()
    else {
        return;
    };

    match write_text(&path, &invoices, date_format) {
        Ok(()) => {
            state.notice = Some((
                format!("Exported {} bills to {}.", invoices.len(), path.display()),
                true,
            ));
        }
        Err(error) => {
            state.notice = Some((format!("Could not export bills: {error}"), false));
        }
    }
}

fn write_text(
    path: &std::path::Path,
    invoices: &[Invoice],
    date_format: &str,
) -> Result<(), String> {
    use std::fmt::Write as _;

    let mut text = String::new();
    let _ = writeln!(text, "CRUSTOM BILLING EXPORT");
    let _ = writeln!(
        text,
        "Generated: {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );
    let _ = writeln!(text, "Bills: {}", invoices.len());
    let _ = writeln!(text);

    for (index, invoice) in invoices.iter().enumerate() {
        let _ = writeln!(
            text,
            "============================================================"
        );
        let _ = writeln!(text, "Bill {} of {}", index + 1, invoices.len());
        let _ = writeln!(text, "Serial number of the bill: {}", invoice.invoice_no);
        let _ = writeln!(text, "Customer: {}", invoice.customer);
        if let Some(phone) = &invoice.customer_phone {
            let _ = writeln!(text, "Phone: {}", phone);
        }
        let _ = writeln!(
            text,
            "Issued: {}",
            format_date(invoice.issue_date, date_format)
        );
        let _ = writeln!(
            text,
            "Due: {}",
            invoice
                .due_date
                .map(|date| format_date(date, date_format))
                .unwrap_or_else(|| "Not set".to_owned())
        );
        let _ = writeln!(
            text,
            "Amount: {}",
            format_amount(invoice.amount_cents, &invoice.currency)
        );
        let _ = writeln!(text, "Status: {}", invoice.status.label());
        let _ = writeln!(text);
    }

    std::fs::write(path, text).map_err(|error| error.to_string())
}

fn write_csv(path: &std::path::Path, invoices: &[Invoice]) -> Result<(), String> {
    let mut writer = csv::Writer::from_path(path).map_err(|error| error.to_string())?;
    writer
        .write_record([
            "bill_serial_no",
            "customer",
            "customer_phone",
            "issue_date",
            "due_date",
            "amount",
            "currency",
            "status",
            "source",
            "source_file",
            "imported_at",
        ])
        .map_err(|error| error.to_string())?;

    for invoice in invoices {
        writer
            .write_record([
                invoice.invoice_no.as_str(),
                invoice.customer.as_str(),
                invoice.customer_phone.as_deref().unwrap_or(""),
                &invoice.issue_date.to_string(),
                &invoice
                    .due_date
                    .map(|date| date.to_string())
                    .unwrap_or_default(),
                &decimal_amount(invoice.amount_cents),
                invoice.currency.as_str(),
                invoice.status.as_str(),
                invoice.source.as_str(),
                invoice.source_file.as_deref().unwrap_or(""),
                &invoice.imported_at.to_rfc3339(),
            ])
            .map_err(|error| error.to_string())?;
    }

    writer.flush().map_err(|error| error.to_string())
}

fn decimal_amount(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.unsigned_abs();
    format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
}

fn compare(left: &Invoice, right: &Invoice, sort: InvoiceSort) -> Ordering {
    match sort {
        InvoiceSort::Newest => right
            .issue_date
            .cmp(&left.issue_date)
            .then_with(|| right.id.cmp(&left.id)),
        InvoiceSort::Oldest => left
            .issue_date
            .cmp(&right.issue_date)
            .then_with(|| left.id.cmp(&right.id)),
        InvoiceSort::AmountHigh => right.amount_cents.cmp(&left.amount_cents),
        InvoiceSort::AmountLow => left.amount_cents.cmp(&right.amount_cents),
        InvoiceSort::Customer => left
            .customer
            .to_lowercase()
            .cmp(&right.customer.to_lowercase()),
        InvoiceSort::Status => status_rank(left.status).cmp(&status_rank(right.status)),
    }
}

fn status_rank(status: InvoiceStatus) -> u8 {
    match status {
        InvoiceStatus::Overdue => 0,
        InvoiceStatus::Pending => 1,
        InvoiceStatus::Paid => 2,
    }
}

fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_owned();
    }
    let mut result: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

fn format_phone(phone: &str) -> String {
    if let Some(national) = phone.strip_prefix("+91")
        && national.len() == 10
    {
        return format!("+91 {} {}", &national[..5], &national[5..]);
    }
    phone.to_owned()
}

fn empty_state(
    ui: &mut egui::Ui,
    screen: &mut Screen,
    state: &mut InvoicesState,
    has_invoices: bool,
) {
    theme::inset_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);

            let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(48.0, 58.0), Sense::hover());
            paint_receipt(ui.painter(), icon_rect);

            ui.add_space(12.0);
            if has_invoices {
                ui.label(
                    RichText::new("No bills match")
                        .size(14.0)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new("Try a different search or clear the status filter.")
                        .size(11.0)
                        .color(theme::muted()),
                );
                ui.add_space(14.0);
                if ui::secondary_button(ui, "Clear filters", 138.0).clicked() {
                    state.query.clear();
                    state.status = None;
                }
            } else {
                ui.label(
                    RichText::new("No bills yet")
                        .size(14.0)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(
                        "Import a billing file or add one manually from the Overview page. Duplicate documents are skipped automatically.",
                    )
                    .size(11.0)
                    .color(theme::muted()),
                );
                ui.add_space(14.0);
                if ui::secondary_button(ui, "Go to importer", 138.0).clicked() {
                    *screen = Screen::Import;
                }
            }
            ui.add_space(24.0);
        });
    });
}

fn paint_receipt(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::same(10), theme::card_raised());
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );

    let left = rect.left() + 12.0;
    let right = rect.right() - 12.0;
    for (index, y) in [16.0, 26.0, 36.0].iter().enumerate() {
        let end = if index == 2 {
            left + (right - left) * 0.55
        } else {
            right
        };
        painter.line_segment(
            [
                Pos2::new(left, rect.top() + y),
                Pos2::new(end, rect.top() + y),
            ],
            Stroke::new(
                1.5,
                if index == 0 {
                    theme::accent().gamma_multiply(0.8)
                } else {
                    theme::border()
                },
            ),
        );
    }

    painter.circle_filled(
        Pos2::new(rect.right() - 12.0, rect.bottom() - 12.0),
        3.0,
        theme::green().gamma_multiply(0.9),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;

    fn seeded_storage() -> Storage {
        let document = import::load_sample().expect("sample loads");
        let mapping = import::auto_mapping(document.headers());
        let plan = import::build_plan(&document, &mapping, "USD").expect("plan builds");
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
    fn renders_ledger_with_imported_rows() {
        let storage = seeded_storage();
        let mut state = InvoicesState::default();
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }

    #[test]
    fn renders_edit_and_delete_modals() {
        let storage = seeded_storage();
        let invoice = storage.recent_invoices(1).expect("bills load").remove(0);
        let mut state = InvoicesState {
            editing: Some(EditingInvoice {
                id: invoice.id,
                original_status: invoice.status,
                form: InvoiceForm::from_invoice(&invoice),
            }),
            confirm_delete: Some(DeleteTarget {
                id: invoice.id,
                invoice_no: invoice.invoice_no.clone(),
            }),
            ..Default::default()
        };
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }

    #[test]
    fn builds_and_renders_the_sms_composer() {
        let storage = seeded_storage();
        let invoice = storage.recent_invoices(1).expect("bills load").remove(0);
        storage
            .set_person_notes(&invoice.customer, "Software licence | 1,000")
            .unwrap();
        let compose = SmsCompose::from_invoice(&invoice, &storage);

        assert!(compose.message.starts_with("CRSTM: "));
        assert!(!compose.message.contains(&invoice.invoice_no));
        assert!(compose.message.contains("Items:"));
        assert!(compose.message.contains("Software licence"));
        assert_eq!(compose.invoice_no, invoice.invoice_no);
        assert_eq!(compose.phone, invoice.customer_phone);

        let mut state = InvoicesState {
            sms_compose: Some(compose),
            ..Default::default()
        };
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }

    #[test]
    fn sorts_invoices() {
        let storage = seeded_storage();
        let invoices = storage.all_invoices().expect("bills load");
        let mut sorted = invoices.iter().collect::<Vec<_>>();
        sorted.sort_by(|left, right| compare(left, right, InvoiceSort::AmountHigh));
        assert!(sorted[0].amount_cents >= sorted[1].amount_cents);
    }

    #[test]
    fn exports_csv() {
        let storage = seeded_storage();
        let invoices = storage.all_invoices().expect("bills load");
        let path = std::env::temp_dir().join(format!("crustom-bills-{}.csv", std::process::id()));
        write_csv(&path, &invoices).expect("CSV writes");
        let text = std::fs::read_to_string(&path).expect("CSV reads");
        assert!(text.starts_with("bill_serial_no,customer"));
        assert!(text.lines().count() > 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn exports_text_bills() {
        let storage = seeded_storage();
        let invoices = storage.all_invoices().expect("bills load");
        let path = std::env::temp_dir().join(format!("crustom-bills-{}.txt", std::process::id()));
        write_text(&path, &invoices, "YYYY-MM-DD").expect("text writes");
        let text = std::fs::read_to_string(&path).expect("text reads");

        assert!(text.contains("CRUSTOM BILLING EXPORT"));
        assert!(text.contains(&invoices[0].invoice_no));
        assert!(text.contains("Customer:"));
        assert!(text.contains("Status:"));
        let _ = std::fs::remove_file(path);
    }
}
