//! Shared invoice form for manual entry and ledger edits.

use chrono::Utc;
use eframe::egui;
use egui::{Align, Layout, Margin, RichText};

use crate::import::normalize;
use crate::models::{DEFAULT_CURRENCY, Invoice, InvoiceStatus, NewInvoice};
use crate::{theme, ui};

#[derive(Clone)]
pub struct InvoiceForm {
    pub invoice_no: String,
    pub customer: String,
    pub customer_phone: String,
    pub issue_date: String,
    pub due_date: String,
    pub amount: String,
    pub currency: String,
    pub status: InvoiceStatus,
    pub error: Option<String>,
    phone_required: bool,
}

impl InvoiceForm {
    pub fn new(default_currency: &str) -> Self {
        Self {
            invoice_no: String::new(),
            customer: String::new(),
            customer_phone: String::new(),
            issue_date: Utc::now().date_naive().to_string(),
            due_date: String::new(),
            amount: String::new(),
            currency: default_currency.to_owned(),
            status: InvoiceStatus::Pending,
            error: None,
            phone_required: true,
        }
    }

    pub fn from_invoice(invoice: &Invoice) -> Self {
        Self {
            invoice_no: invoice.invoice_no.clone(),
            customer: invoice.customer.clone(),
            customer_phone: invoice.customer_phone.clone().unwrap_or_default(),
            issue_date: invoice.issue_date.to_string(),
            due_date: invoice
                .due_date
                .map(|date| date.to_string())
                .unwrap_or_default(),
            amount: cents_to_input(invoice.amount_cents),
            currency: invoice.currency.clone(),
            status: invoice.status,
            error: None,
            phone_required: invoice.source == "manual" || invoice.customer_phone.is_some(),
        }
    }

    pub fn build(&self) -> Result<NewInvoice, String> {
        let invoice_no = self.invoice_no.trim();
        if invoice_no.is_empty() {
            return Err("Serial number of the bill is required.".to_owned());
        }
        let customer = self.customer.trim();
        if customer.is_empty() {
            return Err("Customer is required.".to_owned());
        }

        let issue_date =
            normalize::parse_date(&self.issue_date).map_err(|error| format!("Issue {error}"))?;
        let due_date = if self.due_date.trim().is_empty() {
            None
        } else {
            Some(normalize::parse_date(&self.due_date).map_err(|error| format!("Due {error}"))?)
        };

        let amount_cents = if self.amount.trim().is_empty() {
            return Err("Amount is required.".to_owned());
        } else {
            normalize::parse_amount(&self.amount)?
        };

        let currency = normalize::parse_currency(&self.currency, DEFAULT_CURRENCY)?;
        let customer_phone = parse_indian_phone(&self.customer_phone, self.phone_required)?;
        let fingerprint = normalize::invoice_fingerprint(
            invoice_no,
            customer,
            issue_date,
            amount_cents,
            &currency,
        );

        Ok(NewInvoice {
            invoice_no: invoice_no.to_owned(),
            customer: customer.to_owned(),
            customer_phone,
            issue_date,
            due_date,
            amount_cents,
            currency,
            status: self.status,
            source: "manual".to_owned(),
            source_file: None,
            imported_at: Utc::now(),
            fingerprint,
        })
    }

    /// Draw the form fields and return true when the submit button was pressed.
    pub fn show(&mut self, ui: &mut egui::Ui, id_salt: &str, submit_label: &str) -> bool {
        let mut submitted = false;

        egui::Grid::new(format!("{id_salt}_bill_grid"))
            .num_columns(2)
            .spacing([22.0, 10.0])
            .show(ui, |ui| {
                field(
                    ui,
                    "Serial number of the bill",
                    &mut self.invoice_no,
                    "e.g. 90000025",
                    220.0,
                );
                field(ui, "Customer", &mut self.customer, "Customer name", 260.0);
                ui.end_row();

                field(
                    ui,
                    if self.phone_required {
                        "Phone number (India) *"
                    } else {
                        "Phone number (India)"
                    },
                    &mut self.customer_phone,
                    "Indian mobile · +91 98765 43210",
                    190.0,
                );
                ui.label("");
                ui.end_row();

                field(ui, "Issue date", &mut self.issue_date, "YYYY-MM-DD", 160.0);
                field(
                    ui,
                    "Due date",
                    &mut self.due_date,
                    "Optional · YYYY-MM-DD",
                    160.0,
                );
                ui.end_row();

                field(ui, "Amount", &mut self.amount, "e.g. 1,234.56", 160.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Currency").size(10.5).color(theme::muted()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.currency)
                            .hint_text("INR")
                            .desired_width(90.0)
                            .margin(Margin::symmetric(10, 8)),
                    );
                });
                ui.end_row();

                ui.vertical(|ui| {
                    ui.label(RichText::new("Status").size(10.5).color(theme::muted()));
                    egui::ComboBox::from_id_salt(format!("{id_salt}_status"))
                        .selected_text(self.status.label())
                        .width(148.0)
                        .show_ui(ui, |ui| {
                            for status in [
                                InvoiceStatus::Pending,
                                InvoiceStatus::Paid,
                                InvoiceStatus::Overdue,
                            ] {
                                ui.selectable_value(&mut self.status, status, status.label());
                            }
                        });
                });
                ui.label("");
                ui.end_row();
            });

        if let Some(error) = &self.error {
            ui.add_space(8.0);
            ui::banner(ui, error, theme::red());
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ready = !self.invoice_no.trim().is_empty()
                    && !self.customer.trim().is_empty()
                    && !self.issue_date.trim().is_empty()
                    && !self.amount.trim().is_empty()
                    && (!self.phone_required || !self.customer_phone.trim().is_empty());
                if ui::primary_button_enabled(ui, submit_label, 164.0, ready).clicked() {
                    submitted = true;
                }
            });
        });

        submitted
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, width: f32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).size(10.5).color(theme::muted()));
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .desired_width(width)
                .margin(Margin::symmetric(10, 8)),
        );
    });
}

fn cents_to_input(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.unsigned_abs();
    format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
}

fn parse_indian_phone(input: &str, required: bool) -> Result<Option<String>, String> {
    let raw = input.trim();
    if raw.is_empty() {
        if required {
            return Err("Enter an Indian mobile number.".to_owned());
        }
        return Ok(None);
    }

    if raw.chars().any(|character| {
        !(character.is_ascii_digit()
            || character.is_whitespace()
            || matches!(character, '+' | '-' | '(' | ')'))
    }) {
        return Err(
            "Enter a valid Indian mobile number (10 digits starting 6–9, with optional +91)."
                .to_owned(),
        );
    }

    let digits: String = raw
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();

    let national = if digits.len() == 10 {
        digits
    } else if digits.len() == 11 && digits.starts_with('0') {
        digits[1..].to_owned()
    } else if digits.len() == 12 && digits.starts_with("91") {
        digits[2..].to_owned()
    } else {
        return Err(
            "Enter a valid Indian mobile number (10 digits starting 6–9, with optional +91)."
                .to_owned(),
        );
    };

    if !national
        .chars()
        .next()
        .is_some_and(|first| matches!(first, '6'..='9'))
    {
        return Err("Indian mobile numbers must start with 6, 7, 8 or 9.".to_owned());
    }

    Ok(Some(format!("+91{national}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn builds_valid_manual_invoices() {
        let mut form = InvoiceForm::new("EUR");
        form.invoice_no = "MAN-1".to_owned();
        form.customer = "Acme".to_owned();
        form.customer_phone = "+91 98765 43210".to_owned();
        form.issue_date = "2026-01-15".to_owned();
        form.due_date = "15.02.2026".to_owned();
        form.amount = "1.234,56".to_owned();

        let invoice = form.build().expect("form builds");
        assert_eq!(invoice.amount_cents, 123_456);
        assert_eq!(invoice.currency, "EUR");
        assert_eq!(invoice.customer_phone.as_deref(), Some("+919876543210"));
        assert_eq!(
            invoice.due_date,
            Some(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap())
        );
        assert_eq!(invoice.source, "manual");
    }

    #[test]
    fn rejects_incomplete_manual_invoices() {
        let mut form = InvoiceForm::new("USD");
        assert!(form.build().is_err());
        form.invoice_no = "MAN-2".to_owned();
        form.customer = "Acme".to_owned();
        form.amount = "100".to_owned();
        form.issue_date = "not a date".to_owned();
        assert!(form.build().is_err());

        form.issue_date = "2026-01-15".to_owned();
        assert!(form.build().is_err()); // missing phone
        form.customer_phone = "9876543210".to_owned();
        assert!(form.build().is_ok());
    }

    #[test]
    fn accepts_only_indian_phone_numbers() {
        for (input, expected) in [
            ("9876543210", "+919876543210"),
            ("+91 98765 43210", "+919876543210"),
            ("91-98765-43210", "+919876543210"),
            ("09876543210", "+919876543210"),
            ("(+91) 91234 56789", "+919123456789"),
        ] {
            let parsed = parse_indian_phone(input, true).expect(input);
            assert_eq!(parsed.as_deref(), Some(expected));
        }

        for input in [
            "1234567890",
            "5551234567",
            "98765 4321",
            "+1 555 0100",
            "98765abc43210",
            "",
        ] {
            assert!(parse_indian_phone(input, true).is_err(), "{input}");
        }

        assert_eq!(parse_indian_phone("", false).unwrap(), None);
    }

    #[test]
    fn preloads_an_existing_invoice() {
        let invoice = Invoice {
            id: 1,
            invoice_no: "INV-1".to_owned(),
            customer: "Acme".to_owned(),
            customer_phone: None,
            issue_date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            due_date: None,
            amount_cents: 12_345,
            currency: "USD".to_owned(),
            status: InvoiceStatus::Paid,
            source: "csv".to_owned(),
            source_file: None,
            imported_at: Utc::now(),
        };

        let form = InvoiceForm::from_invoice(&invoice);
        assert_eq!(form.amount, "123.45");
        assert_eq!(form.status, InvoiceStatus::Paid);
        assert!(form.due_date.is_empty());
    }
}
