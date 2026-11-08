//! Directory of every customer/person, grouped from invoice records.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;
use egui::{
    Align, Align2, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::app::Screen;
use crate::db::Storage;
use crate::models::{Invoice, format_amount, format_date};
use crate::notes;
use crate::theme;
use crate::ui;

#[derive(Default)]
pub struct PeopleState {
    query: String,
    selected: Option<String>,
    notes_person: Option<String>,
    items: Vec<NoteItemRow>,
    notes_status: Option<(String, bool)>,
}

#[derive(Default, Clone)]
struct NoteItemRow {
    name: String,
    price: String,
}

struct Person {
    name: String,
    phones: Vec<String>,
    invoices: Vec<Invoice>,
    totals: BTreeMap<String, i64>,
    notes: String,
    extensions: i64,
}

pub fn show(ui: &mut egui::Ui, screen: &mut Screen, storage: &Storage, state: &mut PeopleState) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let invoices = match storage.all_invoices() {
                Ok(invoices) => invoices,
                Err(error) => {
                    ui::banner(ui, &format!("Could not load people: {error}"), theme::red());
                    return;
                }
            };

            let people = build_people(
                &invoices,
                &storage
                    .all_person_notes()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(customer, notes)| (customer.to_lowercase(), notes))
                    .collect(),
                &storage
                    .all_person_extensions()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(customer, count)| (customer.to_lowercase(), count))
                    .collect(),
            );
            let date_format = storage.date_format();

            ui::page_header(
                ui,
                "People",
                "Every customer and their billing history",
                |ui| {
                    ui::pill(ui, &format!("{} PEOPLE", people.len()), theme::green());
                },
            );

            if people.is_empty() {
                ui.add_space(18.0);
                empty_state(ui, screen);
                ui.add_space(24.0);
                return;
            }

            ui.add_space(16.0);

            let query = state.query.trim().to_lowercase();
            let visible: Vec<&Person> = people
                .iter()
                .filter(|person| {
                    query.is_empty()
                        || person.name.to_lowercase().contains(&query)
                        || person.phones.iter().any(|phone| phone.contains(&query))
                        || person.notes.to_lowercase().contains(&query)
                })
                .collect();

            theme::inset_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.query)
                            .hint_text("Search person by name or phone…")
                            .desired_width(320.0)
                            .margin(egui::Margin::symmetric(12, 9)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} OF {} PEOPLE", visible.len(), people.len()))
                                .size(10.0)
                                .strong()
                                .color(theme::faint()),
                        );
                    });
                });
            });

            if state
                .selected
                .as_ref()
                .is_none_or(|selected| !visible.iter().any(|person| person.name == *selected))
            {
                state.selected = visible.first().map(|person| person.name.clone());
            }

            if state.notes_person != state.selected {
                state.notes_person = state.selected.clone();
                state.items = load_note_items(storage, state.selected.as_deref());
                state.notes_status = None;
            }

            ui.add_space(14.0);

            if visible.is_empty() {
                theme::card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.add_space(22.0);
                        ui.label(
                            RichText::new("No person matches that search")
                                .size(14.0)
                                .strong()
                                .color(theme::text()),
                        );
                        ui.label(
                            RichText::new("Search again by name or Indian contact number.")
                                .size(11.0)
                                .color(theme::muted()),
                        );
                        ui.add_space(22.0);
                    });
                });
                return;
            }

            let total_width = ui.available_width();
            let gap = 16.0;
            let left_width = (total_width * 0.34).clamp(260.0, 340.0);
            let right_width = (total_width - left_width - gap).max(280.0);

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(left_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        theme::card_frame().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("Directory")
                                    .size(14.5)
                                    .strong()
                                    .color(theme::text()),
                            );
                            ui.label(
                                RichText::new("Select a person to see their bills.")
                                    .size(10.5)
                                    .color(theme::muted()),
                            );
                            ui.add_space(12.0);

                            for person in &visible {
                                let selected = state
                                    .selected
                                    .as_deref()
                                    .is_some_and(|name| name == person.name);
                                if person_row(ui, person, selected) {
                                    state.selected = Some(person.name.clone());
                                }
                                ui.add_space(6.0);
                            }
                        });
                    },
                );

                ui.add_space(gap);

                ui.allocate_ui_with_layout(
                    Vec2::new(right_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        let selected = state
                            .selected
                            .as_ref()
                            .and_then(|name| people.iter().find(|person| person.name == *name));
                        if let Some(person) = selected {
                            person_detail(ui, person, &date_format, state, storage);
                        } else {
                            theme::card_frame().show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.vertical_centered(|ui| {
                                    ui.add_space(28.0);
                                    ui.label(
                                        RichText::new("Select a person")
                                            .size(14.0)
                                            .strong()
                                            .color(theme::text()),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "Their contact details and bills will appear here.",
                                        )
                                        .size(11.0)
                                        .color(theme::muted()),
                                    );
                                    ui.add_space(28.0);
                                });
                            });
                        }
                    },
                );
            });

            ui.add_space(24.0);
        });
}

fn build_people(
    invoices: &[Invoice],
    notes: &BTreeMap<String, String>,
    extensions: &BTreeMap<String, i64>,
) -> Vec<Person> {
    let mut grouped: BTreeMap<String, Vec<Invoice>> = BTreeMap::new();
    for invoice in invoices {
        grouped
            .entry(invoice.customer.clone())
            .or_default()
            .push(invoice.clone());
    }

    grouped
        .into_iter()
        .map(|(name, mut invoices)| {
            invoices.sort_by(|left, right| {
                right
                    .issue_date
                    .cmp(&left.issue_date)
                    .then_with(|| right.id.cmp(&left.id))
            });

            let mut phones = BTreeSet::new();
            let mut totals = BTreeMap::new();
            for invoice in &invoices {
                if let Some(phone) = &invoice.customer_phone {
                    phones.insert(phone.clone());
                }
                *totals.entry(invoice.currency.clone()).or_insert(0i64) += invoice.amount_cents;
            }

            let key = name.to_lowercase();
            Person {
                notes: notes.get(&key).cloned().unwrap_or_default(),
                extensions: extensions.get(&key).copied().unwrap_or(0),
                name,
                phones: phones.into_iter().collect(),
                invoices,
                totals,
            }
        })
        .collect()
}

fn load_note_items(storage: &Storage, customer: Option<&str>) -> Vec<NoteItemRow> {
    let mut items = customer
        .and_then(|name| storage.person_notes(name).ok().flatten())
        .map(|notes| notes::parse(&notes))
        .unwrap_or_default()
        .into_iter()
        .map(|item| NoteItemRow {
            name: item.name,
            price: notes::price_to_input(item.price_cents),
        })
        .collect::<Vec<_>>();

    if items.is_empty() {
        items.push(NoteItemRow::default());
    }
    items
}

fn collect_note_items(rows: &[NoteItemRow]) -> Result<Vec<notes::NoteItem>, String> {
    let mut items = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let name = row.name.trim();
        if name.is_empty() && row.price.trim().is_empty() {
            continue;
        }
        if name.is_empty() {
            return Err(format!("Item {} needs a name.", index + 1));
        }
        let price_cents = notes::parse_price(&row.price)
            .map_err(|_| format!("Item {} has an invalid price.", index + 1))?;
        items.push(notes::NoteItem {
            name: name.to_owned(),
            price_cents,
        });
    }
    Ok(items)
}

fn person_row(ui: &mut egui::Ui, person: &Person, selected: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 82.0), Sense::click());

    let fill = if selected {
        theme::accent_soft()
    } else if response.hovered() {
        theme::card_hover()
    } else {
        theme::card_raised()
    };
    let stroke = if selected {
        Stroke::new(1.0, theme::accent().gamma_multiply(0.55))
    } else {
        Stroke::new(1.0, theme::border_soft())
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    painter.rect_stroke(rect, CornerRadius::same(10), stroke, StrokeKind::Inside);

    let name = truncate(&person.name, 28);
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.top() + 13.0),
        Align2::LEFT_TOP,
        name,
        FontId::new(12.5, egui::FontFamily::Proportional),
        theme::text(),
    );

    let phone = person
        .phones
        .first()
        .map(|phone| format_phone(phone))
        .unwrap_or_else(|| "No phone on file".to_owned());
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.top() + 35.0),
        Align2::LEFT_TOP,
        phone,
        FontId::new(10.5, egui::FontFamily::Proportional),
        if person.phones.is_empty() {
            theme::faint()
        } else {
            theme::muted()
        },
    );

    let total = primary_total(person);
    painter.text(
        egui::pos2(rect.right() - 12.0, rect.top() + 13.0),
        Align2::RIGHT_TOP,
        total,
        FontId::new(11.5, egui::FontFamily::Proportional),
        theme::text(),
    );
    painter.text(
        egui::pos2(rect.right() - 12.0, rect.top() + 35.0),
        Align2::RIGHT_TOP,
        format!("{} bills", person.invoices.len()),
        FontId::new(9.5, egui::FontFamily::Proportional),
        theme::faint(),
    );

    if !person.notes.trim().is_empty() {
        let preview: String = person
            .notes
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        painter.text(
            egui::pos2(rect.left() + 12.0, rect.top() + 57.0),
            Align2::LEFT_TOP,
            format!("✎ {}", truncate(&preview, 32)),
            FontId::new(9.0, egui::FontFamily::Proportional),
            theme::faint(),
        );
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn person_detail(
    ui: &mut egui::Ui,
    person: &Person,
    date_format: &str,
    state: &mut PeopleState,
    storage: &Storage,
) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            let initial = person
                .name
                .chars()
                .next()
                .map(|character| character.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_owned());
            let (avatar, _) =
                ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
            ui.painter()
                .circle_filled(avatar.center(), 21.0, theme::accent_soft());
            ui.painter().circle_stroke(
                avatar.center(),
                21.0,
                Stroke::new(1.0, theme::accent().gamma_multiply(0.35)),
            );
            ui.painter().text(
                avatar.center(),
                Align2::CENTER_CENTER,
                initial,
                FontId::new(17.0, egui::FontFamily::Proportional),
                theme::accent(),
            );

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new(&person.name)
                        .size(18.0)
                        .strong()
                        .color(theme::text()),
                );
                if person.phones.is_empty() {
                    ui.label(
                        RichText::new("No contact number on file")
                            .size(10.5)
                            .color(theme::faint()),
                    );
                } else {
                    ui.label(
                        RichText::new(
                            person
                                .phones
                                .iter()
                                .map(|phone| format_phone(phone))
                                .collect::<Vec<_>>()
                                .join("  ·  "),
                        )
                        .size(11.0)
                        .color(theme::muted()),
                    );
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut summary = format!("{} BILLS", person.invoices.len());
                if person.extensions > 0 {
                    summary.push_str(&format!(" · {} EXTENSIONS", person.extensions));
                }
                ui.label(
                    RichText::new(summary)
                        .size(9.5)
                        .strong()
                        .color(theme::faint()),
                );
            });
        });

        ui.add_space(14.0);

        ui.horizontal_wrapped(|ui| {
            for (currency, amount) in &person.totals {
                ui::pill(
                    ui,
                    &format!("{}  {}", currency, format_amount(*amount, currency)),
                    theme::accent(),
                );
            }
        });

        ui.add_space(12.0);

        ui.label(
            RichText::new("Products bought / notes")
                .size(12.5)
                .strong()
                .color(theme::text()),
        );
        ui.label(
            RichText::new(
                "Add each item in order with its price. Saved items appear as a numbered list in SMS messages.",
            )
            .size(10.0)
            .color(theme::faint()),
        );
        ui.add_space(8.0);

        let name_width = (ui.available_width() - 250.0).max(160.0);
        let mut save_notes = false;
        let mut add_item = false;
        let mut remove_item = None;

        for (index, row) in state.items.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}.", index + 1))
                        .size(10.5)
                        .color(theme::faint()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut row.name)
                        .hint_text("Item / product")
                        .desired_width(name_width)
                        .margin(Margin::symmetric(10, 7)),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut row.price)
                        .hint_text("Price")
                        .desired_width(110.0)
                        .margin(Margin::symmetric(10, 7)),
                );
                if ui
                    .link(RichText::new("Remove").size(9.5).color(theme::red()))
                    .clicked()
                {
                    remove_item = Some(index);
                }
            });
        }

        if let Some(index) = remove_item {
            if state.items.len() == 1 {
                state.items[0] = NoteItemRow::default();
            } else {
                state.items.remove(index);
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui::secondary_button(ui, "Add item", 100.0).clicked() {
                add_item = true;
            }
            ui.add_space(6.0);
            if ui::secondary_button(ui, "Save notes", 122.0).clicked() {
                save_notes = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("Saved only for this person")
                        .size(9.5)
                        .color(theme::faint()),
                );
            });
        });

        if add_item {
            state.items.push(NoteItemRow::default());
        }

        if save_notes {
            match collect_note_items(&state.items) {
                Ok(items) => {
                    let notes = notes::serialize(&items);
                    match storage.set_person_notes(&person.name, &notes) {
                        Ok(()) => {
                            state.notes_status = Some((
                                format!("Products and notes saved for {}.", person.name),
                                true,
                            ));
                        }
                        Err(error) => {
                            state.notes_status = Some((
                                format!("Could not save notes: {error}"),
                                false,
                            ));
                        }
                    }
                }
                Err(error) => state.notes_status = Some((error, false)),
            }
        }

        if let Some((message, success)) = &state.notes_status {
            ui.add_space(8.0);
            ui::banner(
                ui,
                message,
                if *success { theme::green() } else { theme::red() },
            );
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(10.0);
        ui.label(
            RichText::new("Bill history")
                .size(12.5)
                .strong()
                .color(theme::text()),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .max_height(420.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("person_bill_grid")
                    .num_columns(4)
                    .striped(true)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        for (index, label) in
                            ["BILL SERIAL NO.", "ISSUED", "AMOUNT", "STATUS"].iter().enumerate()
                        {
                            if index == 2 {
                                ui.with_layout(
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(*label)
                                                .size(9.0)
                                                .strong()
                                                .color(theme::faint()),
                                        );
                                    },
                                );
                            } else {
                                ui.label(
                                    RichText::new(*label)
                                        .size(9.0)
                                        .strong()
                                        .color(theme::faint()),
                                );
                            }
                        }
                        ui.end_row();

                        for invoice in &person.invoices {
                            ui.label(
                                RichText::new(&invoice.invoice_no)
                                    .size(10.0)
                                    .monospace()
                                    .color(theme::text()),
                            );
                            ui.label(
                                RichText::new(format_date(
                                    invoice.issue_date,
                                    date_format,
                                ))
                                .size(10.0)
                                .color(theme::muted()),
                            );
                            ui.with_layout(
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format_amount(
                                            invoice.amount_cents,
                                            &invoice.currency,
                                        ))
                                        .size(10.0)
                                        .strong()
                                        .color(theme::text()),
                                    );
                                },
                            );
                            ui::status_pill(ui, invoice.status);
                            ui.end_row();
                        }
                    });
            });

        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "People are created automatically from bill customers. Phone numbers come from manual entries or bill edits.",
            )
            .size(9.5)
            .color(theme::faint()),
        );
    });
}

fn primary_total(person: &Person) -> String {
    if let Some(amount) = person.totals.get("INR") {
        return format_amount(*amount, "INR");
    }
    person
        .totals
        .iter()
        .next()
        .map(|(currency, amount)| format_amount(*amount, currency))
        .unwrap_or_else(|| "—".to_owned())
}

fn format_phone(phone: &str) -> String {
    if let Some(national) = phone.strip_prefix("+91")
        && national.len() == 10
    {
        return format!("+91 {} {}", &national[..5], &national[5..]);
    }
    phone.to_owned()
}

fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_owned();
    }
    let mut result: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

fn empty_state(ui: &mut egui::Ui, screen: &mut Screen) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.add_space(26.0);
            ui.label(
                RichText::new("No people yet")
                    .size(18.0)
                    .strong()
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(
                    "People are created automatically when you import bills or add one from the Overview page.",
                )
                .size(11.5)
                .color(theme::muted()),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::primary_button(ui, "Import billing data", 176.0).clicked() {
                        *screen = Screen::Import;
                    }
                    ui.add_space(6.0);
                    if ui::secondary_button(ui, "Open overview", 132.0).clicked() {
                        *screen = Screen::Dashboard;
                    }
                });
            });
            ui.add_space(26.0);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;

    fn seeded_storage() -> Storage {
        let document = import::load_sample().expect("sample loads");
        let mapping = import::auto_mapping(document.headers());
        let plan = import::build_plan(&document, &mapping, "INR").expect("plan builds");
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
    }

    #[test]
    fn builds_item_rows_with_prices() {
        let rows = vec![
            NoteItemRow {
                name: "Software licence".to_owned(),
                price: "1,25,000".to_owned(),
            },
            NoteItemRow {
                name: "Support".to_owned(),
                price: String::new(),
            },
            NoteItemRow::default(),
        ];

        let items = collect_note_items(&rows).expect("items valid");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].price_cents, Some(12_500_000));
        assert_eq!(items[1].price_cents, None);

        let bad = vec![NoteItemRow {
            name: "Broken".to_owned(),
            price: "not a price".to_owned(),
        }];
        assert!(collect_note_items(&bad).is_err());
    }

    #[test]
    fn groups_invoices_into_people() {
        let storage = seeded_storage();
        let invoices = storage.all_invoices().expect("bills load");
        let customer = invoices[0].customer.clone();
        storage
            .set_person_notes(&customer, "Software licence")
            .unwrap();
        let notes = storage
            .all_person_notes()
            .expect("notes load")
            .into_iter()
            .map(|(customer, notes)| (customer.to_lowercase(), notes))
            .collect::<BTreeMap<_, _>>();
        let extensions = BTreeMap::new();
        let people = build_people(&invoices, &notes, &extensions);

        assert!(!people.is_empty());
        assert!(people.iter().all(|person| !person.invoices.is_empty()));
        assert!(people.iter().all(|person| !person.totals.is_empty()));

        let mut extensions = BTreeMap::new();
        extensions.insert(customer.to_lowercase(), 3);
        let people = build_people(&invoices, &notes, &extensions);
        assert!(
            people
                .iter()
                .any(|person| person.name == customer && person.extensions == 3)
        );
        assert!(
            people
                .iter()
                .any(|person| person.name == customer && person.notes == "Software licence")
        );
    }

    #[test]
    fn renders_people_directory() {
        let storage = seeded_storage();
        let mut state = PeopleState::default();
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
        assert!(state.selected.is_some());
    }

    #[test]
    fn renders_empty_people_directory() {
        let storage = Storage::open_in_memory().expect("database opens");
        let mut state = PeopleState::default();
        let mut screen = Screen::Dashboard;

        egui::__run_test_ui(|ui| {
            show(ui, &mut screen, &storage, &mut state);
        });
    }
}
