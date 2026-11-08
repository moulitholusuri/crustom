//! Normalization helpers for imported billing data.

use chrono::{DateTime, NaiveDate, Utc};
use sha2::{Digest, Sha256};

use crate::models::InvoiceStatus;

/// Parse a date from common billing, spreadsheet and ISO formats.
pub fn parse_date(input: &str) -> Result<NaiveDate, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("date is empty".to_owned());
    }

    if let Ok(timestamp) = DateTime::parse_from_rfc3339(raw) {
        return Ok(timestamp.date_naive());
    }

    let without_time = raw
        .split_once('T')
        .map(|(date, _)| date)
        .unwrap_or(raw)
        .trim();
    let first_word = raw.split_whitespace().next().unwrap_or(raw).trim();

    const FORMATS: &[&str] = &[
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%Y.%m.%d",
        "%Y%m%d",
        "%d.%m.%Y",
        "%d/%m/%Y",
        "%d-%m-%Y",
        "%m/%d/%Y",
        "%m-%d-%Y",
        "%d.%m.%y",
        "%d/%m/%y",
        "%m/%d/%y",
        "%d %b %Y",
        "%d-%b-%Y",
        "%d %B %Y",
        "%b %d, %Y",
        "%B %d, %Y",
        "%Y-%m-%d %H:%M:%S",
        "%d.%m.%Y %H:%M:%S",
        "%d/%m/%Y %H:%M:%S",
    ];

    for candidate in [raw, without_time, first_word] {
        for format in FORMATS {
            if let Ok(date) = NaiveDate::parse_from_str(candidate, format) {
                return Ok(date);
            }
        }
    }

    Err(format!("date '{}' was not recognised", clip(raw)))
}

/// Parse thousands-and-decimal amounts in both European and US formats.
pub fn parse_amount(input: &str) -> Result<i64, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("amount is empty".to_owned());
    }

    let mut negative = false;
    let mut cleaned = String::new();
    for character in raw.chars() {
        match character {
            '(' | ')' | '-' => negative = true,
            character if character.is_ascii_digit() => cleaned.push(character),
            '.' | ',' => cleaned.push(character),
            _ => {}
        }
    }

    if !cleaned.chars().any(|character| character.is_ascii_digit()) {
        return Err(format!("amount '{}' was not recognised", clip(raw)));
    }

    let normalized = normalize_separators(&cleaned)?;
    amount_to_cents(&normalized, negative)
}

fn normalize_separators(cleaned: &str) -> Result<String, String> {
    let last_dot = cleaned.rfind('.');
    let last_comma = cleaned.rfind(',');

    if let (Some(dot), Some(comma)) = (last_dot, last_comma) {
        let decimal_separator = if dot > comma { '.' } else { ',' };
        return Ok(decimal_at_last(cleaned, decimal_separator));
    }

    let separator = match (last_dot, last_comma) {
        (Some(_), None) => '.',
        (None, Some(_)) => ',',
        (None, None) => return Ok(cleaned.to_owned()),
        (Some(_), Some(_)) => unreachable!("handled above"),
    };

    let last = cleaned.rfind(separator).expect("separator exists");
    let decimal_digits = cleaned[last + 1..]
        .chars()
        .filter(|character| character.is_ascii_digit())
        .count();

    match decimal_digits {
        0 | 3 => Ok(cleaned
            .chars()
            .filter(|character| character.is_ascii_digit())
            .collect()),
        1 | 2 => Ok(decimal_at_last(cleaned, separator)),
        _ => Err("amount has too many decimal places".to_owned()),
    }
}

fn decimal_at_last(cleaned: &str, separator: char) -> String {
    let last = cleaned.rfind(separator).expect("separator exists");
    let integer: String = cleaned[..last]
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();
    let fraction: String = cleaned[last + 1..]
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();

    if fraction.is_empty() {
        integer
    } else {
        format!("{integer}.{fraction}")
    }
}

fn amount_to_cents(normalized: &str, negative: bool) -> Result<i64, String> {
    let (integer_part, fraction_part) = normalized.split_once('.').unwrap_or((normalized, ""));
    if integer_part.is_empty() && fraction_part.is_empty() {
        return Err("amount is empty".to_owned());
    }

    let integer: i128 = if integer_part.is_empty() {
        0
    } else {
        integer_part
            .parse()
            .map_err(|_| "amount is too large".to_owned())?
    };

    let digits: Vec<u8> = fraction_part
        .bytes()
        .map(|byte| byte.saturating_sub(b'0'))
        .collect();
    let mut fraction = 0i128;
    if let Some(first) = digits.first() {
        fraction += i128::from(*first) * 10;
    }
    if let Some(second) = digits.get(1) {
        fraction += i128::from(*second);
    }
    if let Some(third) = digits.get(2)
        && *third >= 5
    {
        fraction += 1;
    }

    let cents = integer
        .checked_mul(100)
        .and_then(|value| value.checked_add(fraction))
        .ok_or_else(|| "amount is too large".to_owned())?;

    let cents = if negative { -cents } else { cents };
    if cents > i128::from(i64::MAX) || cents < i128::from(i64::MIN) {
        return Err("amount is too large".to_owned());
    }

    Ok(cents as i64)
}

/// Return an ISO 4217 code. A blank value falls back to `default_currency`.
pub fn parse_currency(input: &str, default_currency: &str) -> Result<String, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(default_currency.to_ascii_uppercase());
    }

    let code = match raw {
        "$" | "US$" => "USD",
        "€" => "EUR",
        "£" => "GBP",
        "₹" => "INR",
        "¥" => "JPY",
        other
            if other.len() == 3
                && other
                    .chars()
                    .all(|character| character.is_ascii_alphabetic()) =>
        {
            other
        }
        _ => {
            return Err(format!(
                "currency '{}' is not a three-letter code",
                clip(raw)
            ));
        }
    };

    Ok(code.to_ascii_uppercase())
}

pub fn parse_status(input: &str) -> Option<InvoiceStatus> {
    let normalized: String = input
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect();
    let phrase = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    match phrase.as_str() {
        "paid" | "p" | "closed" | "cleared" | "settled" | "completed" | "complete"
        | "paid in full" | "fully paid" | "paid off" | "payment received" | "received" | "done" => {
            Some(InvoiceStatus::Paid)
        }

        "pending" | "open" | "o" | "unpaid" | "outstanding" | "due" | "invoiced" | "sent"
        | "awaiting payment" | "not paid" | "open item" => Some(InvoiceStatus::Pending),

        "overdue" | "past due" | "late" | "delinquent" | "defaulted" | "pastdue" | "over due" => {
            Some(InvoiceStatus::Overdue)
        }

        _ => None,
    }
}

/// Stable identity used to skip duplicate invoices across imports.
pub fn invoice_fingerprint(
    invoice_no: &str,
    customer: &str,
    issue_date: NaiveDate,
    amount_cents: i64,
    currency: &str,
) -> String {
    let mut hasher = Sha256::new();
    for part in [
        invoice_no.trim().to_lowercase(),
        customer.trim().to_lowercase(),
        issue_date.to_string(),
        amount_cents.to_string(),
        currency.trim().to_uppercase(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    format!("{:x}", hasher.finalize())
}

pub fn today() -> NaiveDate {
    Utc::now().date_naive()
}

fn clip(value: &str) -> String {
    const MAX_CHARS: usize = 40;
    if value.chars().count() <= MAX_CHARS {
        return value.to_owned();
    }
    let shortened: String = value.chars().take(MAX_CHARS).collect();
    format!("{shortened}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn parses_us_and_european_amounts() {
        assert_eq!(parse_amount("1,234.56").unwrap(), 123_456);
        assert_eq!(parse_amount("1.234,56").unwrap(), 123_456);
        assert_eq!(parse_amount("1234").unwrap(), 123_400);
        assert_eq!(parse_amount("12,50").unwrap(), 1_250);
        assert_eq!(parse_amount("1.234,5").unwrap(), 123_450);
        assert_eq!(parse_amount("₹1,23,456.78").unwrap(), 12_345_678);
        assert_eq!(parse_amount("1 234,56").unwrap(), 123_456);
        assert_eq!(parse_amount("1'234.50").unwrap(), 123_450);
        assert_eq!(parse_amount("(1,234.56)").unwrap(), -123_456);
        assert_eq!(parse_amount("-99").unwrap(), -9_900);
        assert_eq!(parse_amount("USD 1,234.56").unwrap(), 123_456);
        assert_eq!(parse_amount("1,234").unwrap(), 123_400);
    }

    #[test]
    fn rejects_unparseable_amounts() {
        assert!(parse_amount("").is_err());
        assert!(parse_amount("not a number").is_err());
        assert!(parse_amount("1.2345").is_err());
    }

    #[test]
    fn parses_common_dates() {
        let expected = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        assert_eq!(parse_date("2026-01-31").unwrap(), expected);
        assert_eq!(parse_date("31.01.2026").unwrap(), expected);
        assert_eq!(parse_date("31/01/2026").unwrap(), expected);
        assert_eq!(parse_date("31 Jan 2026").unwrap(), expected);
        assert_eq!(parse_date("2026-01-31 12:30:00").unwrap(), expected);
        assert_eq!(parse_date("2026-01-31T12:30:00Z").unwrap(), expected);
        assert_eq!(parse_date("01/13/2026").unwrap().year(), 2026);
    }

    #[test]
    fn parses_currencies() {
        assert_eq!(parse_currency("", "USD").unwrap(), "USD");
        assert_eq!(parse_currency("eur", "USD").unwrap(), "EUR");
        assert_eq!(parse_currency("€", "USD").unwrap(), "EUR");
        assert_eq!(parse_currency("$", "EUR").unwrap(), "USD");
        assert!(parse_currency("EURO", "USD").is_err());
    }

    #[test]
    fn parses_status_aliases() {
        assert_eq!(parse_status("PAID"), Some(InvoiceStatus::Paid));
        assert_eq!(parse_status("Open item"), Some(InvoiceStatus::Pending));
        assert_eq!(parse_status("Past Due"), Some(InvoiceStatus::Overdue));
        assert_eq!(parse_status(""), None);
        assert_eq!(parse_status("whatever"), None);
    }

    #[test]
    fn fingerprints_are_stable_and_distinct() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let first = invoice_fingerprint(" INV-1 ", "Acme", date, 1000, "eur");
        let second = invoice_fingerprint("inv-1", "acme", date, 1000, "EUR");
        let different = invoice_fingerprint("inv-1", "acme", date, 1001, "EUR");

        assert_eq!(first, second);
        assert_ne!(first, different);
        assert_eq!(first.len(), 64);
    }
}
