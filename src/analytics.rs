//! Aggregations behind the dashboard: KPIs, time buckets, targets and aging.

use std::collections::{BTreeSet, HashMap};

use chrono::{Datelike, Days, NaiveDate};

use crate::models::{Invoice, InvoiceStatus};

const DAYS_PER_MONTH: f64 = 30.4375;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    ThisMonth,
    QuarterToDate,
    YearToDate,
    #[default]
    Last12Months,
    AllTime,
}

impl Period {
    pub const ALL: [Self; 5] = [
        Self::ThisMonth,
        Self::QuarterToDate,
        Self::YearToDate,
        Self::Last12Months,
        Self::AllTime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ThisMonth => "This month",
            Self::QuarterToDate => "QTD",
            Self::YearToDate => "YTD",
            Self::Last12Months => "Last 12M",
            Self::AllTime => "All time",
        }
    }

    pub fn range(self, invoices: &[Invoice], today: NaiveDate) -> (NaiveDate, NaiveDate) {
        match self {
            Self::ThisMonth => (first_of_month(today), today),
            Self::QuarterToDate => {
                let month = quarter_start_month(today.month());
                (
                    NaiveDate::from_ymd_opt(today.year(), month, 1).unwrap_or(today),
                    today,
                )
            }
            Self::YearToDate => (
                NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today),
                today,
            ),
            Self::Last12Months => {
                let month = add_months(first_of_month(today), -11);
                (month, today)
            }
            Self::AllTime => {
                let earliest = invoices
                    .iter()
                    .map(|invoice| invoice.issue_date)
                    .min()
                    .unwrap_or_else(|| first_of_month(today));
                let latest = invoices
                    .iter()
                    .map(|invoice| invoice.issue_date)
                    .max()
                    .unwrap_or(today);
                (earliest, latest)
            }
        }
    }

    /// The immediately preceding range of the same length, for growth comparisons.
    pub fn previous_range(
        self,
        invoices: &[Invoice],
        today: NaiveDate,
    ) -> Option<(NaiveDate, NaiveDate)> {
        if self == Self::AllTime {
            return None;
        }
        let (start, end) = self.range(invoices, today);
        let days = (end - start).num_days() + 1;
        let previous_end = start.pred_opt()?;
        let previous_start = previous_end.checked_sub_days(Days::new(days as u64 - 1))?;
        Some((previous_start, previous_end))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Granularity {
    Day,
    Week,
    #[default]
    Month,
    Quarter,
}

impl Granularity {
    pub const ALL: [Self; 4] = [Self::Day, Self::Week, Self::Month, Self::Quarter];

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "Daily",
            Self::Week => "Weekly",
            Self::Month => "Monthly",
            Self::Quarter => "Quarterly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Metric {
    #[default]
    Revenue,
    Invoices,
    Average,
}

impl Metric {
    pub const ALL: [Self; 3] = [Self::Revenue, Self::Invoices, Self::Average];

    pub fn label(self) -> &'static str {
        match self {
            Self::Revenue => "Revenue",
            Self::Invoices => "Bill count",
            Self::Average => "Average bill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartKind {
    Line,
    #[default]
    Bar,
}

impl ChartKind {
    pub const ALL: [Self; 2] = [Self::Line, Self::Bar];

    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Bar => "Bar",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsFilter {
    pub period: Period,
    pub currency: String,
    pub customer: Option<String>,
    pub status: Option<InvoiceStatus>,
}

#[derive(Debug, Clone, Default)]
pub struct Bucket {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub label: String,
    pub revenue: i64,
    pub count: usize,
}

impl Bucket {
    pub fn metric_value(&self, metric: Metric) -> f64 {
        match metric {
            Metric::Revenue => self.revenue as f64 / 100.0,
            Metric::Invoices => self.count as f64,
            Metric::Average => {
                if self.count == 0 {
                    0.0
                } else {
                    self.revenue as f64 / self.count as f64 / 100.0
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgressPoint {
    pub date: NaiveDate,
    pub actual: i64,
    pub target: i64,
}

#[derive(Debug, Clone)]
pub struct CustomerTotal {
    pub customer: String,
    pub amount: i64,
    pub invoices: usize,
}

#[derive(Debug, Clone)]
pub struct Analytics {
    pub range_start: NaiveDate,
    pub range_end: NaiveDate,
    pub currency: String,
    pub invoice_count: usize,
    pub total_billed: i64,
    pub collected: i64,
    pub outstanding: i64,
    pub overdue: i64,
    pub average_invoice: i64,
    pub growth_pct: Option<f64>,
    pub buckets: Vec<Bucket>,
    pub progress: Vec<ProgressPoint>,
    pub status_totals: [i64; 3],
    pub status_counts: [usize; 3],
    pub top_customers: Vec<CustomerTotal>,
    /// Current, 1–30, 31–60, 61–90, 90+.
    pub aging_totals: [i64; 5],
    pub aging_counts: [usize; 5],
    pub target_monthly: i64,
    pub target_total: i64,
    pub target_progress_pct: f64,
}

pub fn analyze(
    invoices: &[Invoice],
    filter: &AnalyticsFilter,
    granularity: Granularity,
    monthly_target_cents: i64,
    today: NaiveDate,
) -> Analytics {
    let (range_start, range_end) = filter.period.range(invoices, today);
    let includes = |invoice: &Invoice, start: NaiveDate, end: NaiveDate| {
        invoice.currency.eq_ignore_ascii_case(&filter.currency)
            && invoice.issue_date >= start
            && invoice.issue_date <= end
            && filter
                .customer
                .as_ref()
                .is_none_or(|customer| invoice.customer == *customer)
            && filter.status.is_none_or(|status| invoice.status == status)
    };

    let filtered: Vec<&Invoice> = invoices
        .iter()
        .filter(|invoice| includes(invoice, range_start, range_end))
        .collect();

    let mut total_billed = 0i64;
    let mut collected = 0i64;
    let mut outstanding = 0i64;
    let mut overdue = 0i64;
    let mut status_totals = [0i64; 3];
    let mut status_counts = [0usize; 3];
    let mut aging_totals = [0i64; 5];
    let mut aging_counts = [0usize; 5];
    let mut customers: HashMap<&str, (i64, usize)> = HashMap::new();

    for invoice in &filtered {
        total_billed += invoice.amount_cents;
        match invoice.status {
            InvoiceStatus::Paid => {
                collected += invoice.amount_cents;
                status_totals[0] += invoice.amount_cents;
                status_counts[0] += 1;
            }
            InvoiceStatus::Pending => {
                outstanding += invoice.amount_cents;
                status_totals[1] += invoice.amount_cents;
                status_counts[1] += 1;
            }
            InvoiceStatus::Overdue => {
                outstanding += invoice.amount_cents;
                overdue += invoice.amount_cents;
                status_totals[2] += invoice.amount_cents;
                status_counts[2] += 1;
            }
        }

        if invoice.status != InvoiceStatus::Paid {
            let bucket = match invoice.due_date {
                Some(due) => {
                    let days = (today - due).num_days();
                    match days {
                        ..=0 => 0,
                        1..=30 => 1,
                        31..=60 => 2,
                        61..=90 => 3,
                        _ => 4,
                    }
                }
                None => 0,
            };
            aging_totals[bucket] += invoice.amount_cents;
            aging_counts[bucket] += 1;
        }

        let entry = customers.entry(invoice.customer.as_str()).or_insert((0, 0));
        entry.0 += invoice.amount_cents;
        entry.1 += 1;
    }

    let invoice_count = filtered.len();
    let average_invoice = if invoice_count == 0 {
        0
    } else {
        total_billed / invoice_count as i64
    };

    let growth_pct =
        filter
            .period
            .previous_range(invoices, today)
            .and_then(|(previous_start, previous_end)| {
                let previous_total: i64 = invoices
                    .iter()
                    .filter(|invoice| includes(invoice, previous_start, previous_end))
                    .map(|invoice| invoice.amount_cents)
                    .sum();
                if previous_total == 0 {
                    None
                } else {
                    Some((total_billed - previous_total) as f64 / previous_total as f64 * 100.0)
                }
            });

    let buckets = build_buckets(&filtered, range_start, range_end, granularity);

    let period_days = (range_end - range_start).num_days() + 1;
    let target_total = if monthly_target_cents > 0 {
        (monthly_target_cents as f64 * period_days as f64 / DAYS_PER_MONTH).round() as i64
    } else {
        0
    };
    let target_progress_pct = if target_total > 0 {
        total_billed as f64 / target_total as f64 * 100.0
    } else {
        0.0
    };

    let mut progress = Vec::with_capacity(buckets.len());
    let mut cumulative = 0i64;
    let range_end_exclusive = range_end.succ_opt().unwrap_or(range_end);
    for bucket in &buckets {
        cumulative += bucket.revenue;
        let elapsed_days = (bucket.end.min(range_end_exclusive) - range_start)
            .num_days()
            .max(1);
        let target =
            (monthly_target_cents as f64 * elapsed_days as f64 / DAYS_PER_MONTH).round() as i64;
        progress.push(ProgressPoint {
            date: bucket.end.pred_opt().unwrap_or(bucket.start),
            actual: cumulative,
            target,
        });
    }

    let mut top_customers: Vec<CustomerTotal> = customers
        .into_iter()
        .map(|(customer, (amount, invoices))| CustomerTotal {
            customer: customer.to_owned(),
            amount,
            invoices,
        })
        .collect();
    top_customers.sort_by(|left, right| {
        right
            .amount
            .cmp(&left.amount)
            .then_with(|| left.customer.cmp(&right.customer))
    });
    top_customers.truncate(5);

    Analytics {
        range_start,
        range_end,
        currency: filter.currency.clone(),
        invoice_count,
        total_billed,
        collected,
        outstanding,
        overdue,
        average_invoice,
        growth_pct,
        buckets,
        progress,
        status_totals,
        status_counts,
        top_customers,
        aging_totals,
        aging_counts,
        target_monthly: monthly_target_cents,
        target_total,
        target_progress_pct,
    }
}

pub fn available_currencies(invoices: &[Invoice]) -> Vec<String> {
    invoices
        .iter()
        .map(|invoice| invoice.currency.to_ascii_uppercase())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn available_customers(invoices: &[Invoice], currency: &str) -> Vec<String> {
    invoices
        .iter()
        .filter(|invoice| invoice.currency.eq_ignore_ascii_case(currency))
        .map(|invoice| invoice.customer.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Pick the currency with the most invoices; ties resolve alphabetically.
pub fn preferred_currency(invoices: &[Invoice]) -> Option<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for invoice in invoices {
        *counts
            .entry(invoice.currency.to_ascii_uppercase())
            .or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(currency, _)| currency)
}

fn build_buckets(
    filtered: &[&Invoice],
    range_start: NaiveDate,
    range_end: NaiveDate,
    granularity: Granularity,
) -> Vec<Bucket> {
    let range_end_exclusive = range_end.succ_opt().unwrap_or(range_end);
    let mut cursor = bucket_start(range_start, granularity);
    let mut buckets = Vec::new();

    while cursor < range_end_exclusive {
        let next = next_bucket(cursor, granularity);
        let start = cursor.max(range_start);
        let end = next.min(range_end_exclusive);
        let mut bucket = Bucket {
            start,
            end,
            label: bucket_label(start, granularity),
            revenue: 0,
            count: 0,
        };

        for invoice in filtered {
            if invoice.issue_date >= start && invoice.issue_date < end {
                bucket.revenue += invoice.amount_cents;
                bucket.count += 1;
            }
        }

        buckets.push(bucket);
        cursor = next;
    }

    buckets
}

fn bucket_start(date: NaiveDate, granularity: Granularity) -> NaiveDate {
    match granularity {
        Granularity::Day => date,
        Granularity::Week => date
            .checked_sub_days(Days::new(date.weekday().num_days_from_monday() as u64))
            .unwrap_or(date),
        Granularity::Month => first_of_month(date),
        Granularity::Quarter => {
            let month = quarter_start_month(date.month());
            NaiveDate::from_ymd_opt(date.year(), month, 1).unwrap_or(date)
        }
    }
}

fn next_bucket(date: NaiveDate, granularity: Granularity) -> NaiveDate {
    match granularity {
        Granularity::Day => date.succ_opt().unwrap_or(date),
        Granularity::Week => date.checked_add_days(Days::new(7)).unwrap_or(date),
        Granularity::Month => add_months(date, 1),
        Granularity::Quarter => add_months(date, 3),
    }
}

fn bucket_label(date: NaiveDate, granularity: Granularity) -> String {
    match granularity {
        Granularity::Day => date.format("%d %b %Y").to_string(),
        Granularity::Week => format!("Week of {}", date.format("%d %b")),
        Granularity::Month => date.format("%b %Y").to_string(),
        Granularity::Quarter => {
            format!("Q{} {}", (date.month0() / 3) + 1, date.year())
        }
    }
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

fn quarter_start_month(month: u32) -> u32 {
    ((month - 1) / 3) * 3 + 1
}

fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + months;
    let year = total.div_euclid(12);
    let month = total.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InvoiceStatus;
    use chrono::Utc;

    fn invoice(
        number: &str,
        customer: &str,
        issue: (i32, u32, u32),
        due: Option<(i32, u32, u32)>,
        amount: i64,
        status: InvoiceStatus,
    ) -> Invoice {
        Invoice {
            id: 1,
            invoice_no: number.to_owned(),
            customer: customer.to_owned(),
            customer_phone: None,
            issue_date: NaiveDate::from_ymd_opt(issue.0, issue.1, issue.2).unwrap(),
            due_date: due.map(|date| NaiveDate::from_ymd_opt(date.0, date.1, date.2).unwrap()),
            amount_cents: amount,
            currency: "EUR".to_owned(),
            status,
            source: "csv".to_owned(),
            source_file: None,
            imported_at: Utc::now(),
        }
    }

    fn sample() -> Vec<Invoice> {
        vec![
            invoice(
                "1",
                "Acme",
                (2026, 1, 15),
                Some((2026, 2, 14)),
                100_000,
                InvoiceStatus::Paid,
            ),
            invoice(
                "2",
                "Globex",
                (2026, 2, 10),
                Some((2026, 3, 12)),
                50_000,
                InvoiceStatus::Pending,
            ),
            invoice(
                "3",
                "Acme",
                (2026, 3, 5),
                Some((2026, 4, 4)),
                25_000,
                InvoiceStatus::Overdue,
            ),
            invoice(
                "4",
                "Initech",
                (2025, 12, 1),
                Some((2026, 1, 1)),
                75_000,
                InvoiceStatus::Paid,
            ),
        ]
    }

    fn filter(period: Period) -> AnalyticsFilter {
        AnalyticsFilter {
            period,
            currency: "EUR".to_owned(),
            customer: None,
            status: None,
        }
    }

    #[test]
    fn period_ranges_are_calendar_aligned() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        assert_eq!(
            Period::ThisMonth.range(&[], today),
            (NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(), today)
        );
        assert_eq!(
            Period::QuarterToDate.range(&[], today).0,
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );
        assert_eq!(
            Period::YearToDate.range(&[], today).0,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
        assert_eq!(
            Period::Last12Months.range(&[], today).0,
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
    }

    #[test]
    fn totals_and_growth_are_computed() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let data = analyze(
            &sample(),
            &filter(Period::AllTime),
            Granularity::Month,
            100_000,
            today,
        );
        assert_eq!(data.total_billed, 250_000);
        assert_eq!(data.collected, 175_000);
        assert_eq!(data.outstanding, 75_000);
        assert_eq!(data.overdue, 25_000);
        assert_eq!(data.invoice_count, 4);
        assert_eq!(data.target_monthly, 100_000);
    }

    #[test]
    fn buckets_cover_months_and_quarters() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let monthly = analyze(
            &sample(),
            &filter(Period::AllTime),
            Granularity::Month,
            0,
            today,
        );
        assert_eq!(monthly.buckets.len(), 4);
        assert_eq!(monthly.buckets[0].revenue, 75_000);
        assert_eq!(monthly.buckets[1].revenue, 100_000);

        let quarterly = analyze(
            &sample(),
            &filter(Period::AllTime),
            Granularity::Quarter,
            0,
            today,
        );
        assert_eq!(quarterly.buckets.len(), 2);
        assert_eq!(quarterly.buckets[1].revenue, 175_000);
    }

    #[test]
    fn aging_buckets_use_due_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let data = analyze(
            &sample(),
            &filter(Period::AllTime),
            Granularity::Month,
            0,
            today,
        );
        assert_eq!(data.aging_totals[2], 25_000);
        assert_eq!(data.aging_totals[3], 50_000);
        assert_eq!(data.aging_counts, [0, 0, 1, 1, 0]);
    }

    #[test]
    fn target_progress_is_prorated_by_days() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let data = analyze(
            &sample(),
            &filter(Period::AllTime),
            Granularity::Month,
            100_000,
            today,
        );
        assert!(data.target_total > 0);
        assert!(data.target_progress_pct > 0.0);
        assert_eq!(data.progress.last().unwrap().actual, data.total_billed);
    }

    #[test]
    fn customer_and_status_filters_apply() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let mut only_acme = filter(Period::AllTime);
        only_acme.customer = Some("Acme".to_owned());
        let data = analyze(&sample(), &only_acme, Granularity::Month, 0, today);
        assert_eq!(data.total_billed, 125_000);
        assert_eq!(data.top_customers.len(), 1);

        let mut only_paid = filter(Period::AllTime);
        only_paid.status = Some(InvoiceStatus::Paid);
        let data = analyze(&sample(), &only_paid, Granularity::Month, 0, today);
        assert_eq!(data.invoice_count, 2);
        assert_eq!(data.collected, 175_000);
        assert_eq!(data.outstanding, 0);
    }

    #[test]
    fn available_filters_and_preferences() {
        let invoices = sample();
        assert_eq!(available_currencies(&invoices), vec!["EUR"]);
        assert_eq!(
            available_customers(&invoices, "EUR"),
            vec!["Acme", "Globex", "Initech"]
        );
        assert_eq!(preferred_currency(&invoices).as_deref(), Some("EUR"));
    }
}
