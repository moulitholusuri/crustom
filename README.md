# Crustom · Billing Intelligence

A local-first desktop dashboard that turns billing exports into a clear picture of
business progress. Built entirely in Rust with [egui](https://github.com/emilk/egui).

## Download

Prebuilt **universal macOS** builds (Apple Silicon + Intel) are published on the
[Releases page](https://github.com/moulitholusuri/crustom/releases/latest). Download the
latest `Crustom-*-macOS-universal.zip`, unzip it and open `Crustom.app`.

The app is ad-hoc signed for local use. If macOS warns about the developer, right-click the
app and choose **Open** once.

[![CI](https://github.com/moulitholusuri/crustom/actions/workflows/ci.yml/badge.svg)](https://github.com/moulitholusuri/crustom/actions/workflows/ci.yml)

> **Status: Phase 7 of 7 — release.**
> Username-only access, multi-format imports, bill management, People notes, SMS/Messages
> alerts, analytics, settings and macOS app packaging are complete. Version 0.13.0.

## What is in place

### Local account
- First-run **Create your local account** asks only for a **username**.
- Sign-in also asks only for the username: no password is stored or required.
- **Sign out** clears the in-memory session; the username is remembered for next time.
- Database directory (`0700`) and file (`0600`) are restricted to the current user.
- The workspace relies on the macOS account and filesystem permissions for protection.
- REST tokens stay in memory for the session only.

### Billing imports
One pipeline for **CSV, Excel, JSON and REST** with shared mapping, validation and
SHA-256 de-duplication:

- **CSV / TSV / delimited text** with delimiter/header detection and UTF-8, UTF-16 and
  Windows-1252 decoding.
- **Excel workbooks** (`.xlsx`, `.xls`, `.xlsb`, `.ods`) with non-empty sheet detection,
  a sheet picker and Excel date/number normalization.
- **JSON files** with object arrays, array-of-arrays, wrapped arrays and single-object
  responses.
- **REST API** under **Settings → Developer settings**, with GET endpoint, bearer token,
  custom headers and JSON path. Requests run on a background thread with a 20-second timeout.
- Automatic billing header mapping plus full manual re-mapping.
- Validation preview, rejected-row reasons, in-file duplicates and cross-import duplicate
  skipping.
- Transactional inserts and import history.
- Bundled samples:
  - [`samples/billing_sample.csv`](samples/billing_sample.csv)
  - [`samples/billing_sample.json`](samples/billing_sample.json)

### Bill management
- Search, status filtering and **sorting** by date, amount, customer or status.
- **Change a bill's status directly in the ledger** using the status selector on each row.
- **People directory** tab: every customer automatically grouped with contact numbers,
  bill counts, totals, extension counts and full bill history.
- Per-person **Products bought / notes**: an ordered list of items, each with an optional
  price on the same line. Items are stored separately per person, searchable, included in
  SMS messages and kept in JSON backups.
- **Extend** any bill by 30 days from the ledger. The due date moves forward and the
  extension is counted against that customer.
- **Manual entry** from **Overview → Add bill**, including a required Indian mobile
  number normalized to `+91`.
- **Edit** any bill in a modal form.
- **Delete** with confirmation.
- **CSV and TXT export** of the complete ledger, plus a JSON backup of bills, import
  history and non-secret settings.
- Manual and imported bills share the same duplicate identity and analytics.

### SMS & Messages
- When enabled, changing a bill's status notifies that bill customer's Indian mobile
  number. Two delivery methods are available in **Settings → SMS notifications**:
  - **Messages app link** (default) opens the macOS Messages app with the customer's number
    and message ready; you review and press Send. Requires an iPhone signed into the same
    Apple Account with **Text Message Forwarding** enabled. No API key or Crustom-side charge.
  - **Fast2SMS** sends automatically in the background using a Fast2SMS API key. Delivery
    results appear as a notification banner in Details of the bills. Requires Fast2SMS credits and uses
    the **Quick SMS** route, so no DLT template registration is needed.
- Outgoing messages start with **CRSTM** and include the amount and new status. Bill
  numbers are left out of the SMS text, and the customer's saved items are appended as a
  numbered list with prices.
- The **Messages** tab is a dedicated composer: select a bill, load the **Pending**,
  **Paid** or **Due** predefined text, or write your own. Templates can be edited and saved.
  A live preview shows the complete message before **Send** opens it in the Mac Messages app.
- Each bill row also has an **SMS** action. It opens an in-app composer with the customer's
  number, saved items and an editable message; clicking **Send** opens that text in Messages.
- Notifications are skipped with a warning if the bill has no phone number.
- The Fast2SMS key is stored locally, excluded from JSON backups, and only sent to Fast2SMS
  when delivering a message. You can also supply `FAST2SMS_API_KEY` via the environment.

### Analytics dashboard
- Period filters (month, QTD, YTD, last 12 months, all time), currency, customer and status.
- KPI cards for billed, collected, outstanding, overdue and growth.
- Interactive `egui_plot` revenue trend: line/bar, daily/weekly/monthly/quarterly,
  revenue/bill count/average bill, tooltips, pan and zoom.
- Business progress vs a prorated monthly target.
- Payment-status split, top customers and receivables aging.
- Targets and date formats are configurable in Settings.

### Settings & data
- **Themes**: Crustom Dark and Crustom Light, applied immediately and remembered across
  launches. The macOS window chrome follows the selected theme.
- **Account**: signed-in username and username-only access status.
- **SMS notifications**: choose Messages app link or Fast2SMS, enable/disable and test.
- **Developer settings**: REST API import for OData or any JSON HTTP endpoint.
- **Default import currency** and **date format** preferences. New workspaces default to
  **INR (₹)**; existing bills keep their original currency.
- Monthly, quarterly and annual revenue targets.
- **Open data folder**.
- **Export backup (JSON)** without account credentials, including bills, people notes,
  import history and non-secret settings.
- **Reset workspace** with confirmation (keeps the account and settings).
- About panel with version and build phase.

### Application shell
- Sidebar navigation, keyboard shortcuts (`⌘1`–`⌘6`, `⌘⇧L`), native macOS window.
- Custom app icon and locally signed `.app` bundle.

## Run it

```bash
cargo run
```

- Requires a recent stable Rust toolchain (edition 2024).
- First launch asks you to choose a username for the local workspace.
- Try the importer with a bundled sample, set a target in Settings, then explore the
  dashboard.
- Add manual bills from **Overview → Add bill**.
- Customer phone numbers must be valid Indian numbers (10 digits starting 6–9, with
  optional `+91`); they are stored as `+91XXXXXXXXXX` and shown under the customer in the
  ledger and exports.
- To send SMS status alerts: **Settings → SMS notifications**, choose **Messages app link**
  (no API key needed) or **Fast2SMS** (API key required), then enable notifications.
- To compose a message manually: **Messages** tab, or use the **SMS** action on a bill.
- To add items and prices for a person: **People → Products bought / notes**, then **Save notes**.
- To extend a bill: **Details of the bills → Extend** adds 30 days and counts the extension for that person.
- To import from an API: **Settings → Developer settings → Open REST API import**.
- To change a bill status: **Details of the bills → Status selector**.

### Build a macOS app

```bash
cargo run --release --bin package-macos
open dist/Crustom.app
```

The Rust packaging tool builds the release binary, writes a complete `.app` bundle with
`Contents/Info.plist`, the Crustom icon and the executable, then ad-hoc signs it. The
bundle is placed at `dist/Crustom.app`. No Python or shell build scripts are required.

### Data location

macOS: `~/Library/Application Support/Crustom/crustom.db`

The path is shown in Settings. For isolated development, set `CRUSTOM_DATA_DIR` to a
directory; Crustom creates `crustom.db` inside it.

### macOS build note

`.cargo/config.toml` points Cargo at the Command Line Tools compiler, archiver and an
explicit Xcode SDK. This is a local workaround because the Xcode license has not been
accepted on this machine. The file is excluded from Git; GitHub Actions uses the standard
Xcode toolchain. If you accept the license (`sudo xcodebuild -license`), the local file can
be removed.

## Roadmap

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Foundation: theme, shell, navigation, placeholder screens | ✅ Complete |
| 2 | Username-only local account + SQLite storage | ✅ Complete |
| 3 | Billing model, CSV import, column mapping, dedupe | ✅ Complete |
| 4 | Analytics and interactive dashboard charts | ✅ Complete |
| 5 | Excel, JSON and REST imports | ✅ Complete |
| 6 | Bill editing, manual entry, settings and data controls | ✅ Complete |
| 7 | Themes, packaging, final tests and documentation | ✅ Complete |

## Project structure

```text
src/
├── main.rs           # window bootstrap
├── app.rs            # session state, shortcuts, sidebar and screen routing
├── analytics.rs      # periods, buckets, KPIs, targets, growth and aging
├── auth.rs           # username-only local account access
├── db.rs             # SQLite schema, users, bills, imports and settings
├── models.rs         # bill/status/import types, formatting and preferences
├── notes.rs          # ordered item notes, prices and SMS list formatting
├── import/
│   ├── mod.rs        # CSV/Excel/JSON/REST import, mapping, validation, dedupe
│   └── normalize.rs  # dates, amounts, currencies, statuses, fingerprints
├── sms.rs            # Fast2SMS + Messages-link notifications, templates and phone validation
├── theme.rs          # runtime dark/light palettes and shared styles
└── ui/
    ├── mod.rs        # shared buttons, nav items, pills, banners and layout
    ├── login.rs      # first-run setup and sign-in screens
    ├── fatal.rs      # database startup error screen
    ├── dashboard.rs  # KPIs, filters, egui_plot charts, targets and aging
    ├── import.rs     # file importer and Developer-settings REST wizard
    ├── entry.rs      # shared bill form for manual entry and editing
    ├── invoices.rs   # searchable, sortable ledger with edit/extend/export
    ├── people.rs     # customer directory, itemised notes, extensions and history
    ├── sms.rs        # Messages composer with editable pending/paid/due templates
    └── settings.rs   # account, targets, themes and data controls

assets/               # app icon (PNG + ICNS)
samples/              # sample CSV and JSON billing exports
src/bin/package-macos.rs  # Rust build and macOS packaging tool
```

## Limits & notes

- Files up to 25 MB are supported. REST responses use a 10 MB read limit.
- Amounts are stored as integer cents; there is no FX conversion. The dashboard shows one
  currency at a time and targets use the selected dashboard currency.
- The ledger displays up to 500 matching bills per view; exports cover all rows.
- Excel import assumes the first used row contains column headers.
- The database is **not encrypted at rest**; it relies on operating-system account and
  filesystem protection. No password is stored or required.
- Access is **username-only**: anyone who can use your macOS account and read the data
  directory can open the workspace. Keep your macOS account protected.
- REST API import and automatic Fast2SMS notifications are the only features that make
  network calls.
- SMS notifications require either macOS Messages with an iPhone forwarding SMS (link mode)
  or a Fast2SMS account, credits and API key (automatic mode). The bill customer must have
  a valid Indian mobile number.
- The shipped `.app` is ad-hoc signed for local use, not notarized for distribution.
