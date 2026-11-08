//! Ordered product/item notes with optional prices.
//!
//! Notes are stored as one item per line using `name | price`, which keeps the
//! existing `people.notes` column backward compatible with plain-text notes.

use crate::models::format_amount;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteItem {
    pub name: String,
    pub price_cents: Option<i64>,
}

pub fn parse(notes: &str) -> Vec<NoteItem> {
    notes
        .lines()
        .filter_map(|line| {
            let line = clean_line(line);
            if line.is_empty() {
                return None;
            }

            let (name, price_cents) = split_price(&line);
            if name.is_empty() {
                None
            } else {
                Some(NoteItem { name, price_cents })
            }
        })
        .collect()
}

pub fn serialize(items: &[NoteItem]) -> String {
    items
        .iter()
        .filter(|item| !item.name.trim().is_empty())
        .map(|item| match item.price_cents {
            Some(cents) => format!("{} | {}", item.name.trim(), cents_to_input(cents)),
            None => item.name.trim().to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the saved items as a numbered list for an SMS message.
pub fn format_for_message(items: &[NoteItem], currency: &str) -> Option<String> {
    let lines: Vec<String> = items
        .iter()
        .filter(|item| !item.name.trim().is_empty())
        .enumerate()
        .map(|(index, item)| match item.price_cents {
            Some(cents) => format!(
                "{}. {} — {}",
                index + 1,
                item.name.trim(),
                format_amount(cents, currency)
            ),
            None => format!("{}. {}", index + 1, item.name.trim()),
        })
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(format!("Items:\n{}", lines.join("\n")))
    }
}

pub fn parse_price(input: &str) -> Result<Option<i64>, String> {
    if input.trim().is_empty() {
        return Ok(None);
    }
    crate::import::normalize::parse_amount(input).map(Some)
}

pub fn price_to_input(cents: Option<i64>) -> String {
    cents.map(cents_to_input).unwrap_or_default()
}

fn cents_to_input(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.unsigned_abs();
    format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
}

fn clean_line(line: &str) -> String {
    let mut text = line.trim();
    for prefix in ["- ", "• ", "* "] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim();
            break;
        }
    }

    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &text[digits..];
        if let Some(rest) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            text = rest.trim();
        }
    }

    text.to_owned()
}

fn split_price(line: &str) -> (String, Option<i64>) {
    if let Some((name, price)) = line.rsplit_once('|')
        && !name.trim().is_empty()
        && let Ok(cents) = crate::import::normalize::parse_amount(price.trim())
    {
        return (name.trim().to_owned(), Some(cents));
    }

    (line.trim().to_owned(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_priced_items() {
        let items = parse(
            "1. Software licence | 1,25,000\n- Support renewal | 25000\n\nImplementation\n2) Analytics | 12,500.50",
        );

        assert_eq!(items.len(), 4);
        assert_eq!(items[0].name, "Software licence");
        assert_eq!(items[0].price_cents, Some(12_500_000));
        assert_eq!(items[1].name, "Support renewal");
        assert_eq!(items[1].price_cents, Some(2_500_000));
        assert_eq!(items[2].name, "Implementation");
        assert_eq!(items[2].price_cents, None);
        assert_eq!(items[3].name, "Analytics");
        assert_eq!(items[3].price_cents, Some(1_250_050));
    }

    #[test]
    fn serializes_and_formats_numbered_items() {
        let items = vec![
            NoteItem {
                name: "Software licence".to_owned(),
                price_cents: Some(12_500_000),
            },
            NoteItem {
                name: "Support renewal".to_owned(),
                price_cents: Some(2_500_000),
            },
        ];

        let stored = serialize(&items);
        assert_eq!(
            stored,
            "Software licence | 125000.00\nSupport renewal | 25000.00"
        );
        assert_eq!(parse(&stored), items);

        let message = format_for_message(&items, "INR").expect("items render");
        assert_eq!(
            message,
            "Items:\n1. Software licence — ₹125,000.00\n2. Support renewal — ₹25,000.00"
        );
    }

    #[test]
    fn handles_missing_prices() {
        let items = parse("Consulting");
        assert_eq!(items[0].price_cents, None);
        assert_eq!(
            format_for_message(&items, "INR").as_deref(),
            Some("Items:\n1. Consulting")
        );
        assert_eq!(parse_price("").unwrap(), None);
        assert_eq!(parse_price("₹1,234.50").unwrap(), Some(123_450));
        assert!(parse_price("not a number").is_err());
    }
}
