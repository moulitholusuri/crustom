//! Billing import: CSV/Excel/JSON reading, mapping, validation and dedupe.

pub mod normalize;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use calamine::{Data, Reader, open_workbook_auto};
use chrono::{NaiveTime, Utc};
use serde_json::Value;
use ureq::Agent;

use crate::models::{DEFAULT_CURRENCY, InvoiceStatus, NewInvoice};

const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
const SAMPLE_CSV: &str = include_str!("../../samples/billing_sample.csv");

pub const DELIMITERS: [(u8, &str); 4] = [
    (b',', "Comma"),
    (b';', "Semicolon"),
    (b'\t', "Tab"),
    (b'|', "Pipe"),
];

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Message(String),
    #[error("could not read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse the CSV: {0}")]
    Csv(#[from] csv::Error),
}

impl ImportError {
    fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportField {
    InvoiceNo,
    Customer,
    IssueDate,
    DueDate,
    Amount,
    Currency,
    Status,
}

impl ImportField {
    pub const COUNT: usize = 7;
    pub const ALL: [Self; Self::COUNT] = [
        Self::InvoiceNo,
        Self::Customer,
        Self::IssueDate,
        Self::DueDate,
        Self::Amount,
        Self::Currency,
        Self::Status,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::InvoiceNo => "Bill serial number",
            Self::Customer => "Customer",
            Self::IssueDate => "Issue date",
            Self::DueDate => "Due date",
            Self::Amount => "Amount",
            Self::Currency => "Currency",
            Self::Status => "Status",
        }
    }

    pub fn required(self) -> bool {
        matches!(
            self,
            Self::InvoiceNo | Self::Customer | Self::IssueDate | Self::Amount
        )
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::InvoiceNo => &[
                "invoice",
                "invoice no",
                "invoice number",
                "invoice id",
                "invoice #",
                "billing document",
                "billing doc",
                "billing document no",
                "billing document number",
                "document number",
                "document no",
                "bill no",
                "bill number",
                "bill serial number",
                "bill serial no",
                "serial number of the bill",
                "serial number",
                "number",
                "reference",
                "vbeln",
                "faktura",
            ],
            Self::Customer => &[
                "customer",
                "customer name",
                "customer no",
                "customer number",
                "sold to party",
                "sold-to party",
                "sold to",
                "bill to party",
                "payer",
                "client",
                "account",
                "name",
                "kunnr",
            ],
            Self::IssueDate => &[
                "issue date",
                "invoice date",
                "billing date",
                "document date",
                "posting date",
                "created",
                "date",
                "fkdat",
                "bill date",
            ],
            Self::DueDate => &[
                "due date",
                "due",
                "payment due",
                "payment due date",
                "net due date",
                "due dt",
                "faelligkeitsdatum",
            ],
            Self::Amount => &[
                "amount",
                "invoice amount",
                "billed amount",
                "total",
                "total amount",
                "gross",
                "gross amount",
                "net amount",
                "net value",
                "value",
                "amt",
                "netwr",
                "wrbtr",
                "grand total",
            ],
            Self::Currency => &[
                "currency",
                "document currency",
                "invoice currency",
                "currency code",
                "curr",
                "ccy",
                "waers",
            ],
            Self::Status => &[
                "status",
                "invoice status",
                "payment status",
                "payment state",
                "paid",
                "state",
                "fkst",
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ColumnMapping {
    columns: [Option<usize>; ImportField::COUNT],
}

impl Default for ColumnMapping {
    fn default() -> Self {
        Self {
            columns: [None; ImportField::COUNT],
        }
    }
}

impl ColumnMapping {
    pub fn get(&self, field: ImportField) -> Option<usize> {
        self.columns[field.index()]
    }

    pub fn set(&mut self, field: ImportField, column: Option<usize>) {
        self.columns[field.index()] = column;
    }

    pub fn mapped_count(&self) -> usize {
        self.columns
            .iter()
            .filter(|column| column.is_some())
            .count()
    }

    pub fn required_complete(&self) -> bool {
        ImportField::ALL
            .into_iter()
            .filter(|field| field.required())
            .all(|field| self.get(field).is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Csv,
    Excel,
    Json,
    Rest,
}

impl ImportSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Excel => "xlsx",
            Self::Json => "json",
            Self::Rest => "rest",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Excel => "EXCEL",
            Self::Json => "JSON",
            Self::Rest => "REST",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CsvDocument {
    pub path: PathBuf,
    pub file_name: String,
    pub raw: String,
    pub delimiter: u8,
    pub has_headers: bool,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub first_data_row: usize,
}

impl CsvDocument {
    pub fn set_options(&mut self, delimiter: u8, has_headers: bool) -> Result<(), ImportError> {
        let parsed = parse_csv(&self.raw, delimiter, has_headers)?;
        self.delimiter = delimiter;
        self.has_headers = has_headers;
        self.headers = parsed.headers;
        self.rows = parsed.rows;
        self.first_data_row = parsed.first_data_row;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ExcelDocument {
    pub path: PathBuf,
    pub file_name: String,
    pub sheets: Vec<String>,
    pub selected_sheet: usize,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub first_data_row: usize,
}

impl ExcelDocument {
    pub fn sheet_name(&self) -> &str {
        self.sheets
            .get(self.selected_sheet)
            .map(String::as_str)
            .unwrap_or("Sheet")
    }

    pub fn set_sheet(&mut self, sheet: usize) -> Result<(), ImportError> {
        let parsed = read_excel_sheet(&self.path, sheet)?;
        self.selected_sheet = sheet;
        self.headers = parsed.headers;
        self.rows = parsed.rows;
        self.first_data_row = parsed.first_data_row;
        Ok(())
    }
}

/// JSON file or REST response represented as a table.
#[derive(Debug, Clone)]
pub struct TableDocument {
    pub path: Option<PathBuf>,
    pub file_name: String,
    pub source: ImportSource,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub first_data_row: usize,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ImportDocument {
    Csv(CsvDocument),
    Excel(ExcelDocument),
    Table(TableDocument),
}

impl ImportDocument {
    pub fn file_name(&self) -> &str {
        match self {
            Self::Csv(document) => &document.file_name,
            Self::Excel(document) => &document.file_name,
            Self::Table(document) => &document.file_name,
        }
    }

    pub fn headers(&self) -> &[String] {
        match self {
            Self::Csv(document) => &document.headers,
            Self::Excel(document) => &document.headers,
            Self::Table(document) => &document.headers,
        }
    }

    pub fn rows(&self) -> &[Vec<String>] {
        match self {
            Self::Csv(document) => &document.rows,
            Self::Excel(document) => &document.rows,
            Self::Table(document) => &document.rows,
        }
    }

    pub fn first_data_row(&self) -> usize {
        match self {
            Self::Csv(document) => document.first_data_row,
            Self::Excel(document) => document.first_data_row,
            Self::Table(document) => document.first_data_row,
        }
    }

    pub fn source(&self) -> ImportSource {
        match self {
            Self::Csv(_) => ImportSource::Csv,
            Self::Excel(_) => ImportSource::Excel,
            Self::Table(document) => document.source,
        }
    }

    pub fn source_label(&self) -> &'static str {
        self.source().label()
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Csv(document) => Some(&document.path),
            Self::Excel(document) => Some(&document.path),
            Self::Table(document) => document.path.as_deref(),
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Csv(_) => None,
            Self::Excel(document) => Some(document.sheet_name()),
            Self::Table(document) => document.detail.as_deref(),
        }
    }

    pub fn as_csv(&self) -> Option<&CsvDocument> {
        match self {
            Self::Csv(document) => Some(document),
            _ => None,
        }
    }

    pub fn as_csv_mut(&mut self) -> Option<&mut CsvDocument> {
        match self {
            Self::Csv(document) => Some(document),
            _ => None,
        }
    }

    pub fn as_excel(&self) -> Option<&ExcelDocument> {
        match self {
            Self::Excel(document) => Some(document),
            _ => None,
        }
    }

    pub fn as_excel_mut(&mut self) -> Option<&mut ExcelDocument> {
        match self {
            Self::Excel(document) => Some(document),
            _ => None,
        }
    }
}

struct ParsedCsv {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    first_data_row: usize,
}

#[derive(Debug, Clone)]
pub struct RowIssue {
    pub row_number: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ImportPlan {
    pub rows_total: usize,
    pub candidates: Vec<NewInvoice>,
    pub errors: Vec<RowIssue>,
    pub duplicate_rows: usize,
}

impl ImportPlan {
    pub fn valid_count(&self) -> usize {
        self.candidates.len()
    }

    pub fn rejected_count(&self) -> usize {
        self.errors.len() + self.duplicate_rows
    }
}

/// Read a supported billing export: CSV, Excel or JSON.
pub fn read_file(path: &Path) -> Result<ImportDocument, ImportError> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("xlsx" | "xls" | "xlsb" | "ods") => read_excel_file(path).map(ImportDocument::Excel),
        Some("json") => read_json_file(path).map(ImportDocument::Table),
        Some("csv" | "tsv" | "txt") | None => read_csv_file(path).map(ImportDocument::Csv),
        Some(_) => Err(ImportError::message(
            "Unsupported file type. Choose a CSV, Excel or JSON file.",
        )),
    }
}

fn read_csv_file(path: &Path) -> Result<CsvDocument, ImportError> {
    ensure_file_size(path)?;
    let bytes = fs::read(path)?;
    ensure_file_size_bytes(bytes.len() as u64)?;

    let raw = decode_bytes(&bytes);
    let mut document = document_from_raw(path, raw)?;
    let guessed_headers = guess_has_headers(&document.raw, document.delimiter);
    if guessed_headers != document.has_headers {
        document.set_options(document.delimiter, guessed_headers)?;
    }
    Ok(document)
}

/// Load the bundled billing-style sample without touching the filesystem.
pub fn load_sample() -> Result<ImportDocument, ImportError> {
    Ok(ImportDocument::Csv(document_from_raw(
        Path::new("samples/billing_sample.csv"),
        SAMPLE_CSV.to_owned(),
    )?))
}

fn ensure_file_size(path: &Path) -> Result<(), ImportError> {
    let metadata = fs::metadata(path)?;
    ensure_file_size_bytes(metadata.len())
}

fn ensure_file_size_bytes(bytes: u64) -> Result<(), ImportError> {
    if bytes > MAX_FILE_BYTES {
        return Err(ImportError::message(format!(
            "The file is too large ({:.1} MB). The limit is 25 MB.",
            bytes as f64 / (1024.0 * 1024.0)
        )));
    }
    Ok(())
}

fn read_excel_file(path: &Path) -> Result<ExcelDocument, ImportError> {
    ensure_file_size(path)?;

    let mut workbook = open_workbook_auto(path).map_err(|error| {
        ImportError::message(format!("Could not open the Excel workbook: {error}"))
    })?;
    let sheets = workbook.sheet_names();
    if sheets.is_empty() {
        return Err(ImportError::message(
            "The Excel workbook does not contain any sheets.",
        ));
    }

    let mut selected_sheet = 0usize;
    let mut parsed = None;
    for (index, sheet_name) in sheets.iter().enumerate() {
        let Some(range) = workbook.worksheet_range_at(index) else {
            continue;
        };
        let range = range.map_err(|error| {
            ImportError::message(format!("Could not read sheet '{sheet_name}': {error}"))
        })?;
        if let Ok(table) = table_from_excel_range(&range)
            && !table.rows.is_empty()
        {
            selected_sheet = index;
            parsed = Some(table);
            break;
        }
    }

    let parsed = parsed
        .ok_or_else(|| ImportError::message("No non-empty sheets were found in the workbook."))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("billing.xlsx")
        .to_owned();

    Ok(ExcelDocument {
        path: path.to_path_buf(),
        file_name,
        sheets,
        selected_sheet,
        headers: parsed.headers,
        rows: parsed.rows,
        first_data_row: parsed.first_data_row,
    })
}

fn read_excel_sheet(path: &Path, sheet: usize) -> Result<ParsedCsv, ImportError> {
    let mut workbook = open_workbook_auto(path).map_err(|error| {
        ImportError::message(format!("Could not open the Excel workbook: {error}"))
    })?;
    let range = workbook
        .worksheet_range_at(sheet)
        .ok_or_else(|| ImportError::message("That worksheet no longer exists."))?
        .map_err(|error| ImportError::message(format!("Could not read the worksheet: {error}")))?;

    table_from_excel_range(&range)
}

fn table_from_excel_range(range: &calamine::Range<Data>) -> Result<ParsedCsv, ImportError> {
    let mut records: Vec<Vec<String>> = Vec::new();
    for row in range.rows() {
        let values: Vec<String> = row.iter().map(excel_cell).collect();
        if values.iter().all(|value| value.trim().is_empty()) {
            continue;
        }
        records.push(values);
    }

    if records.is_empty() {
        return Err(ImportError::message("The worksheet is empty."));
    }

    let mut headers: Vec<String> = records
        .remove(0)
        .into_iter()
        .map(|header| header.trim().to_owned())
        .collect();
    let max_width = records
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(headers.len());

    for index in headers.len()..max_width {
        headers.push(format!("Column {}", index + 1));
    }
    for (index, header) in headers.iter_mut().enumerate() {
        if header.is_empty() {
            *header = format!("Column {}", index + 1);
        }
    }
    for row in &mut records {
        row.resize(max_width, String::new());
    }

    Ok(ParsedCsv {
        headers,
        rows: records,
        first_data_row: 2,
    })
}

fn excel_cell(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        Data::Float(value) => format!("{value}"),
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => {
            if *value {
                "TRUE".to_owned()
            } else {
                "FALSE".to_owned()
            }
        }
        Data::DateTime(value) => match value.as_datetime() {
            Some(datetime) if datetime.time() == NaiveTime::MIN => datetime.date().to_string(),
            Some(datetime) => datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
            None => value.to_string(),
        },
        Data::DateTimeIso(value) | Data::DurationIso(value) => value.clone(),
        Data::Error(error) => format!("ERROR {error}"),
    }
}

fn read_json_file(path: &Path) -> Result<TableDocument, ImportError> {
    ensure_file_size(path)?;
    let bytes = fs::read(path)?;
    ensure_file_size_bytes(bytes.len() as u64)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| ImportError::message("The JSON file is not valid UTF-8."))?;
    let text = text.trim_start_matches('\u{feff}');

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("billing.json")
        .to_owned();

    parse_json_document(
        text,
        ImportSource::Json,
        file_name,
        Some(path.to_path_buf()),
        None,
        None,
    )
}

fn parse_json_document(
    raw: &str,
    source: ImportSource,
    file_name: String,
    path: Option<PathBuf>,
    json_path: Option<&str>,
    detail: Option<String>,
) -> Result<TableDocument, ImportError> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| ImportError::message(format!("Invalid JSON: {error}")))?;
    let records = extract_records(&value, json_path)?;

    let (headers, rows) = json_records_to_table(records)?;
    if headers.is_empty() || rows.is_empty() {
        return Err(ImportError::message(
            "The JSON response does not contain any records.",
        ));
    }

    Ok(TableDocument {
        path,
        file_name,
        source,
        headers,
        rows,
        first_data_row: 1,
        detail,
    })
}

fn extract_records<'a>(
    value: &'a Value,
    json_path: Option<&str>,
) -> Result<&'a Value, ImportError> {
    if let Some(path) = json_path.filter(|path| !path.trim().is_empty()) {
        return resolve_json_path(value, path).ok_or_else(|| {
            ImportError::message(format!(
                "JSON path '{}' was not found in the response.",
                path.trim()
            ))
        });
    }

    if value.is_array() {
        return Ok(value);
    }
    if let Value::Object(map) = value {
        if let Some(found) = map.values().find(|value| value.is_array()) {
            return Ok(found);
        }
        return Ok(value);
    }

    Err(ImportError::message(
        "The JSON value is not an array or object.",
    ))
}

fn resolve_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim();
    if path.is_empty() || path == "$" {
        return Some(value);
    }

    if path.starts_with('/') {
        return value.pointer(path);
    }

    let path = path.strip_prefix('$').unwrap_or(path);
    let path = path.strip_prefix('.').unwrap_or(path);
    let mut current = value;
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }
        current = match current {
            Value::Object(map) => map.get(part)?,
            Value::Array(items) => {
                let index: usize = part.parse().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }
    Some(current)
}

fn json_records_to_table(value: &Value) -> Result<(Vec<String>, Vec<Vec<String>>), ImportError> {
    match value {
        Value::Array(items) if items.is_empty() => Err(ImportError::message(
            "The JSON array does not contain any records.",
        )),
        Value::Array(items) if items.iter().all(Value::is_object) => {
            let mut headers = Vec::new();
            let mut seen = HashSet::new();
            for item in items {
                if let Value::Object(map) = item {
                    for key in map.keys() {
                        if seen.insert(key.clone()) {
                            headers.push(key.clone());
                        }
                    }
                }
            }

            let rows = items
                .iter()
                .map(|item| {
                    let Value::Object(map) = item else {
                        return vec![String::new(); headers.len()];
                    };
                    headers
                        .iter()
                        .map(|header| map.get(header.as_str()).map(json_cell).unwrap_or_default())
                        .collect()
                })
                .collect();

            Ok((headers, rows))
        }
        Value::Array(items) if items.iter().all(Value::is_array) => {
            let mut rows: Vec<Vec<String>> = items
                .iter()
                .map(|item| {
                    let Value::Array(values) = item else {
                        return Vec::new();
                    };
                    values.iter().map(json_cell).collect()
                })
                .collect();

            let candidate_headers: Vec<String> = rows
                .first()
                .map(|row| row.iter().map(|value| value.trim().to_owned()).collect())
                .unwrap_or_default();
            let looks_like_header = auto_mapping(&candidate_headers).mapped_count() > 0;

            let headers = if looks_like_header {
                rows.remove(0);
                candidate_headers
            } else {
                let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                (1..=width).map(|index| format!("Column {index}")).collect()
            };

            let width = headers.len();
            for row in &mut rows {
                row.resize(width, String::new());
            }
            Ok((headers, rows))
        }
        Value::Array(items) => {
            let rows = items
                .iter()
                .map(|item| vec![json_cell(item)])
                .collect::<Vec<_>>();
            Ok((vec!["Value".to_owned()], rows))
        }
        Value::Object(_) => {
            let headers = match value {
                Value::Object(map) => map.keys().cloned().collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            let row = headers
                .iter()
                .map(|header| {
                    value
                        .get(header.as_str())
                        .map(json_cell)
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            Ok((headers, vec![row]))
        }
        _ => Err(ImportError::message(
            "The JSON value does not contain tabular records.",
        )),
    }
}

fn json_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct RestConfig {
    pub url: String,
    pub token: String,
    pub headers: String,
    pub json_path: String,
}

/// Fetch JSON from a REST endpoint. This is blocking; call it from a worker thread.
pub fn fetch_rest(config: &RestConfig) -> Result<TableDocument, ImportError> {
    let url = config.url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ImportError::message(
            "The endpoint must start with http:// or https://.",
        ));
    }

    let extra_headers = parse_headers(&config.headers)?;
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();

    let mut request = agent.get(url).header("Accept", "application/json");
    let mut authorization_sent = false;
    for (name, value) in extra_headers {
        if name.eq_ignore_ascii_case("authorization") {
            authorization_sent = true;
        }
        request = request.header(name.as_str(), value);
    }
    if !authorization_sent && !config.token.trim().is_empty() {
        request = request.header("Authorization", format!("Bearer {}", config.token.trim()));
    }

    let mut response = request
        .call()
        .map_err(|error| ImportError::message(format!("API request failed: {error}")))?;
    let body = response.body_mut().read_to_string().map_err(|error| {
        ImportError::message(format!("Could not read the API response: {error}"))
    })?;

    let json_path = config.json_path.trim();
    let detail = if json_path.is_empty() {
        format!("Fetched from {url}")
    } else {
        format!("Fetched from {url} · JSON path {json_path}")
    };

    parse_json_document(
        &body,
        ImportSource::Rest,
        api_file_name(url),
        None,
        Some(json_path),
        Some(detail),
    )
}

fn parse_headers(input: &str) -> Result<Vec<(String, String)>, ImportError> {
    let mut headers = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(ImportError::message(format!(
                "Header line {} must look like `Name: value`.",
                index + 1
            )));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(ImportError::message(format!(
                "Header line {} has no name.",
                index + 1
            )));
        }
        headers.push((name.to_owned(), value.trim().to_owned()));
    }
    Ok(headers)
}

fn api_file_name(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let path = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let label = clip_text(path);
    if label.is_empty() {
        "REST API response".to_owned()
    } else {
        format!("REST · {label}")
    }
}

fn clip_text(value: &str) -> String {
    const MAX_CHARS: usize = 48;
    if value.chars().count() <= MAX_CHARS {
        return value.to_owned();
    }
    let shortened: String = value.chars().take(MAX_CHARS).collect();
    format!("{shortened}…")
}

pub fn auto_mapping(headers: &[String]) -> ColumnMapping {
    let normalized_headers: Vec<String> = headers
        .iter()
        .map(|header| normalize_header(header))
        .collect();

    let mut pairs: Vec<(u32, ImportField, usize)> = Vec::new();
    for field in ImportField::ALL {
        for (column, header) in normalized_headers.iter().enumerate() {
            let score = if header.is_empty() {
                0
            } else {
                field_score(field, header)
            };
            if score > 0 {
                pairs.push((score, field, column));
            }
        }
    }

    pairs.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.index().cmp(&right.1.index()))
            .then_with(|| left.2.cmp(&right.2))
    });

    let mut mapping = ColumnMapping::default();
    let mut used_columns = vec![false; headers.len()];
    for (_, field, column) in pairs {
        if mapping.get(field).is_some() || used_columns[column] {
            continue;
        }
        mapping.set(field, Some(column));
        used_columns[column] = true;
    }

    mapping
}

/// Validate the mapped rows into a plan that is ready to insert.
pub fn build_plan(
    document: &ImportDocument,
    mapping: &ColumnMapping,
    default_currency: &str,
) -> Result<ImportPlan, ImportError> {
    if !mapping.required_complete() {
        let missing: Vec<&str> = ImportField::ALL
            .into_iter()
            .filter(|field| field.required() && mapping.get(*field).is_none())
            .map(ImportField::label)
            .collect();
        return Err(ImportError::message(format!(
            "Map the required columns first: {}.",
            missing.join(", ")
        )));
    }

    let mut seen_columns = HashSet::new();
    for field in ImportField::ALL {
        if let Some(column) = mapping.get(field)
            && !seen_columns.insert(column)
        {
            return Err(ImportError::message(
                "The same column is mapped to more than one field.",
            ));
        }
    }

    let default_currency = normalize::parse_currency(default_currency, DEFAULT_CURRENCY)
        .unwrap_or_else(|_| DEFAULT_CURRENCY.to_owned());
    let imported_at = Utc::now();
    let today = normalize::today();

    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut duplicates = 0usize;
    let mut fingerprints = HashSet::new();

    for (index, row) in document.rows().iter().enumerate() {
        let cell = |field: ImportField| -> &str {
            mapping
                .get(field)
                .and_then(|column| row.get(column))
                .map(|value| value.trim())
                .unwrap_or("")
        };

        let invoice_no = cell(ImportField::InvoiceNo);
        let customer = cell(ImportField::Customer);
        let issue_date_raw = cell(ImportField::IssueDate);
        let due_date_raw = cell(ImportField::DueDate);
        let amount_raw = cell(ImportField::Amount);
        let currency_raw = cell(ImportField::Currency);
        let status_raw = cell(ImportField::Status);
        let row_number = document.first_data_row() + index;

        let mut row_errors: Vec<String> = Vec::new();
        if invoice_no.is_empty() {
            row_errors.push("bill serial number is missing".to_owned());
        }
        if customer.is_empty() {
            row_errors.push("customer is missing".to_owned());
        }

        let issue_date = if issue_date_raw.is_empty() {
            row_errors.push("issue date is missing".to_owned());
            None
        } else {
            match normalize::parse_date(issue_date_raw) {
                Ok(date) => Some(date),
                Err(error) => {
                    row_errors.push(format!("issue {error}"));
                    None
                }
            }
        };

        let due_date = if due_date_raw.is_empty() {
            None
        } else {
            match normalize::parse_date(due_date_raw) {
                Ok(date) => Some(date),
                Err(error) => {
                    row_errors.push(format!("due {error}"));
                    None
                }
            }
        };

        let amount_cents = if amount_raw.is_empty() {
            row_errors.push("amount is missing".to_owned());
            None
        } else {
            match normalize::parse_amount(amount_raw) {
                Ok(amount) => Some(amount),
                Err(error) => {
                    row_errors.push(error);
                    None
                }
            }
        };

        let currency = match normalize::parse_currency(currency_raw, &default_currency) {
            Ok(currency) => Some(currency),
            Err(error) => {
                row_errors.push(error);
                None
            }
        };

        let status = normalize::parse_status(status_raw).unwrap_or_else(|| match due_date {
            Some(due) if due < today => InvoiceStatus::Overdue,
            _ => InvoiceStatus::Pending,
        });

        let (Some(issue_date), Some(amount_cents), Some(currency)) =
            (issue_date, amount_cents, currency)
        else {
            errors.push(RowIssue {
                row_number,
                message: row_errors.join(" · "),
            });
            continue;
        };

        if !row_errors.is_empty() {
            errors.push(RowIssue {
                row_number,
                message: row_errors.join(" · "),
            });
            continue;
        }

        let fingerprint = normalize::invoice_fingerprint(
            invoice_no,
            customer,
            issue_date,
            amount_cents,
            &currency,
        );

        if !fingerprints.insert(fingerprint.clone()) {
            duplicates += 1;
            continue;
        }

        candidates.push(NewInvoice {
            invoice_no: invoice_no.to_owned(),
            customer: customer.to_owned(),
            customer_phone: None,
            issue_date,
            due_date,
            amount_cents,
            currency,
            status,
            source: document.source().as_str().to_owned(),
            source_file: Some(document.file_name().to_owned()),
            imported_at,
            fingerprint,
        });
    }

    Ok(ImportPlan {
        rows_total: document.rows().len(),
        candidates,
        errors,
        duplicate_rows: duplicates,
    })
}

fn document_from_raw(path: &Path, raw: String) -> Result<CsvDocument, ImportError> {
    let raw = raw.trim_start_matches('\u{feff}').to_owned();
    if raw.trim().is_empty() {
        return Err(ImportError::message("The file is empty."));
    }

    let delimiter = detect_delimiter(&raw);
    let has_headers = guess_has_headers(&raw, delimiter);
    let parsed = parse_csv(&raw, delimiter, has_headers)?;
    if parsed.headers.is_empty() {
        return Err(ImportError::message("No columns were found in the file."));
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("billing.csv")
        .to_owned();

    Ok(CsvDocument {
        path: path.to_path_buf(),
        file_name,
        raw,
        delimiter,
        has_headers,
        headers: parsed.headers,
        rows: parsed.rows,
        first_data_row: parsed.first_data_row,
    })
}

fn parse_csv(raw: &str, delimiter: u8, has_headers: bool) -> Result<ParsedCsv, ImportError> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(raw.as_bytes());

    let mut records: Vec<Vec<String>> = Vec::new();
    for record in reader.records() {
        let record = record?;
        let values: Vec<String> = record
            .iter()
            .map(|value| value.trim_start_matches('\u{feff}').to_owned())
            .collect();
        if values.iter().all(|value| value.trim().is_empty()) {
            continue;
        }
        records.push(values);
    }

    if records.is_empty() {
        return Err(ImportError::message("The file is empty."));
    }

    let first_data_row = if has_headers { 2 } else { 1 };
    let mut headers = if has_headers {
        records
            .remove(0)
            .into_iter()
            .map(|header| header.trim().to_owned())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let max_width = records
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(headers.len());

    if headers.is_empty() {
        headers = (1..=max_width)
            .map(|index| format!("Column {index}"))
            .collect();
    } else if headers.len() < max_width {
        for index in headers.len()..max_width {
            headers.push(format!("Column {}", index + 1));
        }
    }

    for (index, header) in headers.iter_mut().enumerate() {
        if header.is_empty() {
            *header = format!("Column {}", index + 1);
        }
    }

    for row in &mut records {
        row.resize(max_width, String::new());
    }

    Ok(ParsedCsv {
        headers,
        rows: records,
        first_data_row,
    })
}

fn detect_delimiter(raw: &str) -> u8 {
    let mut best_score = 0i64;
    let mut best = b',';

    for (delimiter, _) in DELIMITERS {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(raw.as_bytes());

        let mut widths = Vec::new();
        let mut first = Vec::new();
        for record in reader.records().take(20) {
            let Ok(record) = record else {
                break;
            };
            let values: Vec<String> = record.iter().map(str::to_owned).collect();
            if values.iter().all(|value| value.trim().is_empty()) {
                continue;
            }
            if first.is_empty() {
                first = values.clone();
            }
            widths.push(values.len());
        }

        if widths.len() < 2 || widths[0] < 2 {
            continue;
        }

        let consistent = widths.iter().all(|width| *width == widths[0]);
        let alias_hits = auto_mapping(&first).mapped_count() as i64;
        let width = widths[0].min(20) as i64;
        let score = alias_hits * 1000 + i64::from(consistent) * 200 + width * 10;

        if score > best_score {
            best_score = score;
            best = delimiter;
        }
    }

    best
}

fn guess_has_headers(raw: &str, delimiter: u8) -> bool {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(raw.as_bytes());

    let Some(Ok(first)) = reader.records().next() else {
        return true;
    };
    let cells: Vec<String> = first.iter().map(|value| value.trim().to_owned()).collect();

    if auto_mapping(&cells).mapped_count() > 0 {
        return true;
    }

    cells.iter().all(|cell| {
        !cell.is_empty()
            && cell.chars().any(char::is_alphabetic)
            && normalize::parse_date(cell).is_err()
            && normalize::parse_amount(cell).is_err()
    })
}

fn normalize_header(header: &str) -> String {
    header
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn field_score(field: ImportField, normalized_header: &str) -> u32 {
    let mut best = 0;
    for alias in field.aliases() {
        let alias = normalize_header(alias);
        if alias.is_empty() {
            continue;
        }
        let score = if normalized_header == alias {
            1000 + alias.chars().count() as u32
        } else if alias.chars().count() >= 4
            && (normalized_header.starts_with(&alias) || normalized_header.ends_with(&alias))
        {
            100 + alias.chars().count() as u32
        } else if alias.chars().count() >= 5 && normalized_header.contains(&alias) {
            50 + alias.chars().count() as u32
        } else {
            0
        };
        best = best.max(score);
    }
    best
}

fn decode_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (decoded, _, _) = encoding_rs::UTF_16LE.decode(bytes);
        return decoded.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (decoded, _, _) = encoding_rs::UTF_16BE.decode(bytes);
        return decoded.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
            decoded.into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    const US_CSV: &str = "Invoice No,Customer,Invoice Date,Due Date,Amount,Currency,Status
INV-1001,Acme Corp,2026-01-15,2026-02-14,\"1,234.56\",USD,Paid
INV-1002,Globex,2026-02-01,2026-03-03,2500.00,EUR,Open";

    const EU_CSV: &str =
        "Billing Document;Billing Date;Sold-to Party;Net Value;Document Currency;Due Date;Status
90000001;15.01.2026;Müller & Söhne GmbH;12.450,00;EUR;14.02.2026;Paid
90000002;18.01.2026;Northwind Traders;2.500,50;EUR;17.02.2026;Open";

    fn document(raw: &str) -> CsvDocument {
        document_from_raw(Path::new("test.csv"), raw.to_owned()).unwrap()
    }

    #[test]
    fn auto_maps_us_headers() {
        let doc = document(US_CSV);
        assert_eq!(doc.delimiter, b',');
        assert!(doc.has_headers);

        let mapping = auto_mapping(&doc.headers);
        assert_eq!(mapping.get(ImportField::InvoiceNo), Some(0));
        assert_eq!(mapping.get(ImportField::Customer), Some(1));
        assert_eq!(mapping.get(ImportField::IssueDate), Some(2));
        assert_eq!(mapping.get(ImportField::DueDate), Some(3));
        assert_eq!(mapping.get(ImportField::Amount), Some(4));
        assert_eq!(mapping.get(ImportField::Currency), Some(5));
        assert_eq!(mapping.get(ImportField::Status), Some(6));
        assert!(mapping.required_complete());
    }

    #[test]
    fn auto_maps_billing_headers_and_detects_semicolon() {
        let doc = document(EU_CSV);
        assert_eq!(doc.delimiter, b';');
        assert!(doc.has_headers);
        assert_eq!(doc.rows.len(), 2);

        let mapping = auto_mapping(&doc.headers);
        assert_eq!(mapping.mapped_count(), ImportField::COUNT);
        assert_eq!(mapping.get(ImportField::InvoiceNo), Some(0));
        assert_eq!(mapping.get(ImportField::Customer), Some(2));
        assert_eq!(mapping.get(ImportField::Amount), Some(3));
    }

    fn sample_plan() -> (ImportDocument, ColumnMapping, ImportPlan) {
        let document = load_sample().unwrap();
        let mapping = auto_mapping(document.headers());
        let plan = build_plan(&document, &mapping, "USD").unwrap();
        (document, mapping, plan)
    }

    #[test]
    fn builds_plan_with_normalized_values() {
        let doc = ImportDocument::Csv(document(EU_CSV));
        let mapping = auto_mapping(doc.headers());
        let plan = build_plan(&doc, &mapping, "USD").unwrap();

        assert_eq!(plan.rows_total, 2);
        assert_eq!(plan.valid_count(), 2);
        assert!(plan.errors.is_empty());
        assert_eq!(plan.candidates[0].amount_cents, 1_245_000);
        assert_eq!(plan.candidates[0].currency, "EUR");
        assert_eq!(plan.candidates[0].status, InvoiceStatus::Paid);
        assert_eq!(
            plan.candidates[0].issue_date,
            NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
        );
    }

    #[test]
    fn reports_invalid_rows_and_duplicates() {
        let raw = "Invoice No,Customer,Invoice Date,Amount
INV-1,Acme,2026-01-01,100.00
INV-1,Acme,2026-01-01,100.00
INV-2,Globex,not-a-date,50.00
";
        let doc = ImportDocument::Csv(document(raw));
        let mapping = auto_mapping(doc.headers());
        let plan = build_plan(&doc, &mapping, "USD").unwrap();

        assert_eq!(plan.rows_total, 3);
        assert_eq!(plan.valid_count(), 1);
        assert_eq!(plan.duplicate_rows, 1);
        assert_eq!(plan.errors.len(), 1);
        assert_eq!(plan.errors[0].row_number, 4);
        assert!(plan.errors[0].message.contains("date"));
    }

    #[test]
    fn missing_required_mapping_is_an_error() {
        let doc = ImportDocument::Csv(document(US_CSV));
        let mut mapping = auto_mapping(doc.headers());
        mapping.set(ImportField::Amount, None);
        let error = build_plan(&doc, &mapping, "USD").unwrap_err();
        assert!(error.to_string().contains("Amount"));
    }

    #[test]
    fn duplicate_mapped_columns_are_rejected() {
        let doc = ImportDocument::Csv(document(US_CSV));
        let mut mapping = auto_mapping(doc.headers());
        mapping.set(ImportField::Amount, mapping.get(ImportField::InvoiceNo));
        let error = build_plan(&doc, &mapping, "USD").unwrap_err();
        assert!(error.to_string().contains("same column"));
    }

    #[test]
    fn sample_imports_cleanly() {
        let (document, mapping, plan) = sample_plan();
        assert!(mapping.required_complete());
        assert!(plan.rows_total >= 20);
        assert_eq!(plan.valid_count(), plan.rows_total);
        assert!(plan.errors.is_empty());
        assert_eq!(document.source(), ImportSource::Csv);
    }

    #[test]
    fn parses_headerless_data_with_generated_columns() {
        let raw = "INV-1,Acme,2026-01-01,100.00\nINV-2,Globex,2026-01-02,200.00\n";
        let doc = document(raw);
        assert!(!doc.has_headers);
        assert_eq!(doc.headers[0], "Column 1");
        assert_eq!(doc.rows.len(), 2);
    }

    #[test]
    fn reads_the_sample_file_from_disk() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("samples")
            .join("billing_sample.csv");
        let document = read_file(&path).unwrap();
        let csv = document.as_csv().expect("CSV document");

        assert_eq!(document.file_name(), "billing_sample.csv");
        assert_eq!(document.rows().len(), 24);
        assert_eq!(csv.delimiter, b';');
    }

    #[test]
    fn rejects_unknown_extensions() {
        let error = read_file(Path::new("billing.pdf")).unwrap_err();
        assert!(error.to_string().contains("Unsupported"));
    }

    #[test]
    fn decodes_windows_1252_fallback() {
        let text = decode_bytes(&[b'M', 0xFC, b'l', b'l', b'e', b'r']);
        assert_eq!(text, "Müller");
    }

    #[test]
    fn parses_json_array_of_objects() {
        let raw = r#"[
            {"invoice_no":"INV-1","customer":"Acme","issue_date":"2026-01-15","amount":1234.56,"currency":"USD","status":"Paid"},
            {"invoice_no":"INV-2","customer":"Globex","issue_date":"2026-02-01","amount":2500,"currency":"USD","status":"Open"}
        ]"#;
        let document = parse_json_document(
            raw,
            ImportSource::Json,
            "billing.json".to_owned(),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(document.rows.len(), 2);
        assert!(document.headers.iter().any(|header| header == "invoice_no"));
        let mapping = auto_mapping(&document.headers);
        let plan = build_plan(&ImportDocument::Table(document), &mapping, "USD").unwrap();
        assert_eq!(plan.valid_count(), 2);
        assert_eq!(plan.candidates[0].amount_cents, 123_456);
        assert_eq!(plan.candidates[1].status, InvoiceStatus::Pending);
    }

    #[test]
    fn follows_json_paths_and_wrapped_objects() {
        let raw = r#"{"meta":{"page":1},"data":{"items":[
            {"Billing Document":"9001","Sold-to Party":"Acme","Billing Date":"2026-03-01","Net Value":"1.234,56","Document Currency":"EUR"}
        ]}}"#;
        let document = parse_json_document(
            raw,
            ImportSource::Rest,
            "REST · example.com".to_owned(),
            None,
            Some("data.items"),
            Some("Fetched".to_owned()),
        )
        .unwrap();

        assert_eq!(document.rows.len(), 1);
        let mapping = auto_mapping(&document.headers);
        assert!(mapping.required_complete());
        let plan = build_plan(&ImportDocument::Table(document), &mapping, "USD").unwrap();
        assert_eq!(plan.valid_count(), 1);
        assert_eq!(plan.candidates[0].currency, "EUR");
    }

    #[test]
    fn parses_json_array_of_arrays() {
        let raw = r#"[
            ["Invoice No","Customer","Invoice Date","Amount"],
            ["INV-9","Acme","2026-03-01",100.0],
            ["INV-10","Globex","2026-03-02",200.0]
        ]"#;
        let document = parse_json_document(
            raw,
            ImportSource::Json,
            "rows.json".to_owned(),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(document.headers[0], "Invoice No");
        assert_eq!(document.rows.len(), 2);
    }

    #[test]
    fn reports_missing_json_paths() {
        let error = parse_json_document(
            r#"{"data":[]}"#,
            ImportSource::Rest,
            "api".to_owned(),
            None,
            Some("missing.items"),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing.items"));
    }

    #[test]
    fn parses_rest_headers_and_labels() {
        let headers = parse_headers("X-Api-Key: secret\nAccept: application/json").unwrap();
        assert_eq!(headers.len(), 2);
        assert!(parse_headers("not-a-header").is_err());
        assert_eq!(
            api_file_name("https://api.example.com/odata/billing?top=50"),
            "REST · api.example.com/odata/billing"
        );

        let error = fetch_rest(&RestConfig {
            url: "ftp://example.com".to_owned(),
            ..Default::default()
        })
        .unwrap_err();
        assert!(error.to_string().contains("http"));
    }

    #[test]
    fn reads_the_json_sample_file_from_disk() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("samples")
            .join("billing_sample.json");
        let document = read_file(&path).expect("JSON sample imports");

        assert_eq!(document.source(), ImportSource::Json);
        assert_eq!(document.rows().len(), 8);
        let mapping = auto_mapping(document.headers());
        let plan = build_plan(&document, &mapping, "USD").expect("plan builds");
        assert_eq!(plan.valid_count(), 8);
    }

    #[test]
    fn fetches_rest_json_from_a_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("listener binds");
        let address = listener.local_addr().expect("local address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request connects");
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);

            let body = r#"{"items":[{"Invoice No":"REST-1","Customer":"Acme","Invoice Date":"2026-05-01","Amount":420.50,"Currency":"USD"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("response writes");
        });

        let document = fetch_rest(&RestConfig {
            url: format!("http://{address}/billing"),
            json_path: "items".to_owned(),
            ..Default::default()
        })
        .expect("REST fetch succeeds");
        server.join().expect("server thread");

        assert_eq!(document.source, ImportSource::Rest);
        assert_eq!(document.rows.len(), 1);
        let mapping = auto_mapping(&document.headers);
        let plan =
            build_plan(&ImportDocument::Table(document), &mapping, "USD").expect("plan builds");
        assert_eq!(plan.valid_count(), 1);
        assert_eq!(plan.candidates[0].invoice_no, "REST-1");
    }

    #[test]
    fn reads_excel_workbooks_and_selects_non_empty_sheets() {
        use rust_xlsxwriter::Workbook;
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("crustom-excel-{}-{stamp}.xlsx", std::process::id()));

        let mut workbook = Workbook::new();
        workbook
            .add_worksheet()
            .set_name("Cover")
            .expect("cover sheet");
        let sheet = workbook.add_worksheet();
        sheet.set_name("Billing").expect("billing sheet");
        for (column, header) in [
            "Invoice No",
            "Customer",
            "Invoice Date",
            "Amount",
            "Currency",
        ]
        .iter()
        .enumerate()
        {
            sheet
                .write_string(0, column as u16, *header)
                .expect("header");
        }
        sheet.write_string(1, 0, "9001").expect("invoice");
        sheet.write_string(1, 1, "Acme").expect("customer");
        sheet.write_string(1, 2, "2026-01-15").expect("date");
        sheet.write_number(1, 3, 1234.56).expect("amount");
        sheet.write_string(1, 4, "EUR").expect("currency");
        workbook.save(&path).expect("workbook saves");

        let document = read_file(&path).expect("Excel imports");
        let excel = document.as_excel().expect("Excel document");
        assert_eq!(excel.sheet_name(), "Billing");
        assert_eq!(document.source(), ImportSource::Excel);

        let mapping = auto_mapping(document.headers());
        let plan = build_plan(&document, &mapping, "USD").expect("plan builds");
        assert_eq!(plan.valid_count(), 1);
        assert_eq!(plan.candidates[0].amount_cents, 123_456);
        assert_eq!(plan.candidates[0].currency, "EUR");

        let _ = fs::remove_file(path);
    }
}
