use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use chrono::Local;
use eframe::egui;
use egui::{
    Align, Align2, CornerRadius, CursorIcon, FontId, Layout, Margin, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Vec2,
};

use crate::app::Screen;
use crate::db::Storage;
use crate::import::{
    self, ColumnMapping, ImportDocument, ImportField, ImportPlan, ImportSource, RestConfig,
};
use crate::models::{format_amount, format_date};
use crate::theme;
use crate::ui;

#[derive(Default)]
pub struct ImportState {
    document: Option<ImportDocument>,
    mapping: ColumnMapping,
    plan: Option<ImportPlan>,
    result: Option<ImportResultView>,
    error: Option<String>,
    rest_url: String,
    rest_token: String,
    rest_headers: String,
    rest_json_path: String,
    rest_show_token: bool,
    rest_loading: bool,
    rest_receiver: Option<Receiver<Result<ImportDocument, String>>>,
}

#[derive(Clone)]
struct ImportResultView {
    file_name: String,
    imported: usize,
    skipped: usize,
    total_rows: usize,
    invalid_rows: usize,
    duplicate_rows: usize,
    existing_rows: usize,
}

impl ImportState {
    fn reset(&mut self) {
        self.document = None;
        self.mapping = ColumnMapping::default();
        self.plan = None;
        self.result = None;
        self.error = None;
    }

    fn apply_document(&mut self, document: ImportDocument) {
        self.mapping = import::auto_mapping(document.headers());
        self.document = Some(document);
        self.plan = None;
        self.result = None;
        self.error = None;
    }

    fn load_path(&mut self, path: &Path) {
        match import::read_file(path) {
            Ok(document) => self.apply_document(document),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn load_sample(&mut self) {
        match import::load_sample() {
            Ok(document) => self.apply_document(document),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn reparse_csv(&mut self, delimiter: u8, has_headers: bool) {
        let outcome = match self.document.as_mut().and_then(ImportDocument::as_csv_mut) {
            Some(document) => match document.set_options(delimiter, has_headers) {
                Ok(()) => Ok(import::auto_mapping(&document.headers)),
                Err(error) => Err(error.to_string()),
            },
            None => return,
        };

        match outcome {
            Ok(mapping) => {
                self.mapping = mapping;
                self.plan = None;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn set_excel_sheet(&mut self, sheet: usize) {
        let outcome = match self
            .document
            .as_mut()
            .and_then(ImportDocument::as_excel_mut)
        {
            Some(document) => match document.set_sheet(sheet) {
                Ok(()) => Ok(import::auto_mapping(&document.headers)),
                Err(error) => Err(error.to_string()),
            },
            None => return,
        };

        match outcome {
            Ok(mapping) => {
                self.mapping = mapping;
                self.plan = None;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn start_rest_fetch(&mut self) {
        if self.rest_loading {
            return;
        }

        let config = RestConfig {
            url: self.rest_url.clone(),
            token: self.rest_token.clone(),
            headers: self.rest_headers.clone(),
            json_path: self.rest_json_path.clone(),
        };

        let (sender, receiver) = std::sync::mpsc::channel();
        self.rest_loading = true;
        self.error = None;
        self.rest_receiver = Some(receiver);

        std::thread::spawn(move || {
            let result = import::fetch_rest(&config)
                .map(ImportDocument::Table)
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
    }

    fn poll_rest(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.rest_receiver else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(document)) => {
                self.rest_receiver = None;
                self.rest_loading = false;
                self.apply_document(document);
            }
            Ok(Err(error)) => {
                self.rest_receiver = None;
                self.rest_loading = false;
                self.error = Some(error);
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Err(TryRecvError::Disconnected) => {
                self.rest_receiver = None;
                self.rest_loading = false;
                self.error = Some("The API request stopped unexpectedly.".to_owned());
            }
        }
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut ImportState, storage: &Storage, screen: &mut Screen) {
    handle_dropped_files(ui, state, screen);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui::page_header(
                ui,
                "Import billing data",
                "Bring your billing exports into one workspace",
                |ui| {
                    ui::pill(ui, "CSV · EXCEL · JSON", theme::green());
                },
            );

            ui.add_space(18.0);

            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                file_tab(ui, state, storage, screen);
            });

            ui.add_space(16.0);
            import_history(ui, storage);
            ui.add_space(24.0);
        });
}

fn handle_dropped_files(ui: &mut egui::Ui, state: &mut ImportState, screen: &mut Screen) {
    let dropped = ui.ctx().input(|input| input.raw.dropped_files.clone());
    let Some(path) = dropped
        .into_iter()
        .map(|file| file.path().to_path_buf())
        .next()
    else {
        return;
    };

    *screen = Screen::Import;
    state.load_path(&path);
}

pub fn show_rest(
    ui: &mut egui::Ui,
    state: &mut ImportState,
    storage: &Storage,
    screen: &mut Screen,
) {
    let ctx = ui.ctx().clone();
    state.poll_rest(&ctx);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui::page_header(ui, "Developer settings", "REST API billing import", |ui| {
                if ui::secondary_button(ui, "Back to settings", 142.0).clicked() {
                    *screen = Screen::Settings;
                }
                ui.add_space(2.0);
                ui::pill(ui, "DEVELOPER", theme::amber());
            });

            ui.add_space(18.0);

            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(result) = state.result.clone() {
                    success_view(ui, &result, state, screen);
                } else if state.document.is_some() {
                    mapping_view(ui, state, storage, false);
                } else {
                    rest_tab(ui, state);
                }
            });

            ui.add_space(24.0);
        });
}

fn file_tab(ui: &mut egui::Ui, state: &mut ImportState, storage: &Storage, screen: &mut Screen) {
    if let Some(error) = &state.error {
        ui::banner(ui, error, theme::red());
        ui.add_space(10.0);
    }

    if let Some(result) = state.result.clone() {
        success_view(ui, &result, state, screen);
        return;
    }

    if state.document.is_none() {
        dropzone(ui, state);
        return;
    }

    mapping_view(ui, state, storage, true);
}

fn dropzone(ui: &mut egui::Ui, state: &mut ImportState) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 300.0), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);

    let painter = ui.painter();
    let fill = if response.hovered() {
        theme::card_raised()
    } else {
        theme::field()
    };
    painter.rect_filled(rect, CornerRadius::same(14), fill);
    ui::dashed_rect(
        painter,
        rect.shrink(1.0),
        theme::accent().gamma_multiply(if response.hovered() { 0.6 } else { 0.35 }),
        7.0,
        6.0,
    );

    let icon_center = Pos2::new(rect.center().x, rect.top() + 74.0);
    painter.circle_filled(icon_center, 29.0, theme::accent().gamma_multiply(0.14));
    painter.circle_stroke(
        icon_center,
        29.0,
        Stroke::new(1.0, theme::accent().gamma_multiply(0.35)),
    );
    painter.text(
        icon_center + Vec2::new(0.0, -1.0),
        Align2::CENTER_CENTER,
        "↑",
        FontId::new(26.0, egui::FontFamily::Proportional),
        theme::accent(),
    );

    painter.text(
        Pos2::new(rect.center().x, rect.top() + 124.0),
        Align2::CENTER_CENTER,
        "Drop a billing file here",
        FontId::new(17.0, egui::FontFamily::Proportional),
        theme::text(),
    );
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 150.0),
        Align2::CENTER_CENTER,
        "or click anywhere to browse · CSV, Excel or JSON",
        FontId::new(11.5, egui::FontFamily::Proportional),
        theme::muted(),
    );

    let chip = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + 196.0),
        Vec2::new(190.0, 34.0),
    );
    painter.rect_filled(chip, CornerRadius::same(8), theme::accent_soft());
    painter.rect_stroke(
        chip,
        CornerRadius::same(8),
        Stroke::new(1.0, theme::accent().gamma_multiply(0.4)),
        StrokeKind::Inside,
    );
    painter.text(
        chip.center(),
        Align2::CENTER_CENTER,
        "CLICK TO BROWSE FILES",
        FontId::new(10.5, egui::FontFamily::Proportional),
        theme::accent(),
    );

    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 30.0),
        Align2::CENTER_CENTER,
        "Up to 25 MB · rows are validated and de-duplicated before storage",
        FontId::new(10.0, egui::FontFamily::Proportional),
        theme::faint(),
    );

    if response.clicked() {
        pick_file(state);
    }

    ui.add_space(10.0);
    ui.vertical_centered(|ui| {
        if ui
            .link(
                RichText::new("Or load the bundled sample file")
                    .size(11.5)
                    .color(theme::accent()),
            )
            .clicked()
        {
            state.load_sample();
        }
        ui.label(
            RichText::new("CSV, Excel and JSON · UTF-8, Windows-1252 and UTF-16 supported")
                .size(10.0)
                .color(theme::faint()),
        );
    });
}

fn mapping_view(ui: &mut egui::Ui, state: &mut ImportState, storage: &Storage, allow_pick: bool) {
    let Some(document) = state.document.as_ref() else {
        return;
    };
    let file_name = document.file_name().to_owned();
    let headers = document.headers().to_vec();
    let data_rows = document.rows().len();
    let source = document.source();
    let source_label = document.source_label();
    let detail = document.detail().map(str::to_owned);
    let hover_path = document.path().map(|path| path.display().to_string());
    let csv_options = document
        .as_csv()
        .map(|csv| (csv.delimiter, csv.has_headers));
    let excel_options = document
        .as_excel()
        .map(|excel| (excel.sheets.clone(), excel.selected_sheet));

    let base_currency = storage.base_currency();
    let date_format = storage.date_format();

    let source_description = match source {
        ImportSource::Csv => csv_options
            .map(|(delimiter, has_headers)| {
                format!(
                    "{data_rows} data rows · {} separated · {}",
                    delimiter_label(delimiter),
                    if has_headers {
                        "first row is the header"
                    } else {
                        "no header row"
                    }
                )
            })
            .unwrap_or_else(|| format!("{data_rows} data rows")),
        ImportSource::Excel => format!(
            "{data_rows} data rows · Sheet: {}",
            detail.as_deref().unwrap_or("—")
        ),
        ImportSource::Json | ImportSource::Rest => format!(
            "{data_rows} records · {}",
            detail
                .as_deref()
                .unwrap_or("JSON records mapped from object keys")
        ),
    };

    let mut change_source = false;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.horizontal(|ui| {
                let name = ui.label(
                    RichText::new(&file_name)
                        .size(14.0)
                        .strong()
                        .color(theme::text()),
                );
                if let Some(path) = &hover_path {
                    name.on_hover_text(path);
                } else if let Some(detail) = &detail {
                    name.on_hover_text(detail);
                }
                ui::pill(ui, source_label, theme::accent());
            });
            ui.label(
                RichText::new(source_description)
                    .size(10.5)
                    .color(theme::muted()),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if allow_pick {
                "Choose another file"
            } else {
                "Change endpoint"
            };
            if ui::secondary_button(ui, label, 158.0).clicked() {
                change_source = true;
            }
        });
    });

    if change_source {
        if allow_pick {
            pick_file(state);
        } else {
            state.document = None;
            state.plan = None;
            state.error = None;
            state.result = None;
        }
        return;
    }

    ui.add_space(12.0);

    let mut new_has_headers = csv_options.map(|(_, has_headers)| has_headers);
    let mut new_delimiter = csv_options.map(|(delimiter, _)| delimiter);
    let mut new_sheet = excel_options.as_ref().map(|(_, selected)| *selected);
    let mut automap = false;

    theme::inset_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        if csv_options.is_some() {
            ui.horizontal(|ui| {
                if let Some(value) = new_has_headers.as_mut() {
                    ui.checkbox(
                        value,
                        RichText::new("First row contains headers")
                            .size(11.5)
                            .color(theme::text()),
                    );
                }
                ui.add_space(14.0);
                ui.label(RichText::new("Delimiter").size(10.5).color(theme::faint()));
                egui::ComboBox::from_id_salt("csv_delimiter")
                    .selected_text(new_delimiter.map(delimiter_label).unwrap_or("Comma"))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        if let Some(value) = new_delimiter.as_mut() {
                            for (delimiter, label) in import::DELIMITERS {
                                ui.selectable_value(value, delimiter, label);
                            }
                        }
                    });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::secondary_button(ui, "Auto-map columns", 150.0).clicked() {
                        automap = true;
                    }
                });
            });
        } else if let Some((sheets, selected)) = &excel_options {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sheet").size(10.5).color(theme::faint()));
                let selected = new_sheet.unwrap_or(*selected);
                egui::ComboBox::from_id_salt("excel_sheet")
                    .selected_text(sheets.get(selected).map(String::as_str).unwrap_or("Sheet"))
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for (index, sheet) in sheets.iter().enumerate() {
                            ui.selectable_value(&mut new_sheet, Some(index), sheet);
                        }
                    });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::secondary_button(ui, "Auto-map columns", 150.0).clicked() {
                        automap = true;
                    }
                });
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "JSON fields become columns; nested values are kept as JSON text.",
                    )
                    .size(10.5)
                    .color(theme::muted()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::secondary_button(ui, "Auto-map columns", 150.0).clicked() {
                        automap = true;
                    }
                });
            });
        }
    });

    if let (Some(delimiter), Some(has_headers), Some(original_delimiter), Some(original_headers)) = (
        new_delimiter,
        new_has_headers,
        csv_options.map(|(delimiter, _)| delimiter),
        csv_options.map(|(_, has_headers)| has_headers),
    ) && (delimiter != original_delimiter || has_headers != original_headers)
    {
        state.reparse_csv(delimiter, has_headers);
    }

    if let (Some(sheet), Some((_, original))) = (new_sheet, excel_options.as_ref())
        && sheet != *original
    {
        state.set_excel_sheet(sheet);
    }

    if automap && let Some(document) = state.document.as_ref() {
        state.mapping = import::auto_mapping(document.headers());
        state.plan = None;
    }

    ui.add_space(14.0);
    ui.label(
        RichText::new("Map source columns")
            .size(13.0)
            .strong()
            .color(theme::text()),
    );
    ui.label(
        RichText::new("Required fields are marked; the rest can use defaults.")
            .size(10.0)
            .color(theme::faint()),
    );
    ui.add_space(8.0);

    let mut mapping_changed = false;
    egui::Grid::new("import_mapping_grid")
        .num_columns(2)
        .spacing([28.0, 8.0])
        .show(ui, |ui| {
            for field in ImportField::ALL {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(field.label()).size(12.0).color(theme::text()));
                    if field.required() {
                        ui.label(RichText::new("required").size(9.0).color(theme::amber()));
                    } else {
                        ui.label(RichText::new("optional").size(9.0).color(theme::faint()));
                    }
                });

                let selected = state.mapping.get(field);
                let selected_text = selected
                    .and_then(|column| {
                        headers
                            .get(column)
                            .map(|header| format!("{}  ·  {header}", column + 1))
                    })
                    .unwrap_or_else(|| "Not mapped".to_owned());
                let mut new_selected = selected;

                egui::ComboBox::from_id_salt(format!("import_map_{}", field.index()))
                    .selected_text(RichText::new(selected_text).size(11.5).color(
                        if selected.is_some() {
                            theme::text()
                        } else {
                            theme::faint()
                        },
                    ))
                    .width(390.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut new_selected, None, "Not mapped");
                        for (column, header) in headers.iter().enumerate() {
                            ui.selectable_value(
                                &mut new_selected,
                                Some(column),
                                format!("{}  ·  {header}", column + 1),
                            );
                        }
                    });

                if new_selected != selected {
                    state.mapping.set(field, new_selected);
                    mapping_changed = true;
                }
                ui.end_row();
            }
        });

    if mapping_changed {
        state.plan = None;
    }

    ui.add_space(14.0);
    let mut validate = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Required: bill serial number, customer, issue date and amount")
                .size(10.0)
                .color(theme::faint()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if state.plan.is_some() {
                "Refresh preview"
            } else {
                "Validate & preview"
            };
            if ui::primary_button_enabled(ui, label, 186.0, state.mapping.required_complete())
                .clicked()
            {
                validate = true;
            }
        });
    });

    if validate {
        let result = state
            .document
            .as_ref()
            .ok_or_else(|| "No file is loaded.".to_owned())
            .and_then(|document| {
                import::build_plan(document, &state.mapping, &base_currency)
                    .map_err(|error| error.to_string())
            });

        match result {
            Ok(plan) => {
                state.plan = Some(plan);
                state.error = None;
            }
            Err(error) => state.error = Some(error),
        }
    }

    let mut import_clicked = false;
    if let Some(plan) = state.plan.as_ref() {
        ui.add_space(18.0);
        ui.separator();
        ui.add_space(14.0);

        ui.columns(3, |columns| {
            mini_stat(
                &mut columns[0],
                "READY TO IMPORT",
                &plan.valid_count().to_string(),
                theme::green(),
            );
            mini_stat(
                &mut columns[1],
                "NEEDS ATTENTION",
                &plan.errors.len().to_string(),
                if plan.errors.is_empty() {
                    theme::faint()
                } else {
                    theme::red()
                },
            );
            mini_stat(
                &mut columns[2],
                "DUPLICATES IN FILE",
                &plan.duplicate_rows.to_string(),
                if plan.duplicate_rows == 0 {
                    theme::faint()
                } else {
                    theme::amber()
                },
            );
        });

        if !plan.errors.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("Rows that need attention")
                    .size(12.0)
                    .strong()
                    .color(theme::red()),
            );
            egui::ScrollArea::vertical()
                .max_height(116.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for issue in plan.errors.iter().take(20) {
                        ui.label(
                            RichText::new(format!(
                                "Row {}  ·  {}",
                                issue.row_number, issue.message
                            ))
                            .size(10.5)
                            .color(theme::muted()),
                        );
                    }
                    if plan.errors.len() > 20 {
                        ui.label(
                            RichText::new(format!("… and {} more issues", plan.errors.len() - 20))
                                .size(10.0)
                                .color(theme::faint()),
                        );
                    }
                });
        }

        if !plan.candidates.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("Preview")
                    .size(12.0)
                    .strong()
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(format!(
                    "Showing {} of {} valid rows",
                    plan.candidates.len().min(8),
                    plan.candidates.len()
                ))
                .size(10.0)
                .color(theme::faint()),
            );
            ui.add_space(6.0);

            egui::Grid::new("import_preview_grid")
                .num_columns(6)
                .striped(true)
                .spacing([18.0, 7.0])
                .show(ui, |ui| {
                    for (index, label) in [
                        "BILL SERIAL NO.",
                        "CUSTOMER",
                        "ISSUED",
                        "DUE",
                        "AMOUNT",
                        "STATUS",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if index == 4 {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(*label)
                                        .size(9.5)
                                        .strong()
                                        .color(theme::faint()),
                                );
                            });
                        } else {
                            ui.label(
                                RichText::new(*label)
                                    .size(9.5)
                                    .strong()
                                    .color(theme::faint()),
                            );
                        }
                    }
                    ui.end_row();

                    for candidate in plan.candidates.iter().take(8) {
                        ui.label(
                            RichText::new(&candidate.invoice_no)
                                .size(10.5)
                                .monospace()
                                .color(theme::text()),
                        );
                        ui.label(
                            RichText::new(&candidate.customer)
                                .size(10.5)
                                .color(theme::text()),
                        );
                        ui.label(
                            RichText::new(format_date(candidate.issue_date, &date_format))
                                .size(10.5)
                                .color(theme::muted()),
                        );
                        ui.label(
                            RichText::new(
                                candidate
                                    .due_date
                                    .map(|date| format_date(date, &date_format))
                                    .unwrap_or_else(|| "—".to_owned()),
                            )
                            .size(10.5)
                            .color(theme::muted()),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format_amount(
                                    candidate.amount_cents,
                                    &candidate.currency,
                                ))
                                .size(10.5)
                                .color(theme::text()),
                            );
                        });
                        ui::status_pill(ui, candidate.status);
                        ui.end_row();
                    }
                });
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} valid · {} skipped · {} reviewed",
                    plan.valid_count(),
                    plan.rejected_count(),
                    plan.rows_total
                ))
                .size(10.5)
                .color(theme::faint()),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if plan.errors.is_empty() {
                    format!("Import {} bills", plan.valid_count())
                } else {
                    format!("Import {} valid rows", plan.valid_count())
                };
                if ui::primary_button_enabled(ui, &label, 206.0, plan.valid_count() > 0).clicked() {
                    import_clicked = true;
                }
            });
        });
    }

    if import_clicked {
        let (file_name, format) = state
            .document
            .as_ref()
            .map(|document| {
                (
                    document.file_name().to_owned(),
                    document.source().as_str().to_owned(),
                )
            })
            .unwrap_or_else(|| ("billing.csv".to_owned(), "csv".to_owned()));

        if let Some(plan) = state.plan.take() {
            let total_rows = plan.rows_total;
            let invalid_rows = plan.errors.len();
            let duplicate_rows = plan.duplicate_rows;

            match storage.insert_invoices(&plan.candidates, &file_name, &format, total_rows) {
                Ok(result) => {
                    state.result = Some(ImportResultView {
                        file_name,
                        imported: result.inserted,
                        skipped: total_rows.saturating_sub(result.inserted),
                        total_rows,
                        invalid_rows,
                        duplicate_rows,
                        existing_rows: result.skipped_existing,
                    });
                    state.error = None;
                }
                Err(error) => {
                    state.error = Some(format!("Could not save the import: {error}"));
                    state.plan = Some(plan);
                }
            }
        }
    }
}

fn success_view(
    ui: &mut egui::Ui,
    result: &ImportResultView,
    state: &mut ImportState,
    screen: &mut Screen,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(8.0);

        let (icon, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
        ui.painter()
            .circle_filled(icon.center(), 26.0, theme::green().gamma_multiply(0.14));
        ui.painter().circle_stroke(
            icon.center(),
            26.0,
            Stroke::new(1.0, theme::green().gamma_multiply(0.4)),
        );
        ui.painter().text(
            icon.center(),
            Align2::CENTER_CENTER,
            "✓",
            FontId::new(24.0, egui::FontFamily::Proportional),
            theme::green(),
        );

        ui.add_space(12.0);
        ui.label(
            RichText::new(if result.imported == 0 {
                "Nothing new to import"
            } else {
                "Import complete"
            })
            .size(20.0)
            .strong()
            .color(theme::text()),
        );
        ui.label(
            RichText::new(format!(
                "{} · {} rows reviewed",
                result.file_name, result.total_rows
            ))
            .size(11.0)
            .color(theme::muted()),
        );

        ui.add_space(18.0);
        ui.columns(3, |columns| {
            mini_stat(
                &mut columns[0],
                "IMPORTED",
                &result.imported.to_string(),
                theme::green(),
            );
            mini_stat(
                &mut columns[1],
                "SKIPPED",
                &result.skipped.to_string(),
                theme::amber(),
            );
            mini_stat(
                &mut columns[2],
                "TOTAL ROWS",
                &result.total_rows.to_string(),
                theme::faint(),
            );
        });

        let mut details = Vec::new();
        if result.invalid_rows > 0 {
            details.push(format!("{} invalid", result.invalid_rows));
        }
        if result.duplicate_rows > 0 {
            details.push(format!("{} duplicate in file", result.duplicate_rows));
        }
        if result.existing_rows > 0 {
            details.push(format!("{} already in ledger", result.existing_rows));
        }
        if !details.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(details.join(" · "))
                    .size(10.0)
                    .color(theme::faint()),
            );
        }

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::primary_button(ui, "View bills", 160.0).clicked() {
                    *screen = Screen::Invoices;
                }
                ui.add_space(6.0);
                if ui::secondary_button(ui, "Import another file", 170.0).clicked() {
                    state.reset();
                }
            });
        });
        ui.add_space(8.0);
    });
}

fn mini_stat(ui: &mut egui::Ui, label: &str, value: &str, accent: egui::Color32) {
    theme::inset_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(label)
                .size(9.5)
                .strong()
                .color(theme::faint()),
        );
        ui.add_space(2.0);
        ui.label(RichText::new(value).size(22.0).strong().color(accent));
    });
}

fn import_history(ui: &mut egui::Ui, storage: &Storage) {
    let total = storage.import_batch_count().unwrap_or(0);
    let batches = storage.recent_imports(8);

    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    RichText::new("Import history")
                        .size(14.5)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new("Every upload is logged with row-level results.")
                        .size(11.0)
                        .color(theme::muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui::pill(ui, &format!("{total} FILES"), theme::faint());
            });
        });

        ui.add_space(14.0);

        match batches {
            Err(error) => {
                ui::banner(ui, &format!("Could not read import history: {error}"), theme::red());
            }
            Ok(batches) if batches.is_empty() => {
                theme::inset_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new("No imports yet")
                                .size(13.0)
                                .strong()
                                .color(theme::text()),
                        );
                        ui.label(
                            RichText::new(
                                "Your first billing upload will appear here with its result summary.",
                            )
                            .size(11.0)
                            .color(theme::faint()),
                        );
                        ui.add_space(14.0);
                    });
                });
            }
            Ok(batches) => {
                egui::Grid::new("import_history_grid")
                    .num_columns(5)
                    .striped(true)
                    .spacing([26.0, 8.0])
                    .show(ui, |ui| {
                        for label in ["FILE", "ROWS", "IMPORTED", "SKIPPED", "WHEN"] {
                            ui.label(
                                RichText::new(label)
                                    .size(9.5)
                                    .strong()
                                    .color(theme::faint()),
                            );
                        }
                        ui.end_row();

                        for batch in batches {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&batch.filename)
                                        .size(10.5)
                                        .color(theme::text()),
                                );
                                ui::pill(ui, &batch.format.to_uppercase(), theme::accent());
                            });
                            ui.label(
                                RichText::new(batch.rows_total.to_string())
                                    .size(10.5)
                                    .color(theme::muted()),
                            );
                            ui.label(
                                RichText::new(batch.rows_imported.to_string())
                                    .size(10.5)
                                    .strong()
                                    .color(theme::green()),
                            );
                            ui.label(
                                RichText::new(batch.rows_skipped.to_string())
                                    .size(10.5)
                                    .color(if batch.rows_skipped > 0 {
                                        theme::amber()
                                    } else {
                                        theme::faint()
                                    }),
                            );
                            ui.label(
                                RichText::new(
                                    batch
                                        .imported_at
                                        .with_timezone(&Local)
                                        .format("%d %b %Y · %H:%M")
                                        .to_string(),
                                )
                                .size(10.5)
                                .color(theme::muted()),
                            );
                            ui.end_row();
                        }
                    });
            }
        }
    });
}

fn rest_tab(ui: &mut egui::Ui, state: &mut ImportState) {
    if let Some(error) = &state.error {
        ui::banner(ui, error, theme::red());
        ui.add_space(10.0);
    }

    ui.label(
        RichText::new("Test connection & fetch")
            .size(14.5)
            .strong()
            .color(theme::text()),
    );
    ui.label(
        RichText::new(
            "Fetch JSON from an OData service or any HTTP endpoint over GET, then map it through the same validation and dedupe pipeline.",
        )
        .size(11.0)
        .color(theme::muted()),
    );

    ui.add_space(14.0);

    ui.label(
        RichText::new("Endpoint URL")
            .size(11.0)
            .color(theme::muted()),
    );
    ui.add(
        egui::TextEdit::singleline(&mut state.rest_url)
            .hint_text("https://api.example.com/billing/bills")
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(12, 9)),
    );

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Bearer token")
                .size(11.0)
                .color(theme::muted()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.checkbox(
                &mut state.rest_show_token,
                RichText::new("Show").size(10.5).color(theme::faint()),
            );
        });
    });
    ui.add(
        egui::TextEdit::singleline(&mut state.rest_token)
            .hint_text("Optional · sent as Authorization: Bearer …")
            .password(!state.rest_show_token)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(12, 9)),
    );

    ui.add_space(10.0);
    ui.label(
        RichText::new("Extra headers")
            .size(11.0)
            .color(theme::muted()),
    );
    ui.add(
        egui::TextEdit::multiline(&mut state.rest_headers)
            .hint_text("One per line\nX-Api-Key: your-key\nAccept: application/json")
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .margin(Margin::symmetric(12, 9)),
    );

    ui.add_space(10.0);
    ui.label(
        RichText::new("JSON path (optional)")
            .size(11.0)
            .color(theme::muted()),
    );
    ui.add(
        egui::TextEdit::singleline(&mut state.rest_json_path)
            .hint_text("e.g. data.items — leave blank to auto-detect the first array")
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(12, 9)),
    );

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        let can_fetch = !state.rest_loading && !state.rest_url.trim().is_empty();
        if ui::primary_button_enabled(
            ui,
            if state.rest_loading {
                "Fetching…"
            } else {
                "Test connection & fetch"
            },
            210.0,
            can_fetch,
        )
        .clicked()
        {
            state.start_rest_fetch();
        }

        if state.rest_loading {
            ui.add(egui::Spinner::new().size(18.0));
            ui.label(
                RichText::new("Contacting the API on a background thread…")
                    .size(10.5)
                    .color(theme::muted()),
            );
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new("Tokens stay in memory for this session only.")
                    .size(10.0)
                    .color(theme::faint()),
            );
        });
    });

    ui.add_space(12.0);
    theme::inset_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(
                "The response must be a JSON array, or an object containing one. Arrays of objects use their keys as columns; arrays of arrays and JSON paths are supported.",
            )
            .size(10.5)
            .color(theme::muted()),
        );
    });
}

fn delimiter_label(delimiter: u8) -> &'static str {
    import::DELIMITERS
        .iter()
        .find(|(value, _)| *value == delimiter)
        .map(|(_, label)| *label)
        .unwrap_or("Custom")
}

fn pick_file(state: &mut ImportState) {
    if let Some(path) = rfd::FileDialog::new()
        .set_title("Import billing data")
        .add_filter(
            "Billing exports",
            &["csv", "tsv", "txt", "xlsx", "xls", "xlsb", "ods", "json"],
        )
        .pick_file()
    {
        state.load_path(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_mapping_and_preview_without_panicking() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = ImportState::default();
        state.load_sample();
        state.plan = state.document.as_ref().and_then(|document| {
            import::build_plan(document, &state.mapping, crate::models::DEFAULT_CURRENCY).ok()
        });
        assert!(state.plan.is_some());

        let mut screen = Screen::Dashboard;
        egui::__run_test_ui(|ui| {
            show(ui, &mut state, &storage, &mut screen);
        });
    }

    #[test]
    fn renders_empty_importer() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = ImportState::default();
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut state, &storage, &mut screen);
        });
    }

    #[test]
    fn renders_import_result() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = ImportState {
            result: Some(ImportResultView {
                file_name: "billing_sample.csv".to_owned(),
                imported: 20,
                skipped: 4,
                total_rows: 24,
                invalid_rows: 1,
                duplicate_rows: 1,
                existing_rows: 2,
            }),
            ..Default::default()
        };
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut state, &storage, &mut screen);
        });
    }

    #[test]
    fn renders_rest_developer_screen() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = ImportState::default();
        let mut screen = Screen::Settings;

        egui::__run_test_ui(|ui| {
            show_rest(ui, &mut state, &storage, &mut screen);
        });
    }

    #[test]
    fn renders_json_mapping() {
        let storage = Storage::open_in_memory().expect("database opens");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("samples")
            .join("billing_sample.json");
        let document = import::read_file(&path).expect("JSON sample loads");

        let mut state = ImportState::default();
        state.apply_document(document);
        state.plan = state.document.as_ref().and_then(|document| {
            import::build_plan(document, &state.mapping, crate::models::DEFAULT_CURRENCY).ok()
        });
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut state, &storage, &mut screen);
        });
    }
}
