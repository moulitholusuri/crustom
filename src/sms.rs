//! Bill status notifications through Fast2SMS or the macOS Messages app.

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::Value;
use ureq::Agent;

use crate::db::Storage;
use crate::models::{Invoice, InvoiceStatus, format_amount, format_date};

pub const SMS_ENABLED_KEY: &str = "sms_enabled";
pub const SMS_METHOD_KEY: &str = "sms_method";
pub const SMS_TEMPLATE_PENDING_KEY: &str = "sms_template_pending";
pub const SMS_TEMPLATE_PAID_KEY: &str = "sms_template_paid";
pub const SMS_TEMPLATE_DUE_KEY: &str = "sms_template_due";
pub const FAST2SMS_API_KEY: &str = "fast2sms_api_key";
pub const FAST2SMS_ENDPOINT: &str = "https://www.fast2sms.com/dev/bulkV2";
pub const SMS_HEADER: &str = "CRSTM";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SmsMethod {
    Fast2Sms,
    #[default]
    MessagesLink,
}

impl SmsMethod {
    pub const ALL: [Self; 2] = [Self::Fast2Sms, Self::MessagesLink];

    pub fn key(self) -> &'static str {
        match self {
            Self::Fast2Sms => "fast2sms",
            Self::MessagesLink => "messages_link",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast2Sms => "Fast2SMS",
            Self::MessagesLink => "Messages app link",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Fast2Sms => "Automatic SMS through Fast2SMS. Requires an API key and credits.",
            Self::MessagesLink => "Opens Messages with the number and text ready. You press Send.",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        match value {
            "fast2sms" => Self::Fast2Sms,
            _ => Self::MessagesLink,
        }
    }
}

pub struct SmsConfig {
    pub api_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsTemplates {
    pub pending: String,
    pub paid: String,
    pub due: String,
}

impl Default for SmsTemplates {
    fn default() -> Self {
        Self {
            pending: "Dear {customer},\n\nYour payment of {amount} is pending.\nDue date: {due}.\nPlease arrange the payment.".to_owned(),
            paid: "Dear {customer},\n\nThank you. We have received your payment of {amount}.\nStatus: Paid.".to_owned(),
            due: "Dear {customer},\n\nYour payment of {amount} is due on {due}.\nPlease make the payment.".to_owned(),
        }
    }
}

impl SmsTemplates {
    pub fn template_for(&self, status: InvoiceStatus) -> &str {
        match status {
            InvoiceStatus::Pending => &self.pending,
            InvoiceStatus::Paid => &self.paid,
            InvoiceStatus::Overdue => &self.due,
        }
    }
}

pub fn load_templates(storage: &Storage) -> SmsTemplates {
    let defaults = SmsTemplates::default();
    SmsTemplates {
        pending: stored_template(storage, SMS_TEMPLATE_PENDING_KEY, &defaults.pending),
        paid: stored_template(storage, SMS_TEMPLATE_PAID_KEY, &defaults.paid),
        due: stored_template(storage, SMS_TEMPLATE_DUE_KEY, &defaults.due),
    }
}

fn stored_template(storage: &Storage, key: &str, fallback: &str) -> String {
    storage
        .get_setting(key)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

pub fn save_templates(storage: &Storage, templates: &SmsTemplates) -> Result<(), String> {
    storage
        .set_setting(SMS_TEMPLATE_PENDING_KEY, &templates.pending)
        .map_err(|error| error.to_string())?;
    storage
        .set_setting(SMS_TEMPLATE_PAID_KEY, &templates.paid)
        .map_err(|error| error.to_string())?;
    storage
        .set_setting(SMS_TEMPLATE_DUE_KEY, &templates.due)
        .map_err(|error| error.to_string())
}

pub fn render_template(template: &str, invoice: &Invoice, date_format: &str) -> String {
    let due = invoice
        .due_date
        .map(|date| format_date(date, date_format))
        .unwrap_or_else(|| "not set".to_owned());

    template
        .replace("{customer}", &invoice.customer)
        .replace(
            "{amount}",
            &format_amount(invoice.amount_cents, &invoice.currency),
        )
        .replace("{status}", invoice.status.label())
        .replace("{due}", &due)
}

pub fn compose_message(body: &str, items: Option<&str>) -> String {
    let body = body.trim();
    let header = format!("{SMS_HEADER}:");
    let mut message = if body.is_empty() {
        SMS_HEADER.to_owned()
    } else if body.starts_with(&header) {
        body.to_owned()
    } else {
        format!("{header} {body}")
    };

    if let Some(items) = items.map(str::trim).filter(|items| !items.is_empty()) {
        message.push_str("\n\n");
        message.push_str(items);
    }

    message
}

pub fn sms_method(storage: &Storage) -> SmsMethod {
    if let Some(value) = storage.get_setting(SMS_METHOD_KEY).ok().flatten() {
        return SmsMethod::from_setting(&value);
    }

    let has_key = storage
        .get_setting(FAST2SMS_API_KEY)
        .ok()
        .flatten()
        .is_some_and(|value| !value.trim().is_empty());

    if has_key {
        SmsMethod::Fast2Sms
    } else {
        SmsMethod::MessagesLink
    }
}

pub fn sms_enabled(storage: &Storage) -> bool {
    storage
        .get_setting(SMS_ENABLED_KEY)
        .ok()
        .flatten()
        .is_some_and(|value| value == "true")
}

pub fn load_config(storage: &Storage) -> Option<SmsConfig> {
    if !sms_enabled(storage) {
        return None;
    }

    let api_key = storage
        .get_setting(FAST2SMS_API_KEY)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("FAST2SMS_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })?;

    Some(SmsConfig {
        api_key: api_key.trim().to_owned(),
    })
}

pub fn status_message(
    amount_cents: i64,
    currency: &str,
    status: InvoiceStatus,
    items: Option<&str>,
) -> String {
    let body = format!(
        "{} is now {}.",
        format_amount(amount_cents, currency),
        status.label()
    );
    compose_message(&body, items)
}

pub fn send_status_sms(config: &SmsConfig, phone: &str, message: &str) -> Result<String, String> {
    let number = fast2sms_number(phone)?;
    send_via_endpoint(FAST2SMS_ENDPOINT, &config.api_key, &number, message)
}

pub fn send_test_sms(api_key: &str, phone: &str) -> Result<String, String> {
    let number = fast2sms_number(phone)?;
    send_via_endpoint(
        FAST2SMS_ENDPOINT,
        api_key.trim(),
        &number,
        &compose_message("test message - SMS notifications are working.", None),
    )
}

pub fn open_messages_link(phone: &str, message: &str) -> Result<(), String> {
    let number = format!("+91{}", fast2sms_number(phone)?);
    launch_link(&messages_link(&number, message))
}

fn messages_link(number: &str, message: &str) -> String {
    format!("sms:{number}&body={}", percent_encode(message))
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

#[cfg(target_os = "macos")]
fn launch_link(link: &str) -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg(link)
        .status()
        .map_err(|error| format!("Could not launch the Messages app: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("macOS could not open the Messages link.".to_owned())
    }
}

#[cfg(not(target_os = "macos"))]
fn launch_link(_link: &str) -> Result<(), String> {
    Err("Messages links are only supported on macOS.".to_owned())
}

fn fast2sms_number(phone: &str) -> Result<String, String> {
    let digits: String = phone
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();

    let national = if digits.len() == 10 {
        digits
    } else if digits.len() == 12 && digits.starts_with("91") {
        digits[2..].to_owned()
    } else if digits.len() == 11 && digits.starts_with('0') {
        digits[1..].to_owned()
    } else {
        return Err("The bill customer needs a valid 10-digit Indian mobile number.".to_owned());
    };

    if !national
        .chars()
        .next()
        .is_some_and(|first| matches!(first, '6'..='9'))
    {
        return Err("The bill customer's mobile number must start with 6, 7, 8 or 9.".to_owned());
    }

    Ok(national)
}

fn send_via_endpoint(
    endpoint: &str,
    api_key: &str,
    number: &str,
    message: &str,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("Fast2SMS API key is missing.".to_owned());
    }

    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();

    let mut response = agent
        .post(endpoint)
        .header("authorization", api_key.trim())
        .header("Cache-Control", "no-cache")
        .send_form([
            ("route", "q"),
            ("message", message),
            ("language", "english"),
            ("numbers", number),
        ])
        .map_err(|error| format!("Fast2SMS request failed: {error}"))?;

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("Could not read the Fast2SMS response: {error}"))?;
    let payload: Value = serde_json::from_str(&body)
        .map_err(|error| format!("Invalid Fast2SMS response: {error}"))?;

    if payload
        .get("return")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(payload
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or("sent")
            .to_owned());
    }

    let message = match payload.get("message") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" · "),
        Some(Value::String(value)) => value.clone(),
        _ => "Fast2SMS rejected the message.".to_owned(),
    };
    Err(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InvoiceStatus;

    #[test]
    fn builds_status_messages() {
        assert_eq!(
            status_message(123_456, "INR", InvoiceStatus::Paid, None),
            "CRSTM: ₹1,234.56 is now Paid."
        );
        assert_eq!(
            status_message(
                123_456,
                "INR",
                InvoiceStatus::Paid,
                Some("Items:\n1. Software licence — ₹125,000.00")
            ),
            "CRSTM: ₹1,234.56 is now Paid.\n\nItems:\n1. Software licence — ₹125,000.00"
        );
    }

    #[test]
    fn renders_editable_templates() {
        let invoice = Invoice {
            id: 1,
            invoice_no: "INV-1".to_owned(),
            customer: "Acme".to_owned(),
            customer_phone: Some("+919876543210".to_owned()),
            issue_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
            due_date: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            amount_cents: 123_456,
            currency: "INR".to_owned(),
            status: InvoiceStatus::Pending,
            source: "manual".to_owned(),
            source_file: None,
            imported_at: chrono::Utc::now(),
        };
        let templates = SmsTemplates::default();
        let message = render_template(
            templates.template_for(InvoiceStatus::Pending),
            &invoice,
            "YYYY-MM-DD",
        );

        assert!(message.contains("Dear Acme"));
        assert!(message.contains("₹1,234.56"));
        assert!(message.contains("2026-02-15"));
    }

    #[test]
    fn normalizes_indian_numbers() {
        assert_eq!(fast2sms_number("+919876543210").unwrap(), "9876543210");
        assert_eq!(fast2sms_number("919876543210").unwrap(), "9876543210");
        assert_eq!(fast2sms_number("09876543210").unwrap(), "9876543210");
        assert!(fast2sms_number("+1 555 0100").is_err());
        assert!(fast2sms_number("1234567890").is_err());
    }

    #[test]
    fn builds_messages_links() {
        let message = status_message(123_456, "INR", InvoiceStatus::Paid, None);
        assert_eq!(
            messages_link("+919876543210", &message),
            "sms:+919876543210&body=CRSTM%3A%20%E2%82%B91%2C234.56%20is%20now%20Paid."
        );
    }

    #[test]
    fn saves_and_loads_templates() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut templates = load_templates(&storage);
        assert_eq!(templates, SmsTemplates::default());

        templates.pending = "Pending: {customer} owes {amount}.".to_owned();
        save_templates(&storage, &templates).expect("templates save");

        let loaded = load_templates(&storage);
        assert_eq!(loaded.pending, "Pending: {customer} owes {amount}.");
        assert_eq!(loaded.paid, SmsTemplates::default().paid);
    }

    #[test]
    fn prefixes_the_message_header_once() {
        assert_eq!(compose_message("Hello", None), "CRSTM: Hello");
        assert_eq!(compose_message("CRSTM: Hello", None), "CRSTM: Hello");
        assert_eq!(
            compose_message("Hello", Some("Items:\n1. Licence — ₹100.00")),
            "CRSTM: Hello\n\nItems:\n1. Licence — ₹100.00"
        );
    }

    #[test]
    fn parses_sms_methods() {
        assert_eq!(SmsMethod::from_setting("fast2sms"), SmsMethod::Fast2Sms);
        assert_eq!(
            SmsMethod::from_setting("messages_link"),
            SmsMethod::MessagesLink
        );
        assert_eq!(SmsMethod::from_setting("unknown"), SmsMethod::MessagesLink);
        assert_eq!(SmsMethod::default(), SmsMethod::MessagesLink);
    }

    #[test]
    fn chooses_a_method_from_saved_settings() {
        let storage = Storage::open_in_memory().expect("database opens");
        assert_eq!(sms_method(&storage), SmsMethod::MessagesLink);

        storage.set_setting(FAST2SMS_API_KEY, "key").unwrap();
        assert_eq!(sms_method(&storage), SmsMethod::Fast2Sms);

        storage
            .set_setting(SMS_METHOD_KEY, SmsMethod::MessagesLink.key())
            .unwrap();
        assert_eq!(sms_method(&storage), SmsMethod::MessagesLink);
    }

    #[test]
    fn sends_to_an_endpoint_and_parses_success() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let body =
                r#"{"return":true,"request_id":"req-123","message":["SMS sent successfully"]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let result = send_via_endpoint(
            &format!("http://{address}/dev/bulkV2"),
            "test-key",
            "9876543210",
            "test",
        );
        server.join().unwrap();
        assert_eq!(result.unwrap(), "req-123");
    }

    #[test]
    fn surfaces_api_errors() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let body = r#"{"return":false,"message":["Invalid API key"]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let result = send_via_endpoint(
            &format!("http://{address}/dev/bulkV2"),
            "bad-key",
            "9876543210",
            "test",
        );
        server.join().unwrap();
        assert!(result.unwrap_err().contains("Invalid API key"));
    }
}
