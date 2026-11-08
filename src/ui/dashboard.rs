use std::ops::RangeInclusive;

use chrono::{Datelike, Days, NaiveDate, Utc};
use eframe::egui;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, RichText, Sense, Stroke, Vec2};
use egui_plot::{Bar, BarChart, GridMark, HoverPosition, Line, LineStyle, Plot, PlotPoints};

use crate::analytics::{self, Analytics, AnalyticsFilter, ChartKind, Granularity, Metric, Period};
use crate::app::Screen;
use crate::db::Storage;
use crate::models::{
    Invoice, InvoiceStatus, TARGET_MONTHLY_KEY, currency_symbol, format_amount, format_date,
};
use crate::theme;
use crate::ui;
use crate::ui::entry::InvoiceForm;

type CacheKey = (
    Period,
    String,
    Option<String>,
    Option<InvoiceStatus>,
    Granularity,
    i64,
    i64,
);

#[derive(Default)]
pub struct DashboardState {
    period: Period,
    currency: Option<String>,
    customer: Option<String>,
    status: Option<InvoiceStatus>,
    granularity: Granularity,
    metric: Metric,
    chart: ChartKind,
    invoices: Vec<Invoice>,
    invoice_count: i64,
    revision: i64,
    loaded: bool,
    error: Option<String>,
    cache_key: Option<CacheKey>,
    cache: Option<Analytics>,
    manual_form: Option<InvoiceForm>,
    manual_notice: Option<String>,
}

impl DashboardState {
    fn refresh(&mut self, storage: &Storage) {
        let revision = storage.data_revision().unwrap_or(self.revision);
        if self.loaded && revision == self.revision {
            return;
        }

        match storage.all_invoices() {
            Ok(invoices) => {
                self.invoice_count = invoices.len() as i64;
                self.invoices = invoices;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!("Could not load billing data: {error}"));
            }
        }
        self.revision = revision;
        self.loaded = true;
        self.cache_key = None;
    }

    fn analytics(&mut self, target_monthly: i64) -> Option<Analytics> {
        let currency = self.currency.clone()?;
        let key: CacheKey = (
            self.period,
            currency.clone(),
            self.customer.clone(),
            self.status,
            self.granularity,
            target_monthly,
            self.invoice_count,
        );

        if self.cache_key.as_ref() != Some(&key) {
            let filter = AnalyticsFilter {
                period: self.period,
                currency,
                customer: self.customer.clone(),
                status: self.status,
            };
            self.cache = Some(analytics::analyze(
                &self.invoices,
                &filter,
                self.granularity,
                target_monthly,
                Utc::now().date_naive(),
            ));
            self.cache_key = Some(key);
        }

        self.cache.clone()
    }
}

pub fn show(ui: &mut egui::Ui, screen: &mut Screen, storage: &Storage, state: &mut DashboardState) {
    let mut open_manual = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui::page_header(
                ui,
                "Overview",
                "Business progress from your billing data",
                |ui| {
                    if ui::primary_button(ui, "Import billing data", 176.0).clicked() {
                        *screen = Screen::Import;
                    }
                    ui.add_space(2.0);
                    if ui::secondary_button(ui, "Add bill", 118.0).clicked() {
                        open_manual = true;
                    }
                    ui.add_space(2.0);
                    ui::pill(ui, "ANALYTICS · LIVE", theme::green());
                },
            );

            if open_manual {
                state.manual_notice = None;
                state.manual_form = Some(InvoiceForm::new(&storage.base_currency()));
            }

            ui.add_space(16.0);
            state.refresh(storage);

            if let Some(error) = &state.error {
                ui::banner(ui, error, theme::red());
                ui.add_space(12.0);
            }

            if let Some(notice) = &state.manual_notice {
                ui::banner(ui, notice, theme::green());
                ui.add_space(12.0);
            }

            manual_entry_card(ui, state, storage);

            if state.invoices.is_empty() {
                empty_workspace(ui, screen);
                ui.add_space(24.0);
                return;
            }

            let currencies = analytics::available_currencies(&state.invoices);
            if state.currency.as_ref().is_none_or(|currency| {
                !currencies
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(currency))
            }) {
                let base_currency = storage.base_currency();
                state.currency = if currencies
                    .iter()
                    .any(|currency| currency.eq_ignore_ascii_case(&base_currency))
                {
                    Some(base_currency)
                } else {
                    analytics::preferred_currency(&state.invoices)
                };
            }
            let currency = state.currency.clone().unwrap_or_default();
            let customers = analytics::available_customers(&state.invoices, &currency);
            if state
                .customer
                .as_ref()
                .is_some_and(|customer| !customers.contains(customer))
            {
                state.customer = None;
            }

            filter_bar(ui, state, &currencies, &customers);

            let currency = state.currency.clone().unwrap_or_default();
            let customers = analytics::available_customers(&state.invoices, &currency);
            if state
                .customer
                .as_ref()
                .is_some_and(|customer| !customers.contains(customer))
            {
                state.customer = None;
            }

            let target_monthly = storage
                .get_setting(TARGET_MONTHLY_KEY)
                .ok()
                .flatten()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0);

            let Some(data) = state.analytics(target_monthly) else {
                return;
            };

            ui.add_space(14.0);

            if data.invoice_count == 0 {
                ui::banner(ui, "No bills match the selected filters.", theme::amber());
                ui.add_space(12.0);
            }

            let growth = data.growth_pct;
            let growth_color = match growth {
                Some(value) if value > 0.05 => theme::green(),
                Some(value) if value < -0.05 => theme::red(),
                _ => theme::faint(),
            };
            let growth_hint = match growth {
                Some(value) => format!("{value:+.1}% vs previous period"),
                None => format!("{} bills in view", data.invoice_count),
            };

            ui.columns(4, |columns| {
                ui::stat_card(
                    &mut columns[0],
                    "TOTAL BILLED",
                    &format_amount(data.total_billed, &data.currency),
                    &growth_hint,
                    theme::accent(),
                    growth_color,
                );
                ui::stat_card(
                    &mut columns[1],
                    "COLLECTED",
                    &format_amount(data.collected, &data.currency),
                    &format!("{} paid bills", data.status_counts[0]),
                    theme::green(),
                    theme::faint(),
                );
                ui::stat_card(
                    &mut columns[2],
                    "OUTSTANDING",
                    &format_amount(data.outstanding, &data.currency),
                    &format!(
                        "{} open bills",
                        data.status_counts[1] + data.status_counts[2]
                    ),
                    theme::amber(),
                    theme::faint(),
                );
                ui::stat_card(
                    &mut columns[3],
                    "OVERDUE",
                    &format_amount(data.overdue, &data.currency),
                    &format!("{} overdue bills", data.status_counts[2]),
                    theme::red(),
                    theme::faint(),
                );
            });

            ui.add_space(16.0);
            revenue_card(ui, state, &data, &storage.date_format());
            ui.add_space(16.0);

            ui.columns(2, |columns| {
                progress_card(&mut columns[0], &data, screen);
                status_card(&mut columns[1], &data);
            });

            ui.add_space(16.0);

            ui.columns(2, |columns| {
                top_customers_card(&mut columns[0], &data);
                aging_card(&mut columns[1], &data);
            });

            ui.add_space(24.0);
        });
}

fn filter_bar(
    ui: &mut egui::Ui,
    state: &mut DashboardState,
    currencies: &[String],
    customers: &[String],
) {
    theme::inset_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("PERIOD")
                    .size(9.5)
                    .strong()
                    .color(theme::faint()),
            );
            ui.spacing_mut().item_spacing.x = 5.0;
            for period in Period::ALL {
                if segment(ui, period.label(), state.period == period).clicked() {
                    state.period = period;
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .link(
                        RichText::new("Reset filters")
                            .size(10.5)
                            .color(theme::muted()),
                    )
                    .clicked()
                {
                    state.period = Period::Last12Months;
                    state.customer = None;
                    state.status = None;
                }
            });
        });

        ui.add_space(8.0);

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Currency").size(10.0).color(theme::faint()));
            let mut selected_currency = state.currency.clone().unwrap_or_default();
            egui::ComboBox::from_id_salt("dashboard_currency")
                .selected_text(selected_currency.clone())
                .width(78.0)
                .show_ui(ui, |ui| {
                    for currency in currencies {
                        ui.selectable_value(&mut selected_currency, currency.clone(), currency);
                    }
                });
            if !selected_currency.is_empty()
                && state.currency.as_deref() != Some(selected_currency.as_str())
            {
                state.currency = Some(selected_currency);
                state.customer = None;
            }

            ui.add_space(10.0);
            ui.label(RichText::new("Customer").size(10.0).color(theme::faint()));
            egui::ComboBox::from_id_salt("dashboard_customer")
                .selected_text(
                    state
                        .customer
                        .clone()
                        .unwrap_or_else(|| "All customers".to_owned()),
                )
                .width(210.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.customer, None, "All customers");
                    for customer in customers {
                        ui.selectable_value(&mut state.customer, Some(customer.clone()), customer);
                    }
                });

            ui.add_space(10.0);
            ui.label(RichText::new("Status").size(10.0).color(theme::faint()));
            egui::ComboBox::from_id_salt("dashboard_status")
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
                        ui.selectable_value(&mut state.status, Some(status), status.label());
                    }
                });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "Currency view · {}",
                        state.currency.as_deref().unwrap_or("—")
                    ))
                    .size(10.0)
                    .color(theme::faint()),
                );
            });
        });
    });
}

fn revenue_card(
    ui: &mut egui::Ui,
    state: &mut DashboardState,
    data: &Analytics,
    date_format: &str,
) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    RichText::new("Revenue trend")
                        .size(14.5)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {} to {}",
                        data.currency,
                        format_date(data.range_start, date_format),
                        format_date(data.range_end, date_format)
                    ))
                    .size(10.5)
                    .color(theme::muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui::pill(ui, &format!("{} BILLS", data.invoice_count), theme::faint());
                ui::pill(
                    ui,
                    &format!(
                        "AVG {}",
                        format_amount(data.average_invoice, &data.currency)
                    ),
                    theme::faint(),
                );
            });
        });

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Interval").size(10.0).color(theme::faint()));
            egui::ComboBox::from_id_salt("chart_granularity")
                .selected_text(state.granularity.label())
                .width(100.0)
                .show_ui(ui, |ui| {
                    for granularity in Granularity::ALL {
                        ui.selectable_value(
                            &mut state.granularity,
                            granularity,
                            granularity.label(),
                        );
                    }
                });

            ui.add_space(8.0);
            ui.label(RichText::new("Metric").size(10.0).color(theme::faint()));
            egui::ComboBox::from_id_salt("chart_metric")
                .selected_text(state.metric.label())
                .width(130.0)
                .show_ui(ui, |ui| {
                    for metric in Metric::ALL {
                        ui.selectable_value(&mut state.metric, metric, metric.label());
                    }
                });

            ui.add_space(8.0);
            ui.label(RichText::new("View").size(10.0).color(theme::faint()));
            ui.spacing_mut().item_spacing.x = 4.0;
            for kind in ChartKind::ALL {
                if segment(ui, kind.label(), state.chart == kind).clicked() {
                    state.chart = kind;
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("Drag to pan · scroll to zoom")
                        .size(9.5)
                        .color(theme::faint()),
                );
            });
        });

        ui.add_space(8.0);

        if data.invoice_count == 0 {
            empty_plot(ui, 240.0, "No bills in this view");
        } else {
            draw_trend_plot(ui, data, state.metric, state.chart, state.granularity);
        }
    });
}

fn draw_trend_plot(
    ui: &mut egui::Ui,
    data: &Analytics,
    metric: Metric,
    kind: ChartKind,
    granularity: Granularity,
) {
    let points: Vec<[f64; 2]> = data
        .buckets
        .iter()
        .map(|bucket| [day_number(bucket.start), bucket.metric_value(metric)])
        .collect();
    let bars: Vec<Bar> = data
        .buckets
        .iter()
        .map(|bucket| {
            let start = day_number(bucket.start);
            let end = day_number(bucket.end).max(start + 1.0);
            let width = (end - start).max(1.0);
            Bar::new(start + width * 0.5, bucket.metric_value(metric))
                .width(width * 0.72)
                .name(bucket.label.clone())
                .fill(theme::accent())
        })
        .collect();
    let marks = grid_marks(data.buckets.iter().map(|bucket| bucket.start).collect(), 8);
    let currency = data.currency.clone();
    let hover_currency = data.currency.clone();

    let mut plot = Plot::new("revenue_trend")
        .height(240.0)
        .allow_zoom(true)
        .allow_drag(true)
        .allow_double_click_reset(true)
        .show_grid(true)
        .include_y(0.0)
        .x_axis_formatter(move |mark, range| date_axis_label(mark.value, range, granularity))
        .y_axis_formatter(move |mark, _| axis_value(mark.value, metric, &currency))
        .label_formatter(move |position| {
            let (position, name) = match position {
                HoverPosition::NearDataPoint {
                    position,
                    plot_name,
                    ..
                } => (position, *plot_name),
                HoverPosition::Elsewhere { position } => (position, ""),
            };
            let date = date_from_days(position.x)
                .map(|date| date.format("%d %b %Y").to_string())
                .unwrap_or_default();
            let value = full_metric_value(position.y, metric, &hover_currency);
            Some(if name.is_empty() {
                format!("{date}\n{value}")
            } else {
                format!("{name}\n{date}\n{value}")
            })
        });

    if !marks.is_empty() {
        plot = plot.x_grid_spacer(move |_| marks.clone());
    }

    plot.show(ui, move |plot_ui| match kind {
        ChartKind::Bar => {
            plot_ui.bar_chart(BarChart::new("Billed", bars).color(theme::accent()));
        }
        ChartKind::Line => {
            plot_ui.line(
                Line::new("Billed", PlotPoints::from(points))
                    .color(theme::accent())
                    .width(2.0)
                    .fill(0.05),
            );
        }
    });
}

fn progress_card(ui: &mut egui::Ui, data: &Analytics, screen: &mut Screen) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    RichText::new("Business progress vs target")
                        .size(14.5)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new("Cumulative billing against your goal")
                        .size(10.5)
                        .color(theme::muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui::pill(
                    ui,
                    if data.target_total > 0 {
                        "TARGET SET"
                    } else {
                        "NO TARGET"
                    },
                    if data.target_total > 0 {
                        theme::green()
                    } else {
                        theme::faint()
                    },
                );
            });
        });

        ui.add_space(14.0);

        if data.target_total == 0 {
            ui.label(
                RichText::new("Set a monthly target to measure progress")
                    .size(15.0)
                    .strong()
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(
                    "The monthly target is prorated across the selected period and drawn as a straight line.",
                )
                .size(11.0)
                .color(theme::muted()),
            );
            ui.add_space(12.0);
            if ui
                .link(
                    RichText::new("Open target settings →")
                        .size(11.5)
                        .color(theme::accent()),
                )
                .clicked()
            {
                *screen = Screen::Settings;
            }
            ui.add_space(10.0);
            empty_plot(ui, 150.0, "No target configured");
            return;
        }

        let percent = data.target_progress_pct;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format_percent(percent))
                    .size(42.0)
                    .strong()
                    .color(if percent >= 100.0 {
                        theme::green()
                    } else {
                        theme::text()
                    }),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} target",
                        format_amount(data.target_total, &data.currency)
                    ))
                    .size(11.5)
                    .color(theme::muted()),
                );
            });
        });
        ui.label(
            RichText::new(format!(
                "{} of {} billed · monthly target {}",
                format_amount(data.total_billed, &data.currency),
                format_amount(data.target_total, &data.currency),
                format_amount(data.target_monthly, &data.currency)
            ))
            .size(10.5)
            .color(theme::faint()),
        );

        ui.add_space(8.0);
        progress_bar(ui, (percent / 100.0).clamp(0.0, 1.0) as f32);

        if !data.progress.is_empty() {
            ui.add_space(10.0);
            draw_progress_plot(ui, data);
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            legend_dot(ui, theme::accent());
            ui.label(RichText::new("Actual").size(10.0).color(theme::muted()));
            ui.add_space(10.0);
            legend_dot(ui, theme::amber());
            ui.label(RichText::new("Target").size(10.0).color(theme::muted()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .link(RichText::new("Edit target").size(10.0).color(theme::faint()))
                    .clicked()
                {
                    *screen = Screen::Settings;
                }
            });
        });
    });
}

fn draw_progress_plot(ui: &mut egui::Ui, data: &Analytics) {
    let actual: Vec<[f64; 2]> = data
        .progress
        .iter()
        .map(|point| [day_number(point.date), point.actual as f64 / 100.0])
        .collect();
    let target: Vec<[f64; 2]> = data
        .progress
        .iter()
        .map(|point| [day_number(point.date), point.target as f64 / 100.0])
        .collect();
    let marks = grid_marks(data.progress.iter().map(|point| point.date).collect(), 6);
    let currency = data.currency.clone();
    let hover_currency = data.currency.clone();

    let mut plot = Plot::new("business_progress")
        .height(150.0)
        .allow_zoom(true)
        .allow_drag(true)
        .allow_double_click_reset(true)
        .show_grid(true)
        .include_y(0.0)
        .x_axis_formatter(|mark, range| date_axis_label(mark.value, range, Granularity::Month))
        .y_axis_formatter(move |mark, _| compact_money(mark.value, &currency))
        .label_formatter(move |position| {
            let (position, name) = match position {
                HoverPosition::NearDataPoint {
                    position,
                    plot_name,
                    ..
                } => (position, *plot_name),
                HoverPosition::Elsewhere { position } => (position, ""),
            };
            let date = date_from_days(position.x)
                .map(|date| date.format("%d %b %Y").to_string())
                .unwrap_or_default();
            let value = format_amount((position.y * 100.0).round() as i64, &hover_currency);
            Some(if name.is_empty() {
                format!("{date}\n{value}")
            } else {
                format!("{name}\n{date}\n{value}")
            })
        });

    if !marks.is_empty() {
        plot = plot.x_grid_spacer(move |_| marks.clone());
    }

    plot.show(ui, move |plot_ui| {
        plot_ui.line(
            Line::new("Actual", PlotPoints::from(actual))
                .color(theme::accent())
                .width(2.0)
                .fill(0.05),
        );
        plot_ui.line(
            Line::new("Target", PlotPoints::from(target))
                .color(theme::amber())
                .width(1.5)
                .style(LineStyle::Dashed { length: 6.0 }),
        );
    });
}

fn status_card(ui: &mut egui::Ui, data: &Analytics) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("Payment status")
                .size(14.5)
                .strong()
                .color(theme::text()),
        );
        ui.label(
            RichText::new("Billed value by bill state")
                .size(10.5)
                .color(theme::muted()),
        );

        ui.add_space(16.0);

        let total: i64 = data.status_totals.iter().sum();
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), theme::field());

        if total > 0 {
            let mut offset = rect.left();
            for (index, amount) in data.status_totals.iter().enumerate() {
                if *amount <= 0 {
                    continue;
                }
                let width = rect.width() * (*amount as f32 / total as f32);
                let segment = egui::Rect::from_min_size(
                    egui::pos2(offset, rect.top()),
                    Vec2::new(width.max(2.0), rect.height()),
                );
                ui.painter()
                    .rect_filled(segment, CornerRadius::same(6), status_color(index));
                offset += width;
            }
        }

        ui.add_space(16.0);

        for (index, status) in [
            InvoiceStatus::Paid,
            InvoiceStatus::Pending,
            InvoiceStatus::Overdue,
        ]
        .into_iter()
        .enumerate()
        {
            ui.horizontal(|ui| {
                legend_dot(ui, status_color(index));
                ui.label(
                    RichText::new(status.label())
                        .size(11.5)
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(format!("{} bills", data.status_counts[index]))
                        .size(10.0)
                        .color(theme::faint()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format_amount(data.status_totals[index], &data.currency))
                            .size(11.5)
                            .strong()
                            .color(theme::text()),
                    );
                    ui.label(
                        RichText::new(share(data.status_totals[index], total))
                            .size(10.0)
                            .color(theme::faint()),
                    );
                });
            });
            if index < 2 {
                ui.add_space(9.0);
            }
        }

        if total == 0 {
            ui.add_space(12.0);
            ui.label(
                RichText::new("No bills in this view")
                    .size(11.0)
                    .color(theme::faint()),
            );
        }
    });
}

fn top_customers_card(ui: &mut egui::Ui, data: &Analytics) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    RichText::new("Top customers")
                        .size(14.5)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new("By billed revenue in this view")
                        .size(10.5)
                        .color(theme::muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui::pill(ui, &format!("{}", data.top_customers.len()), theme::faint());
            });
        });

        ui.add_space(14.0);

        if data.top_customers.is_empty() {
            ui.label(
                RichText::new("No customer revenue in this view")
                    .size(11.0)
                    .color(theme::faint()),
            );
            return;
        }

        let maximum = data
            .top_customers
            .iter()
            .map(|customer| customer.amount)
            .max()
            .unwrap_or(1);

        for (index, customer) in data.top_customers.iter().enumerate() {
            let name = truncate(&customer.customer, 30);
            ui.horizontal(|ui| {
                let label = ui.label(RichText::new(name).size(11.5).color(theme::text()));
                if customer.customer.chars().count() > 30 {
                    label.on_hover_text(&customer.customer);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format_amount(customer.amount, &data.currency))
                            .size(11.5)
                            .strong()
                            .color(theme::text()),
                    );
                    ui.label(
                        RichText::new(format!("{} inv", customer.invoices))
                            .size(9.5)
                            .color(theme::faint()),
                    );
                });
            });
            ui.add_space(4.0);
            ratio_bar(
                ui,
                customer.amount as f32 / maximum as f32,
                theme::accent().gamma_multiply(0.8),
            );
            if index < data.top_customers.len() - 1 {
                ui.add_space(11.0);
            }
        }
    });
}

fn aging_card(ui: &mut egui::Ui, data: &Analytics) {
    let labels = [
        "Current",
        "1–30 days",
        "31–60 days",
        "61–90 days",
        "90+ days",
    ];
    let colors = [
        theme::green(),
        theme::amber(),
        theme::amber(),
        theme::red(),
        theme::red(),
    ];

    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    RichText::new("Receivables aging")
                        .size(14.5)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new("Outstanding value by days past due")
                        .size(10.5)
                        .color(theme::muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format_amount(data.outstanding, &data.currency))
                        .size(12.0)
                        .strong()
                        .color(theme::amber()),
                );
            });
        });

        ui.add_space(14.0);

        if data.outstanding == 0 {
            ui.label(
                RichText::new("No outstanding receivables")
                    .size(11.0)
                    .color(theme::faint()),
            );
            return;
        }

        let maximum = data.aging_totals.iter().copied().max().unwrap_or(1);
        for (index, label) in labels.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(*label).size(11.0).color(theme::muted()));
                ui.label(
                    RichText::new(format!("{}", data.aging_counts[index]))
                        .size(9.5)
                        .color(theme::faint()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format_amount(data.aging_totals[index], &data.currency))
                            .size(11.0)
                            .strong()
                            .color(if data.aging_totals[index] > 0 {
                                theme::text()
                            } else {
                                theme::faint()
                            }),
                    );
                });
            });
            ui.add_space(4.0);
            ratio_bar(
                ui,
                data.aging_totals[index] as f32 / maximum as f32,
                colors[index],
            );
            if index < labels.len() - 1 {
                ui.add_space(8.0);
            }
        }
    });
}

fn manual_entry_card(ui: &mut egui::Ui, state: &mut DashboardState, storage: &Storage) {
    let Some(mut form) = state.manual_form.take() else {
        return;
    };

    let mut keep_open = true;
    let mut added = None;

    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    RichText::new("Add bill manually")
                        .size(15.0)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(
                        "Saved locally and included in the ledger and analytics immediately.",
                    )
                    .size(10.5)
                    .color(theme::muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::secondary_button(ui, "Close", 88.0).clicked() {
                    keep_open = false;
                }
            });
        });

        ui.add_space(12.0);

        if form.show(ui, "overview_manual_entry", "Add bill") {
            match form.build() {
                Ok(invoice) => match storage.insert_manual_invoice(&invoice) {
                    Ok(true) => {
                        form.error = None;
                        added = Some(invoice.invoice_no);
                    }
                    Ok(false) => {
                        form.error =
                            Some("A bill with the same identity already exists.".to_owned());
                    }
                    Err(error) => {
                        form.error = Some(format!("Could not add the bill: {error}"));
                    }
                },
                Err(error) => form.error = Some(error),
            }
        }
    });

    if let Some(invoice_no) = added {
        state.manual_notice = Some(format!(
            "Bill serial number {invoice_no} added to the ledger."
        ));
        keep_open = false;
    }

    if keep_open {
        state.manual_form = Some(form);
    }

    ui.add_space(16.0);
}

fn empty_workspace(ui: &mut egui::Ui, screen: &mut Screen) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.add_space(26.0);
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
            ui.painter()
                .circle_filled(icon.center(), 24.0, theme::accent().gamma_multiply(0.14));
            ui.painter().text(
                icon.center(),
                Align2::CENTER_CENTER,
                "↑",
                FontId::new(22.0, egui::FontFamily::Proportional),
                theme::accent(),
            );
            ui.add_space(12.0);
            ui.label(
                RichText::new("No billing data yet")
                    .size(18.0)
                    .strong()
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(
                    "Import your billing export or use Add bill above to unlock revenue trends, targets and receivables aging.",
                )
                .size(11.5)
                .color(theme::muted()),
            );
            ui.add_space(14.0);
            if ui::primary_button(ui, "Import billing data", 190.0).clicked() {
                *screen = Screen::Import;
            }
            ui.add_space(26.0);
        });
    });
}

fn empty_plot(ui: &mut egui::Ui, height: f32, message: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), theme::field());

    for line in 1..4 {
        let y = rect.top() + rect.height() * line as f32 / 4.0;
        ui.painter().line_segment(
            [
                egui::pos2(rect.left() + 12.0, y),
                egui::pos2(rect.right() - 12.0, y),
            ],
            Stroke::new(1.0, theme::border_soft()),
        );
    }

    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        message,
        FontId::new(12.5, egui::FontFamily::Proportional),
        theme::faint(),
    );
}

fn progress_bar(ui: &mut egui::Ui, ratio: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(5), theme::field());

    let width = rect.width() * ratio.clamp(0.0, 1.0);
    if width > 0.0 {
        let fill = egui::Rect::from_min_size(rect.min, Vec2::new(width.max(4.0), rect.height()));
        ui.painter().rect_filled(
            fill,
            CornerRadius::same(5),
            if ratio >= 1.0 {
                theme::green()
            } else {
                theme::accent()
            },
        );
    }
}

fn ratio_bar(ui: &mut egui::Ui, ratio: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), theme::border_soft());
    let width = rect.width() * ratio.clamp(0.0, 1.0);
    if width > 0.0 {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(width, rect.height())),
            CornerRadius::same(3),
            color,
        );
    }
}

fn segment(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let font = FontId::new(10.5, egui::FontFamily::Proportional);
    let width = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_no_wrap(label.to_owned(), font.clone(), theme::text()))
        .size()
        .x
        + 20.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());

    let fill = if selected {
        theme::accent_soft()
    } else if response.hovered() {
        theme::card_raised()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    if selected {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(7),
            Stroke::new(1.0, theme::accent().gamma_multiply(0.4)),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        if selected {
            theme::text()
        } else {
            theme::muted()
        },
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn legend_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, color);
}

fn status_color(index: usize) -> Color32 {
    match index {
        0 => theme::green(),
        1 => theme::amber(),
        _ => theme::red(),
    }
}

fn format_percent(value: f64) -> String {
    if value >= 100.0 || (value.fract()).abs() < 0.05 {
        format!("{value:.0}%")
    } else {
        format!("{value:.1}%")
    }
}

fn share(amount: i64, total: i64) -> String {
    if total == 0 {
        "—".to_owned()
    } else {
        format!("{:.0}%", amount as f64 / total as f64 * 100.0)
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

fn day_number(date: NaiveDate) -> f64 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or(date);
    (date - epoch).num_days() as f64
}

fn date_from_days(days: f64) -> Option<NaiveDate> {
    if !days.is_finite() || days < 0.0 || days > i64::MAX as f64 {
        return None;
    }
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    epoch.checked_add_days(Days::new(days.round() as u64))
}

fn date_axis_label(days: f64, range: &RangeInclusive<f64>, granularity: Granularity) -> String {
    let Some(date) = date_from_days(days) else {
        return String::new();
    };
    let span = range.end() - range.start();
    match granularity {
        Granularity::Day => {
            if span > 120.0 {
                date.format("%b %y").to_string()
            } else {
                date.format("%d %b").to_string()
            }
        }
        Granularity::Week => {
            if span > 180.0 {
                date.format("%b %y").to_string()
            } else {
                date.format("%d %b").to_string()
            }
        }
        Granularity::Month => {
            if span > 180.0 {
                date.format("%b %y").to_string()
            } else {
                date.format("%b").to_string()
            }
        }
        Granularity::Quarter => {
            format!("Q{} {}", (date.month0() / 3) + 1, date.format("%y"))
        }
    }
}

fn grid_marks(dates: Vec<NaiveDate>, max_labels: usize) -> Vec<GridMark> {
    if dates.is_empty() {
        return Vec::new();
    }
    let stride = (dates.len() / max_labels.max(1)).max(1);
    let sampled: Vec<NaiveDate> = dates
        .iter()
        .enumerate()
        .filter(|(index, _)| index % stride == 0)
        .map(|(_, date)| *date)
        .collect();

    sampled
        .iter()
        .enumerate()
        .map(|(index, date)| {
            let next = sampled.get(index + 1).copied().unwrap_or(*date);
            GridMark {
                value: day_number(*date),
                step_size: ((next - *date).num_days().max(1)) as f64,
            }
        })
        .collect()
}

fn axis_value(value: f64, metric: Metric, currency: &str) -> String {
    match metric {
        Metric::Invoices => format!("{:.0}", value),
        Metric::Revenue | Metric::Average => compact_money(value, currency),
    }
}

fn full_metric_value(value: f64, metric: Metric, currency: &str) -> String {
    match metric {
        Metric::Invoices => format!("{:.0} bills", value),
        Metric::Revenue | Metric::Average => {
            format_amount((value * 100.0).round() as i64, currency)
        }
    }
}

fn compact_money(value: f64, currency: &str) -> String {
    let prefix = currency_symbol(currency);
    let negative = value < 0.0;
    let absolute = value.abs();
    let body = if absolute >= 1_000_000.0 {
        format!("{:.1}M", absolute / 1_000_000.0)
    } else if absolute >= 1_000.0 {
        format!("{:.0}k", absolute / 1_000.0)
    } else {
        format!("{absolute:.0}")
    };
    format!("{}{prefix}{body}", if negative { "-" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;

    #[test]
    fn renders_dashboard_with_data() {
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
            .set_setting(TARGET_MONTHLY_KEY, "2500000")
            .expect("target saves");

        let mut state = DashboardState::default();
        let mut screen = Screen::Dashboard;
        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
        assert!(state.cache.is_some());
        assert!(state.cache.as_ref().unwrap().total_billed > 0);
    }

    #[test]
    fn renders_empty_dashboard() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = DashboardState::default();
        let mut screen = Screen::Dashboard;
        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }

    #[test]
    fn renders_manual_entry_on_overview() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = DashboardState {
            manual_form: Some(InvoiceForm::new("USD")),
            ..Default::default()
        };
        let mut screen = Screen::Dashboard;
        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }

    #[test]
    fn compacts_money_for_axes() {
        assert_eq!(compact_money(1234.0, "USD"), "$1k");
        assert_eq!(compact_money(1_260_000.0, "EUR"), "€1.3M");
        assert_eq!(compact_money(-500.0, "USD"), "-$500");
    }
}
