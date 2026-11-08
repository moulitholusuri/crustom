//! Local SQLite storage.
//!
//! The database lives in the platform data directory and never leaves the
//! machine. Phase 2 creates the account and settings tables plus the billing
//! tables that the importer will populate in Phase 3.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use directories::BaseDirs;
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{
    BASE_CURRENCY_KEY, DEFAULT_CURRENCY, ImportBatch, InsertResult, Invoice, InvoiceStatus,
    NewInvoice,
};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("could not create the data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid stored data: {0}")]
    Data(String),
}

pub struct Storage {
    conn: Connection,
    path: PathBuf,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    username         TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash    TEXT NOT NULL,
    created_at       TEXT NOT NULL,
    failed_attempts  INTEGER NOT NULL DEFAULT 0,
    locked_until     INTEGER
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS invoices (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_no    TEXT NOT NULL,
    customer      TEXT NOT NULL,
    issue_date    TEXT NOT NULL,
    due_date      TEXT,
    amount_cents  INTEGER NOT NULL,
    currency      TEXT NOT NULL DEFAULT 'USD',
    status        TEXT NOT NULL DEFAULT 'pending',
    source        TEXT NOT NULL,
    source_file   TEXT,
    imported_at   TEXT NOT NULL,
    fingerprint   TEXT NOT NULL UNIQUE
);

CREATE INDEX IF NOT EXISTS idx_invoices_issue_date ON invoices (issue_date);

CREATE TABLE IF NOT EXISTS import_batches (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    filename      TEXT NOT NULL,
    format        TEXT NOT NULL,
    rows_total    INTEGER NOT NULL DEFAULT 0,
    rows_imported INTEGER NOT NULL DEFAULT 0,
    rows_skipped  INTEGER NOT NULL DEFAULT 0,
    imported_at   TEXT NOT NULL
);
"#;

/// `~/Library/Application Support/Crustom/crustom.db` on macOS.
pub fn default_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("CRUSTOM_DATA_DIR") {
        return PathBuf::from(dir).join("crustom.db");
    }

    BaseDirs::new()
        .map(|base| base.data_dir().join("Crustom"))
        .unwrap_or_else(|| std::env::temp_dir().join("Crustom"))
        .join("crustom.db")
}

/// Keep the private data directory readable only by the current user.
fn restrict_directory(path: &Path) -> Result<(), DbError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Keep the database file readable only by the current user.
fn restrict_file(path: &Path) -> Result<(), DbError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            let created = !parent.exists();
            fs::create_dir_all(parent)?;
            let is_app_directory = parent.file_name().is_some_and(|name| name == "Crustom");
            if created || is_app_directory {
                restrict_directory(parent)?;
            }
        }

        let conn = Connection::open(path)?;
        restrict_file(path)?;
        Self::prepare(conn, path.to_path_buf(), false)
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        Self::prepare(conn, PathBuf::from(":memory:"), true)
    }

    fn prepare(conn: Connection, path: PathBuf, in_memory: bool) -> Result<Self, DbError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(Duration::from_secs(5))?;

        if !in_memory {
            // WAL keeps reads and writes from blocking each other.
            let _ = conn.pragma_update(None, "journal_mode", "WAL");
            let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        }

        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA)?;
        }
        if version < 2 {
            conn.execute("ALTER TABLE invoices ADD COLUMN customer_phone TEXT", [])?;
            conn.pragma_update(None, "user_version", 2)?;
        }
        if version < 3 {
            conn.execute("ALTER TABLE users ADD COLUMN recovery_hash TEXT", [])?;
            conn.pragma_update(None, "user_version", 3)?;
        }
        if version < 4 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS people (
                    customer   TEXT PRIMARY KEY COLLATE NOCASE,
                    notes      TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL
                );",
            )?;
            conn.pragma_update(None, "user_version", 4)?;
        }
        if version < 5 {
            conn.execute(
                "ALTER TABLE people ADD COLUMN extension_count INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            conn.pragma_update(None, "user_version", 5)?;
        }

        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn user_count(&self) -> Result<u32, DbError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        Ok(count as u32)
    }

    pub fn find_username(&self, username: &str) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT username FROM users WHERE username = ?1 COLLATE NOCASE",
                params![username],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn create_user(&self, username: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO users (username, password_hash, created_at)
             VALUES (?1, 'username-only', ?2)",
            params![username, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn invoice_count(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM invoices", [], |row| row.get(0))?)
    }

    pub fn import_batch_count(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM import_batches", [], |row| row.get(0))?)
    }

    /// Insert validated invoices in a transaction and log the import batch.
    pub fn insert_invoices(
        &self,
        invoices: &[NewInvoice],
        filename: &str,
        format: &str,
        total_rows: usize,
    ) -> Result<InsertResult, DbError> {
        let transaction = self.conn.unchecked_transaction()?;
        let mut inserted = 0usize;

        for invoice in invoices {
            let due_date = invoice.due_date.map(|date| date.to_string());
            let changed = transaction.execute(
                "INSERT OR IGNORE INTO invoices (
                    invoice_no, customer, customer_phone, issue_date, due_date, amount_cents,
                    currency, status, source, source_file, imported_at, fingerprint
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    invoice.invoice_no,
                    invoice.customer,
                    invoice.customer_phone,
                    invoice.issue_date.to_string(),
                    due_date,
                    invoice.amount_cents,
                    invoice.currency,
                    invoice.status.as_str(),
                    invoice.source,
                    invoice.source_file,
                    invoice.imported_at.to_rfc3339(),
                    invoice.fingerprint,
                ],
            )?;
            inserted += changed;
        }

        let skipped_existing = invoices.len().saturating_sub(inserted);
        let total = total_rows as i64;
        let imported = inserted as i64;
        let skipped = total.saturating_sub(imported);

        transaction.execute(
            "INSERT INTO import_batches (
                filename, format, rows_total, rows_imported, rows_skipped, imported_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                filename,
                format,
                total,
                imported,
                skipped,
                Utc::now().to_rfc3339()
            ],
        )?;

        transaction.commit()?;
        let _ = self.bump_data_revision();

        Ok(InsertResult {
            inserted,
            skipped_existing,
        })
    }

    pub fn recent_imports(&self, limit: usize) -> Result<Vec<ImportBatch>, DbError> {
        let mut statement = self.conn.prepare(
            "SELECT id, filename, format, rows_total, rows_imported, rows_skipped, imported_at
             FROM import_batches
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;

        let mut batches = Vec::new();
        for row in rows {
            let (id, filename, format, rows_total, rows_imported, rows_skipped, imported_at) = row?;
            batches.push(ImportBatch {
                id,
                filename,
                format,
                rows_total: rows_total as u32,
                rows_imported: rows_imported as u32,
                rows_skipped: rows_skipped as u32,
                imported_at: parse_timestamp(&imported_at)?,
            });
        }
        Ok(batches)
    }

    #[cfg(test)]
    pub fn recent_invoices(&self, limit: usize) -> Result<Vec<Invoice>, DbError> {
        self.query_invoices(limit as i64)
    }

    /// Load every invoice for dashboard aggregation.
    pub fn all_invoices(&self) -> Result<Vec<Invoice>, DbError> {
        self.query_invoices(-1)
    }

    fn query_invoices(&self, limit: i64) -> Result<Vec<Invoice>, DbError> {
        let mut statement = self.conn.prepare(
            "SELECT id, invoice_no, customer, customer_phone, issue_date, due_date,
                    amount_cents, currency, status, source, source_file, imported_at
             FROM invoices
             ORDER BY issue_date DESC, id DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, String>(11)?,
            ))
        })?;

        let mut invoices = Vec::new();
        for row in rows {
            let (
                id,
                invoice_no,
                customer,
                customer_phone,
                issue_date,
                due_date,
                amount_cents,
                currency,
                status,
                source,
                source_file,
                imported_at,
            ) = row?;

            invoices.push(Invoice {
                id,
                invoice_no,
                customer,
                customer_phone,
                issue_date: parse_date(&issue_date)?,
                due_date: due_date.as_deref().map(parse_date).transpose()?,
                amount_cents,
                currency,
                status: InvoiceStatus::from_stored(&status),
                source,
                source_file,
                imported_at: parse_timestamp(&imported_at)?,
            });
        }
        Ok(invoices)
    }

    /// Update an existing invoice, keeping its original source metadata.
    pub fn update_invoice(&self, id: i64, invoice: &NewInvoice) -> Result<(), DbError> {
        let duplicate: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM invoices WHERE fingerprint = ?1 AND id <> ?2",
                params![invoice.fingerprint, id],
                |row| row.get(0),
            )
            .optional()?;
        if duplicate.is_some() {
            return Err(DbError::Data(
                "Another bill already has the same identity.".to_owned(),
            ));
        }

        let changed = self.conn.execute(
            "UPDATE invoices
             SET invoice_no = ?1, customer = ?2, customer_phone = ?3,
                 issue_date = ?4, due_date = ?5, amount_cents = ?6,
                 currency = ?7, status = ?8, fingerprint = ?9
             WHERE id = ?10",
            params![
                invoice.invoice_no,
                invoice.customer,
                invoice.customer_phone,
                invoice.issue_date.to_string(),
                invoice.due_date.map(|date| date.to_string()),
                invoice.amount_cents,
                invoice.currency,
                invoice.status.as_str(),
                invoice.fingerprint,
                id,
            ],
        )?;

        if changed == 0 {
            return Err(DbError::Data("The bill no longer exists.".to_owned()));
        }
        self.bump_data_revision()?;
        Ok(())
    }

    pub fn delete_invoice(&self, id: i64) -> Result<bool, DbError> {
        let changed = self
            .conn
            .execute("DELETE FROM invoices WHERE id = ?1", params![id])?;
        if changed > 0 {
            self.bump_data_revision()?;
        }
        Ok(changed > 0)
    }

    pub fn set_invoice_status(&self, id: i64, status: InvoiceStatus) -> Result<bool, DbError> {
        let changed = self.conn.execute(
            "UPDATE invoices SET status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        if changed > 0 {
            self.bump_data_revision()?;
        }
        Ok(changed > 0)
    }

    /// Add days to an invoice's due date (today + days when no due date exists)
    /// and increment the extension counter for that customer.
    pub fn extend_invoice_due_date(
        &self,
        id: i64,
        days: i64,
    ) -> Result<Option<(String, NaiveDate)>, DbError> {
        let record: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT customer, due_date FROM invoices WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let Some((customer, due_date)) = record else {
            return Ok(None);
        };

        let base = due_date
            .as_deref()
            .map(parse_date)
            .transpose()?
            .unwrap_or_else(|| Utc::now().date_naive());
        let new_due = base + chrono::Duration::days(days);

        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            "UPDATE invoices SET due_date = ?1 WHERE id = ?2",
            params![new_due.to_string(), id],
        )?;
        transaction.execute(
            "INSERT INTO people (customer, notes, extension_count, updated_at)
             VALUES (?1, '', 1, ?2)
             ON CONFLICT(customer) DO UPDATE SET
                 extension_count = people.extension_count + 1,
                 updated_at = excluded.updated_at",
            params![customer.trim(), Utc::now().to_rfc3339()],
        )?;
        transaction.commit()?;
        self.bump_data_revision()?;

        Ok(Some((customer, new_due)))
    }

    /// Insert a manually entered invoice without adding an import-history entry.
    pub fn insert_manual_invoice(&self, invoice: &NewInvoice) -> Result<bool, DbError> {
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO invoices (
                invoice_no, customer, customer_phone, issue_date, due_date, amount_cents,
                currency, status, source, source_file, imported_at, fingerprint
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                invoice.invoice_no,
                invoice.customer,
                invoice.customer_phone,
                invoice.issue_date.to_string(),
                invoice.due_date.map(|date| date.to_string()),
                invoice.amount_cents,
                invoice.currency,
                invoice.status.as_str(),
                invoice.source,
                invoice.source_file,
                invoice.imported_at.to_rfc3339(),
                invoice.fingerprint,
            ],
        )?;
        if changed > 0 {
            self.bump_data_revision()?;
        }
        Ok(changed > 0)
    }

    /// Delete all billing records and import history, but keep the account and settings.
    pub fn clear_workspace(&self) -> Result<(), DbError> {
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute("DELETE FROM invoices", [])?;
        transaction.execute("DELETE FROM import_batches", [])?;
        transaction.execute("DELETE FROM people", [])?;
        transaction.commit()?;
        self.bump_data_revision()?;
        Ok(())
    }

    /// Remove the account, billing data, history and settings to start over.
    pub fn data_revision(&self) -> Result<i64, DbError> {
        Ok(self
            .get_setting("data_revision")?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0))
    }

    fn bump_data_revision(&self) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('data_revision', '1')
             ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            [],
        )?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<Vec<(String, String)>, DbError> {
        let mut statement = self
            .conn
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut settings = Vec::new();
        for row in rows {
            settings.push(row?);
        }
        Ok(settings)
    }

    pub fn base_currency(&self) -> String {
        self.get_setting(BASE_CURRENCY_KEY)
            .ok()
            .flatten()
            .filter(|currency| {
                currency.len() == 3
                    && currency
                        .chars()
                        .all(|character| character.is_ascii_alphabetic())
            })
            .map(|currency| currency.to_ascii_uppercase())
            .unwrap_or_else(|| DEFAULT_CURRENCY.to_owned())
    }

    pub fn date_format(&self) -> String {
        self.get_setting(crate::models::DATE_FORMAT_KEY)
            .ok()
            .flatten()
            .unwrap_or_else(|| "YYYY-MM-DD".to_owned())
    }

    pub fn person_notes(&self, customer: &str) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT notes FROM people WHERE customer = ?1 COLLATE NOCASE",
                params![customer.trim()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn all_person_notes(&self) -> Result<Vec<(String, String)>, DbError> {
        let mut statement = self
            .conn
            .prepare("SELECT customer, notes FROM people ORDER BY customer")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut notes = Vec::new();
        for row in rows {
            notes.push(row?);
        }
        Ok(notes)
    }

    pub fn all_person_extensions(&self) -> Result<Vec<(String, i64)>, DbError> {
        let mut statement = self
            .conn
            .prepare("SELECT customer, extension_count FROM people ORDER BY customer")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut extensions = Vec::new();
        for row in rows {
            extensions.push(row?);
        }
        Ok(extensions)
    }

    pub fn set_person_notes(&self, customer: &str, notes: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO people (customer, notes, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(customer) DO UPDATE SET
                 notes = excluded.notes,
                 updated_at = excluded.updated_at",
            params![customer.trim(), notes, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

fn parse_date(value: &str) -> Result<NaiveDate, DbError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|error| DbError::Data(format!("invalid date '{value}': {error}")))
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, DbError> {
    Ok(DateTime::parse_from_rfc3339(value)
        .map_err(|error| DbError::Data(format!("invalid timestamp '{value}': {error}")))?
        .with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_empty_database() {
        let storage = Storage::open_in_memory().expect("database opens");
        assert_eq!(storage.user_count().unwrap(), 0);
        assert_eq!(storage.invoice_count().unwrap(), 0);
        assert_eq!(storage.base_currency(), "INR");
    }

    #[test]
    fn find_username_is_case_insensitive() {
        let storage = Storage::open_in_memory().expect("database opens");
        storage.create_user("Ravi").unwrap();

        let found = storage.find_username("ravi").unwrap().expect("user exists");
        assert_eq!(found, "Ravi");
    }

    #[test]
    fn settings_round_trip() {
        let storage = Storage::open_in_memory().expect("database opens");
        assert_eq!(storage.get_setting("last_username").unwrap(), None);

        storage.set_setting("last_username", "ravi").unwrap();
        assert_eq!(
            storage.get_setting("last_username").unwrap().as_deref(),
            Some("ravi")
        );

        storage.set_setting("last_username", "ravi2").unwrap();
        assert_eq!(
            storage.get_setting("last_username").unwrap().as_deref(),
            Some("ravi2")
        );
    }

    #[test]
    fn imports_sample_and_deduplicates() {
        let document = crate::import::load_sample().expect("sample loads");
        let mapping = crate::import::auto_mapping(document.headers());
        let plan = crate::import::build_plan(&document, &mapping, "USD").expect("plan builds");
        assert_eq!(plan.valid_count(), plan.rows_total);

        let storage = Storage::open_in_memory().expect("database opens");
        let first = storage
            .insert_invoices(
                &plan.candidates,
                document.file_name(),
                "csv",
                plan.rows_total,
            )
            .expect("first import");
        assert_eq!(first.inserted, plan.rows_total);
        assert_eq!(first.skipped_existing, 0);
        assert_eq!(storage.invoice_count().unwrap(), plan.rows_total as i64);

        let second = storage
            .insert_invoices(
                &plan.candidates,
                document.file_name(),
                "csv",
                plan.rows_total,
            )
            .expect("second import");
        assert_eq!(second.inserted, 0);
        assert_eq!(second.skipped_existing, plan.candidates.len());
        assert_eq!(storage.invoice_count().unwrap(), plan.rows_total as i64);

        let invoices = storage.recent_invoices(100).unwrap();
        assert_eq!(invoices.len(), plan.rows_total);
        assert!(invoices.iter().all(|invoice| invoice.currency == "INR"));

        let batches = storage.recent_imports(10).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].rows_imported, 0);
        assert_eq!(batches[0].rows_skipped, plan.rows_total as u32);
        assert_eq!(batches[1].rows_imported, plan.rows_total as u32);
        assert_eq!(storage.import_batch_count().unwrap(), 2);
    }

    #[test]
    fn stores_notes_per_person() {
        let storage = Storage::open_in_memory().expect("database opens");

        storage
            .set_person_notes("Ravi", "Software licence\nImplementation services")
            .unwrap();
        storage
            .set_person_notes("Priya", "Analytics licence")
            .unwrap();

        assert_eq!(
            storage.person_notes("ravi").unwrap().as_deref(),
            Some("Software licence\nImplementation services")
        );
        assert_eq!(
            storage.person_notes("Priya").unwrap().as_deref(),
            Some("Analytics licence")
        );

        storage
            .set_person_notes("RAVI", "Renewal: Cloud platform")
            .unwrap();
        assert_eq!(
            storage.person_notes("Ravi").unwrap().as_deref(),
            Some("Renewal: Cloud platform")
        );

        let all = storage.all_person_notes().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|(name, notes)| {
            name.eq_ignore_ascii_case("ravi") && notes == "Renewal: Cloud platform"
        }));
    }

    #[test]
    fn migrates_existing_databases_to_current_schema() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "crustom-migration-{}-{stamp}.db",
            std::process::id()
        ));

        {
            let connection = Connection::open(&path).expect("old database opens");
            connection
                .execute_batch(SCHEMA)
                .expect("old schema applies");
            connection
                .pragma_update(None, "user_version", 1)
                .expect("old version set");
        }

        let storage = Storage::open(&path).expect("database migrates");
        let version: i64 = storage
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);

        let invoice_columns: Vec<String> = {
            let mut statement = storage.conn.prepare("PRAGMA table_info(invoices)").unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .map(|row| row.unwrap())
                .collect()
        };
        assert!(
            invoice_columns
                .iter()
                .any(|column| column == "customer_phone")
        );

        let user_columns: Vec<String> = {
            let mut statement = storage.conn.prepare("PRAGMA table_info(users)").unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .map(|row| row.unwrap())
                .collect()
        };
        assert!(user_columns.iter().any(|column| column == "recovery_hash"));
        assert!(storage.all_invoices().unwrap().is_empty());

        drop(storage);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("db-wal"));
        let _ = fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn extends_due_dates_and_counts_extensions_per_person() {
        let document = crate::import::load_sample().expect("sample loads");
        let mapping = crate::import::auto_mapping(document.headers());
        let plan = crate::import::build_plan(&document, &mapping, "USD").expect("plan builds");
        let storage = Storage::open_in_memory().expect("database opens");

        let mut invoice = plan.candidates[0].clone();
        invoice.source = "manual".to_owned();
        invoice.source_file = None;
        storage.insert_manual_invoice(&invoice).unwrap();

        let id = storage.all_invoices().unwrap()[0].id;
        let expected = invoice
            .due_date
            .map(|date| date + chrono::Duration::days(30));

        let (customer, new_due) = storage
            .extend_invoice_due_date(id, 30)
            .unwrap()
            .expect("invoice exists");
        assert_eq!(customer, invoice.customer);
        if let Some(expected) = expected {
            assert_eq!(new_due, expected);
        }

        let extensions = storage.all_person_extensions().unwrap();
        assert!(
            extensions.iter().any(|(name, count)| {
                name.eq_ignore_ascii_case(&invoice.customer) && *count == 1
            })
        );

        storage.extend_invoice_due_date(id, 30).unwrap();
        let extensions = storage.all_person_extensions().unwrap();
        assert!(
            extensions.iter().any(|(name, count)| {
                name.eq_ignore_ascii_case(&invoice.customer) && *count == 2
            })
        );
    }

    #[test]
    fn manually_adds_edits_deletes_and_clears_invoices() {
        let document = crate::import::load_sample().expect("sample loads");
        let mapping = crate::import::auto_mapping(document.headers());
        let plan = crate::import::build_plan(&document, &mapping, "USD").expect("plan builds");
        let storage = Storage::open_in_memory().expect("database opens");

        let mut invoice = plan.candidates[0].clone();
        invoice.source = "manual".to_owned();
        invoice.source_file = None;
        assert!(storage.insert_manual_invoice(&invoice).unwrap());
        assert!(!storage.insert_manual_invoice(&invoice).unwrap());

        let stored = storage.all_invoices().unwrap();
        assert_eq!(stored.len(), 1);
        let id = stored[0].id;

        let mut edited = invoice.clone();
        edited.amount_cents += 1_000;
        edited.status = InvoiceStatus::Paid;
        edited.fingerprint = crate::import::normalize::invoice_fingerprint(
            &edited.invoice_no,
            &edited.customer,
            edited.issue_date,
            edited.amount_cents,
            &edited.currency,
        );
        storage.update_invoice(id, &edited).unwrap();

        let updated = storage.all_invoices().unwrap();
        assert_eq!(updated[0].amount_cents, edited.amount_cents);
        assert_eq!(updated[0].status, InvoiceStatus::Paid);

        assert!(
            storage
                .set_invoice_status(id, InvoiceStatus::Overdue)
                .unwrap()
        );
        assert_eq!(
            storage.all_invoices().unwrap()[0].status,
            InvoiceStatus::Overdue
        );

        assert!(storage.delete_invoice(id).unwrap());
        assert_eq!(storage.invoice_count().unwrap(), 0);

        storage
            .insert_invoices(
                &plan.candidates,
                document.file_name(),
                "csv",
                plan.rows_total,
            )
            .unwrap();
        assert!(storage.invoice_count().unwrap() > 0);
        storage.clear_workspace().unwrap();
        assert_eq!(storage.invoice_count().unwrap(), 0);
        assert_eq!(storage.import_batch_count().unwrap(), 0);
    }
}
