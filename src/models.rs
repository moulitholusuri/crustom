//! Domain types shared by storage, import and UI layers.

use chrono::{DateTime, NaiveDate, Utc};

pub const DEFAULT_CURRENCY: &str = "INR";
pub const BASE_CURRENCY_KEY: &str = "base_currency";
pub const DATE_FORMAT_KEY: &str = "date_format";
pub const THEME_KEY: &str = "theme";
pub const TARGET_MONTHLY_KEY: &str = "target_monthly_cents";
pub const TARGET_QUARTERLY_KEY: &str = "target_quarterly_cents";
pub const TARGET_ANNUAL_KEY: &str = "target_annual_cents";

pub const DATE_FORMATS: [(&str, &str); 4] = [
    ("YYYY-MM-DD", "%Y-%m-%d"),
    ("DD MMM YYYY", "%d %b %Y"),
    ("MM/DD/YYYY", "%m/%d/%Y"),
    ("DD/MM/YYYY", "%d/%m/%Y"),
];

pub fn date_format_pattern(setting: &str) -> &'static str {
    DATE_FORMATS
        .iter()
        .find(|(label, _)| *label == setting)
        .map(|(_, pattern)| *pattern)
        .unwrap_or("%Y-%m-%d")
}

pub fn format_date(date: NaiveDate, setting: &str) -> String {
    date.format(date_format_pattern(setting)).to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvoiceStatus {
    Paid,
    Pending,
    Overdue,
}

impl InvoiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paid => "paid",
            Self::Pending => "pending",
            Self::Overdue => "overdue",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Paid => "Paid",
            Self::Pending => "Pending",
            Self::Overdue => "Overdue",
        }
    }

    pub fn from_stored(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "paid" => Self::Paid,
            "overdue" => Self::Overdue,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Invoice {
    #[allow(dead_code)]
    pub id: i64,
    pub invoice_no: String,
    pub customer: String,
    pub customer_phone: Option<String>,
    pub issue_date: NaiveDate,
    pub due_date: Option<NaiveDate>,
    pub amount_cents: i64,
    pub currency: String,
    pub status: InvoiceStatus,
    #[allow(dead_code)]
    pub source: String,
    pub source_file: Option<String>,
    pub imported_at: DateTime<Utc>,
}

/// A validated row ready to be inserted.
#[derive(Debug, Clone)]
pub struct NewInvoice {
    pub invoice_no: String,
    pub customer: String,
    pub customer_phone: Option<String>,
    pub issue_date: NaiveDate,
    pub due_date: Option<NaiveDate>,
    pub amount_cents: i64,
    pub currency: String,
    pub status: InvoiceStatus,
    pub source: String,
    pub source_file: Option<String>,
    pub imported_at: DateTime<Utc>,
    pub fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct ImportBatch {
    #[allow(dead_code)]
    pub id: i64,
    pub filename: String,
    pub format: String,
    pub rows_total: u32,
    pub rows_imported: u32,
    pub rows_skipped: u32,
    pub imported_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InsertResult {
    pub inserted: usize,
    pub skipped_existing: usize,
}

pub fn format_amount(cents: i64, currency: &str) -> String {
    let negative = cents < 0;
    let absolute = cents.unsigned_abs();
    let whole = absolute / 100;
    let fraction = absolute % 100;

    let grouped = group_digits(whole);
    let prefix = currency_symbol(currency);

    let value = format!("{prefix}{grouped}.{fraction:02}");
    if negative { format!("-{value}") } else { value }
}

/// A prefix for a currency: `$`, `€`, … or the uppercase code plus a space.
pub fn currency_symbol(currency: &str) -> String {
    match currency.to_ascii_uppercase().as_str() {
        "USD" => "$".to_owned(),
        "EUR" => "€".to_owned(),
        "GBP" => "£".to_owned(),
        "INR" => "₹".to_owned(),
        "JPY" => "¥".to_owned(),
        "" => String::new(),
        other => format!("{other} "),
    }
}

fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_amounts_with_grouping() {
        assert_eq!(format_amount(123_456, "USD"), "$1,234.56");
        assert_eq!(format_amount(-50, "EUR"), "-€0.50");
        assert_eq!(format_amount(100_000_000, "INR"), "₹1,000,000.00");
        assert_eq!(format_amount(1234, "CHF"), "CHF 12.34");
    }
}
