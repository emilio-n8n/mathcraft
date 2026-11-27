//! MathCraft application UI and project lifecycle.

use std::path::PathBuf;

use craftkit_io::DocumentHost;
use craftkit_ui::cli::CliOptions;
use craftkit_ui::eframe::{self, egui};
use craftkit_ui::egui::{Align, Layout, RichText};
use craftkit_ui::{AppIdentity, AppShell, Tone, dialogs, theme, widgets};

use mathcraft::document::{Document, NamedMatrix, NamedSeries, SCHEMA_VERSION, Sheet};
use mathcraft::engine::csv;
use mathcraft::engine::expr::{self, FUNCTION_NAMES};
use mathcraft::engine::matrix::{self, MatrixError};
use mathcraft::engine::plot::{self, PlotOptions, Sample};
use mathcraft::engine::raster;
use mathcraft::engine::sheet;

const FORMAT: &str = "mathcraft";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const HISTORY_LIMIT: usize = 100;
const PLOT_SAMPLES: usize = 401;
const PLOT_HEIGHT: f32 = 260.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    New,
    Open,
    Save,
    SaveAs,
    ExportSheet,
    ExportVariables,
    Undo,
    Redo,
    About,
    Settings,
}

#[derive(Debug, Clone)]
enum PendingAction {
    New,
    Open(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatrixAction {
    Determinant,
    Transpose,
    Product,
    Add,
    Subtract,
}

/// The application state.
pub struct MathApp {
    shell: AppShell,
    host: DocumentHost<Document>,
    new_variable_name: String,
    new_variable_value: f64,
    matrix_a: String,
    matrix_b: String,
    matrix_name: String,
    matrix_result: Option<Result<String, String>>,
    sheet_name_input: String,
    selected_cell: usize,
    plot_x_min: f64,
    plot_x_max: f64,
    plot_y_min: Option<f64>,
    plot_y_max: Option<f64>,
    show_all_curves: bool,
    csv_text: String,
    series_name: String,
    import_status: Option<Result<String, String>>,
    pending: Option<PendingAction>,
    gesture_active: bool,
}

impl MathApp {
    /// Build the application, opening the startup project when requested.
    pub fn new(context: &eframe::CreationContext<'_>, options: CliOptions) -> Self {
        let identity = AppIdentity {
            id: "mathcraft",
            name: "MathCraft",
            version: APP_VERSION,
            tagline: "Scientific calculator, matrices and 2D plots",
            repository: Some("https://github.com/emilio-n8n/mathcraft"),
        };
        let shell = AppShell::new(identity);
        let document = if options.sample {
            Document::sample()
        } else {
            Document::default()
        };
        let mut app = Self {
            shell,
            host: DocumentHost::new(document, FORMAT, SCHEMA_VERSION, APP_VERSION),
            new_variable_name: String::new(),
            new_variable_value: 0.0,
            matrix_a: "1 2; 3 4".to_owned(),
            matrix_b: "5 6; 7 8".to_owned(),
            matrix_name: "A".to_owned(),
            matrix_result: None,
            sheet_name_input: String::new(),
            selected_cell: 0,
            plot_x_min: -10.0,
            plot_x_max: 10.0,
            plot_y_min: None,
            plot_y_max: None,
            show_all_curves: true,
            csv_text: String::new(),
            series_name: "series".to_owned(),
            import_status: None,
            pending: None,
            gesture_active: false,
        };
        app.shell.install_theme(&context.egui_ctx);
        if let Some(path) = options.open {
            app.load_path(path);
        }
        app
    }

    fn load_path(&mut self, path: PathBuf) {
        match DocumentHost::<Document>::from_file(path.clone(), FORMAT, SCHEMA_VERSION, APP_VERSION)
        {
            Ok(mut host) => {
                let mut document = host.history_mut().current_mut().clone();
                document.normalize();
                refresh_document(&mut document);
                host.history_mut().replace(document);
                self.host = host;
                self.gesture_active = false;
                self.selected_cell = 0;
                self.shell.set_status(format!("Opened {}", path.display()));
            }
            Err(error) => {
                self.shell
                    .notify(Tone::Error, "Could not open project", error.to_string())
            }
        }
    }

    fn new_document(&mut self) {
        self.host.reset(Document::default());
        self.gesture_active = false;
        self.selected_cell = 0;
        self.matrix_result = None;
        self.shell.set_status("New document");
    }

    fn finish_gesture(&mut self) {
        if self.gesture_active {
            self.host.history_mut().end_gesture();
            self.gesture_active = false;
        }
    }

    fn apply(&mut self, mutate: impl FnOnce(&mut Document)) -> bool {
        self.finish_gesture();
        self.host.apply(mutate)
    }

    fn save(&mut self) -> bool {
        self.finish_gesture();
        if self.host.path().is_none() {
            return self.save_as();
        }
        self.refresh_results(false);
        self.host.apply(Document::sync_legacy_expression);
        match self.host.save() {
            Ok(path) => {
                self.shell.set_status(format!("Saved {}", path.display()));
                true
            }
            Err(error) => {
                self.shell
                    .notify(Tone::Error, "Could not save project", error.to_string());
                false
            }
        }
    }

    fn save_as(&mut self) -> bool {
        self.finish_gesture();
        let default_name = format!("{}.json", slugify(&self.host.document().title));
        let Some(path) = dialogs::save_project("Save project", &default_name, "json") else {
            return false;
        };
        self.refresh_results(false);
        self.host.apply(Document::sync_legacy_expression);
        match self.host.save_as(path) {
            Ok(path) => {
                self.shell.set_status(format!("Saved {}", path.display()));
                true
            }
            Err(error) => {
                self.shell
                    .notify(Tone::Error, "Could not save project", error.to_string());
                false
            }
        }
    }

    fn request_new(&mut self) {
        self.finish_gesture();
        if self.host.is_dirty() {
            self.pending = Some(PendingAction::New);
        } else {
            self.new_document();
        }
    }

    fn request_open(&mut self) {
        self.finish_gesture();
        let Some(path) = dialogs::open_project("Open project", &["json"]) else {
            return;
        };
        if self.host.is_dirty() {
            self.pending = Some(PendingAction::Open(path));
        } else {
            self.load_path(path);
        }
    }

    fn perform(&mut self, action: PendingAction) {
        match action {
            PendingAction::New => self.new_document(),
            PendingAction::Open(path) => self.load_path(path),
        }
    }

    fn resolve_pending(&mut self, choice: PendingChoice) {
        let Some(action) = self.pending.take() else {
            return;
        };
        match choice {
            PendingChoice::Cancel => {}
            PendingChoice::Discard => self.perform(action),
            PendingChoice::Save => {
                if self.save() {
                    self.perform(action);
                } else {
                    self.pending = Some(action);
                }
            }
        }
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::New => self.request_new(),
            Action::Open => self.request_open(),
            Action::Save => {
                self.save();
            }
            Action::SaveAs => {
                self.save_as();
            }
            Action::ExportSheet => self.export_sheet_csv(),
            Action::ExportVariables => self.export_variables_csv(),
            Action::Undo => {
                self.finish_gesture();
                if self.host.undo() {
                    self.clamp_selected_cell();
                }
            }
            Action::Redo => {
                self.finish_gesture();
                if self.host.redo() {
                    self.clamp_selected_cell();
                }
            }
            Action::About => self.shell.open_about(),
            Action::Settings => self.shell.open_settings(),
        }
    }

    fn clamp_selected_cell(&mut self) {
        let count = self
            .host
            .document()
            .active_sheet()
            .map_or(0, |sheet| sheet.cells.len());
        self.selected_cell = if count == 0 {
            0
        } else {
            self.selected_cell.min(count - 1)
        };
    }

    fn handle_shortcuts(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let action = ui.input_mut(|input| {
            if input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            ) {
                Some(Action::SaveAs)
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::S) {
                Some(Action::Save)
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::O) {
                Some(Action::Open)
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::N) {
                Some(Action::New)
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                Some(Action::Undo)
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y) {
                Some(Action::Redo)
            } else {
                None
            }
        });
        if let Some(action) = action {
            self.handle_action(action);
        }
    }

    /// Variables visible to the active plot, computed from fresh workbook
    /// results so cross-sheet references never use stale persisted values.
    fn evaluation_variables(&self) -> Vec<(String, f64)> {
        let document = self.host.document();
        let outcomes = sheet::evaluate_workbook(&document.sheets, &document.variables);
        let mut variables = document.variables.clone();
        for (sheet, outcomes) in document.sheets.iter().zip(&outcomes) {
            if let Some(value) = outcomes.iter().find_map(|outcome| outcome.value) {
                variables.push((sheet.name.clone(), value));
            }
            for (index, outcome) in outcomes.iter().enumerate() {
                if let Some(value) = outcome.value {
                    variables.push((
                        format!("{}.{}", sheet.name, mathcraft::document::Cell::name(index)),
                        value,
                    ));
                    if document
                        .active_sheet()
                        .is_some_and(|active| active.name == sheet.name)
                    {
                        variables.push((mathcraft::document::Cell::name(index), value));
                        variables
                            .push((mathcraft::document::Cell::name(index).to_lowercase(), value));
                    }
                }
            }
        }
        variables
    }

    fn refresh_results(&mut self, record_expressions: bool) {
        let mut document = self.host.document().clone();
        refresh_document(&mut document);
        let expressions = if record_expressions {
            document
                .active_sheet()
                .map(|sheet| {
                    sheet
                        .cells
                        .iter()
                        .filter(|cell| !cell.expression.trim().is_empty())
                        .map(|cell| cell.expression.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        self.apply(|current| {
            *current = document;
            for expression in expressions {
                record_history(current, expression);
            }
        });
    }

    fn evaluate_sheet(&mut self) {
        self.refresh_results(true);
    }

    fn run_matrix_action(&mut self, action: MatrixAction) {
        let left = match matrix::parse_matrix(&self.matrix_a) {
            Ok(matrix) => matrix,
            Err(error) => {
                self.matrix_result = Some(Err(error.to_string()));
                return;
            }
        };
        let outcome = match action {
            MatrixAction::Determinant => left.det().map(expr::format_value),
            MatrixAction::Transpose => Ok(left.transpose().to_string()),
            MatrixAction::Product => matrix::parse_matrix(&self.matrix_b)
                .and_then(|right| left.mul(&right).map(|product| product.to_string())),
            MatrixAction::Add => matrix::parse_matrix(&self.matrix_b)
                .and_then(|right| left.add(&right).map(|sum| sum.to_string())),
            MatrixAction::Subtract => matrix::parse_matrix(&self.matrix_b)
                .and_then(|right| left.sub(&right).map(|difference| difference.to_string())),
        };
        self.matrix_result = Some(outcome.map_err(|error: MatrixError| error.to_string()));
    }

    fn save_current_matrix(&mut self) {
        let name = self.matrix_name.trim().to_owned();
        if name.is_empty() {
            self.matrix_result = Some(Err("Enter a matrix name first".to_owned()));
            return;
        }
        let parsed = match matrix::parse_matrix(&self.matrix_a) {
            Ok(matrix) => matrix,
            Err(error) => {
                self.matrix_result = Some(Err(error.to_string()));
                return;
            }
        };
        self.apply(|document| {
            if let Some(entry) = document
                .matrices
                .iter_mut()
                .find(|entry| entry.name == name)
            {
                entry.matrix = parsed;
            } else {
                document.matrices.push(NamedMatrix {
                    name: name.clone(),
                    matrix: parsed,
                });
            }
        });
        self.matrix_result = Some(Ok(format!("Matrix '{name}' saved")));
    }

    fn import_csv(&mut self) {
        let name = self.series_name.trim();
        let name = if name.is_empty() { "series" } else { name };
        let imported = match csv::parse_series(&self.csv_text, name) {
            Ok(series) => series,
            Err(error) => {
                self.import_status = Some(Err(error.to_string()));
                return;
            }
        };
        let point_count = imported
            .iter()
            .map(|(_, points)| points.len())
            .sum::<usize>();
        let series_count = imported.len();
        self.apply(|document| {
            for (name, points) in imported {
                if let Some(series) = document
                    .series
                    .iter_mut()
                    .find(|series| series.name.eq_ignore_ascii_case(&name))
                {
                    series.points = points;
                } else {
                    document.series.push(NamedSeries { name, points });
                }
            }
        });
        self.import_status = Some(Ok(format!(
            "Imported {point_count} points into {series_count} series"
        )));
    }

    fn export_sheet_csv(&mut self) {
        self.finish_gesture();
        let mut document = self.host.document().clone();
        refresh_document(&mut document);
        let Some(sheet) = document.active_sheet() else {
            return;
        };
        let rows = sheet
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                vec![
                    mathcraft::document::Cell::name(index),
                    cell.expression.clone(),
                    cell.result.map(expr::format_value).unwrap_or_default(),
                    cell.error.clone().unwrap_or_default(),
                ]
            })
            .collect::<Vec<_>>();
        let text = csv::write_rows(&["cell", "expression", "result", "error"], &rows);
        self.write_export(
            "Export sheet CSV",
            "sheet.csv",
            "csv",
            &text,
            "sheet results",
        );
    }

    fn export_variables_csv(&mut self) {
        let text = csv::export_variables(&self.host.document().variables);
        self.write_export(
            "Export variables CSV",
            "variables.csv",
            "csv",
            &text,
            "variables",
        );
    }

    fn write_export(
        &mut self,
        title: &str,
        default: &str,
        extension: &str,
        text: &str,
        what: &str,
    ) {
        let Some(path) = dialogs::save_project(title, default, extension) else {
            return;
        };
        match std::fs::write(&path, text) {
            Ok(()) => self
                .shell
                .set_status(format!("Exported {what} to {}", path.display())),
            Err(error) => self
                .shell
                .notify(Tone::Error, "Could not export", error.to_string()),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let title = self.host.display_name();
        let dirty = self.host.is_dirty();
        let can_undo = self.host.can_undo();
        let can_redo = self.host.can_redo();
        let mut action = None;
        let mut example = None;
        self.shell.top_bar(ui, &title, dirty, |ui| {
            ui.menu_button("File", |ui| {
                for (label, candidate) in [
                    ("New", Action::New),
                    ("Open...", Action::Open),
                    ("Save", Action::Save),
                    ("Save As...", Action::SaveAs),
                    ("Export sheet CSV", Action::ExportSheet),
                    ("Export variables CSV", Action::ExportVariables),
                ] {
                    if ui.button(label).clicked() {
                        action = Some(candidate);
                        ui.close();
                    }
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo"))
                    .clicked()
                {
                    action = Some(Action::Undo);
                    ui.close();
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("Redo"))
                    .clicked()
                {
                    action = Some(Action::Redo);
                    ui.close();
                }
            });
            ui.menu_button("Examples", |ui| {
                for (label, cells) in [
                    (
                        "Trigonometry",
                        vec!["sin(x)", "cos(x)", "sin(x)^2 + cos(x)^2"],
                    ),
                    ("Quadratic", vec!["x^2 - 4", "sqrt(16)", "mean(1,2,3,4)"]),
                    ("Scope demo", vec!["2+3*4", "A1 * 2", "pi r^2"]),
                ] {
                    if ui.button(label).clicked() {
                        example = Some(cells);
                        ui.close();
                    }
                }
            });
            if ui.button("About").clicked() {
                action = Some(Action::About);
            }
            if ui.button("Settings").clicked() {
                action = Some(Action::Settings);
            }
        });
        if let Some(cells) = example {
            self.load_example(&cells);
        }
        if let Some(action) = action {
            self.handle_action(action);
        }
    }

    fn load_example(&mut self, expressions: &[&str]) {
        let expressions = expressions
            .iter()
            .map(|expression| (*expression).to_owned())
            .collect::<Vec<_>>();
        self.apply(|document| {
            if let Some(sheet) = document.active_sheet_mut() {
                sheet.cells = expressions
                    .iter()
                    .map(|expression| mathcraft::document::Cell::new(expression.clone()))
                    .collect();
            }
            document.sync_legacy_expression();
        });
        self.selected_cell = 0;
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        self.shell.status_bar(ui);
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        let mut remove_variable = None;
        let mut add_variable = false;
        let mut remove_matrix = None;
        let mut load_matrix_a = None;
        let mut load_matrix_b = None;
        let mut remove_series = None;
        let mut picked_history = None;
        let mut variable_focus_gained = false;
        let mut variable_focus_lost = false;

        egui::Panel::left("notebook-panel")
            .resizable(true)
            .default_size(260.0)
            .min_size(210.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        widgets::section_label(ui, "Variables");
                        // Snapshot before egui can mutate a field. Starting a
                        // gesture is idempotent, so focus moves within this
                        // panel remain one undo step.
                        self.host.history_mut().begin_gesture();
                        for (index, (name, value)) in self
                            .host
                            .history_mut()
                            .current_mut()
                            .variables
                            .iter_mut()
                            .enumerate()
                        {
                            ui.horizontal(|ui| {
                                let previous_value = *value;
                                let name_response =
                                    ui.add(egui::TextEdit::singleline(name).desired_width(78.0));
                                let value_response = ui.add(egui::DragValue::new(value).speed(0.1));
                                if !value.is_finite() {
                                    *value = previous_value;
                                    self.shell.notify(
                                        Tone::Error,
                                        "Invalid variable",
                                        "Variable values must be finite",
                                    );
                                }
                                variable_focus_gained |=
                                    name_response.gained_focus() || value_response.gained_focus();
                                variable_focus_lost |=
                                    name_response.lost_focus() || value_response.lost_focus();
                                if ui.small_button("x").clicked() {
                                    remove_variable = Some(index);
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.new_variable_name)
                                    .hint_text("name")
                                    .desired_width(78.0),
                            );
                            ui.add(egui::DragValue::new(&mut self.new_variable_value).speed(0.1));
                            let enter = response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            if ui.button("Add").clicked() || enter {
                                add_variable = true;
                            }
                        });
                        if let Some(index) = remove_variable {
                            self.apply(|document| {
                                document.variables.remove(index);
                            });
                        }
                        if add_variable {
                            let name = self.new_variable_name.trim().to_owned();
                            let value = self.new_variable_value;
                            if name.is_empty() || !expr::is_identifier(&name) {
                                self.shell.notify(
                                    Tone::Error,
                                    "Invalid variable name",
                                    "Use letters, digits, underscores or dotted names",
                                );
                            } else if !value.is_finite() {
                                self.shell.notify(
                                    Tone::Error,
                                    "Invalid variable",
                                    "Variable values must be finite",
                                );
                            } else {
                                self.apply(|document| {
                                    if let Some(entry) = document
                                        .variables
                                        .iter_mut()
                                        .rev()
                                        .find(|(existing, _)| existing == &name)
                                    {
                                        entry.1 = value;
                                    } else {
                                        document.variables.push((name.clone(), value));
                                    }
                                });
                                self.new_variable_name.clear();
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        widgets::section_label(ui, "Saved matrices");
                        let matrices = self.host.document().matrices.clone();
                        if matrices.is_empty() {
                            ui.label(RichText::new("No saved matrices").color(theme::FAINT));
                        }
                        for (index, entry) in matrices.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&entry.name).monospace());
                                ui.label(
                                    RichText::new(format!(
                                        "{}x{}",
                                        entry.matrix.rows(),
                                        entry.matrix.cols()
                                    ))
                                    .color(theme::MUTED),
                                );
                                if ui.small_button("A").clicked() {
                                    load_matrix_a = Some(entry.matrix.to_string());
                                }
                                if ui.small_button("B").clicked() {
                                    load_matrix_b = Some(entry.matrix.to_string());
                                }
                                if ui.small_button("x").clicked() {
                                    remove_matrix = Some(index);
                                }
                            });
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        widgets::section_label(ui, "Imported series");
                        let series = self.host.document().series.clone();
                        if series.is_empty() {
                            ui.label(
                                RichText::new("Import CSV to add a curve").color(theme::FAINT),
                            );
                        }
                        for (index, entry) in series.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} ({} pts)",
                                        entry.name,
                                        entry.points.len()
                                    ))
                                    .monospace(),
                                );
                                if ui.small_button("x").clicked() {
                                    remove_series = Some(index);
                                }
                            });
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        widgets::section_label(ui, "History");
                        let history = self.host.document().history.clone();
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .max_height(180.0)
                            .show(ui, |ui| {
                                if history.is_empty() {
                                    ui.label(
                                        RichText::new("Nothing evaluated yet").color(theme::FAINT),
                                    );
                                }
                                for expression in history.iter().rev() {
                                    if ui.selectable_label(false, expression).clicked() {
                                        picked_history = Some(expression.clone());
                                    }
                                }
                            });
                        ui.add_space(8.0);
                        ui.separator();
                        self.csv_panel(ui);
                    });
            });

        if let Some(text) = load_matrix_a {
            self.matrix_a = text;
            self.matrix_result = Some(Ok("Matrix loaded into A".to_owned()));
        }
        if let Some(text) = load_matrix_b {
            self.matrix_b = text;
            self.matrix_result = Some(Ok("Matrix loaded into B".to_owned()));
        }
        if let Some(index) = remove_matrix {
            self.apply(|document| {
                document.matrices.remove(index);
            });
        }
        if let Some(index) = remove_series {
            self.apply(|document| {
                document.series.remove(index);
            });
        }
        if let Some(expression) = picked_history {
            let index = self.selected_cell;
            self.apply(|document| {
                if let Some(cell) = document
                    .active_sheet_mut()
                    .and_then(|sheet| sheet.cells.get_mut(index))
                {
                    cell.expression = expression;
                }
            });
        }
        if variable_focus_gained {
            self.gesture_active = true;
        }
        if variable_focus_lost && !variable_focus_gained {
            self.finish_gesture();
        }
    }

    fn sheet_panel(&mut self, ui: &mut egui::Ui, evaluate_requested: &mut bool) {
        ui.horizontal(|ui| {
            ui.heading("Notebook");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Evaluate").clicked() {
                    *evaluate_requested = true;
                }
            });
        });

        let document = self.host.document().clone();
        ui.horizontal(|ui| {
            egui::ScrollArea::horizontal()
                .id_salt("sheet-tabs")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (index, sheet) in document.sheets.iter().enumerate() {
                            if ui
                                .selectable_label(index == document.active_sheet, &sheet.name)
                                .clicked()
                            {
                                self.apply(|document| document.active_sheet = index);
                                self.selected_cell = 0;
                            }
                        }
                    });
                });
            if ui.small_button("+").clicked() {
                let requested = self.sheet_name_input.trim();
                let name = if requested.is_empty() {
                    next_sheet_name(&document)
                } else {
                    requested.to_owned()
                };
                if !sheet_name_available(&document, &name, None) {
                    self.shell
                        .set_status(format!("Sheet name '{name}' is empty or already in use"));
                } else {
                    self.apply(|document| {
                        document.sheets.push(Sheet::new(name));
                        document.active_sheet = document.sheets.len() - 1;
                    });
                    self.sheet_name_input.clear();
                    self.selected_cell = 0;
                }
            }
            if document.sheets.len() > 1 && ui.small_button("-").clicked() {
                self.apply(|document| {
                    let index = document.active_sheet.min(document.sheets.len() - 1);
                    document.sheets.remove(index);
                    document.active_sheet = index.min(document.sheets.len() - 1);
                });
                self.selected_cell = 0;
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.sheet_name_input)
                    .hint_text("new sheet name")
                    .desired_width(150.0),
            );
            if ui.button("Rename").clicked() {
                let name = self.sheet_name_input.trim().to_owned();
                if !sheet_name_available(&document, &name, Some(document.active_sheet)) {
                    self.shell
                        .set_status(format!("Sheet name '{name}' is empty or already in use"));
                } else {
                    self.apply(|document| {
                        if let Some(sheet) = document.active_sheet_mut() {
                            sheet.name = name;
                        }
                    });
                    self.sheet_name_input.clear();
                }
            }
        });

        let Some(sheet) = document.active_sheet() else {
            return;
        };
        let index = self.selected_cell;
        for (cell_index, cell) in sheet.cells.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        cell_index == index,
                        mathcraft::document::Cell::name(cell_index),
                    )
                    .clicked()
                {
                    self.selected_cell = cell_index;
                }
                let gained_focus;
                let lost_focus;
                {
                    // Capture the pre-edit snapshot before egui processes the
                    // first keystroke, not one frame after focus was gained.
                    self.host.history_mut().begin_gesture();
                    let target = self
                        .host
                        .history_mut()
                        .current_mut()
                        .sheets
                        .get_mut(document.active_sheet)
                        .and_then(|sheet| sheet.cells.get_mut(cell_index));
                    if let Some(target) = target {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut target.expression)
                                .desired_width(f32::INFINITY),
                        );
                        gained_focus = response.gained_focus();
                        lost_focus = response.lost_focus();
                    } else {
                        gained_focus = false;
                        lost_focus = false;
                    }
                }
                if gained_focus {
                    self.gesture_active = true;
                }
                if lost_focus && !gained_focus {
                    self.finish_gesture();
                }
                match (&cell.result, &cell.error) {
                    (Some(value), _) => {
                        ui.label(
                            RichText::new(format!("= {}", expr::format_value(*value)))
                                .color(theme::SUCCESS),
                        );
                    }
                    (None, Some(error)) => {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("Error: {error}")).color(theme::ERROR),
                            )
                            .wrap(),
                        );
                    }
                    _ => {}
                }
                if sheet.cells.len() > 1 && ui.small_button("x").clicked() {
                    self.apply(|document| {
                        if let Some(sheet) = document.sheets.get_mut(document.active_sheet) {
                            sheet.cells.remove(cell_index);
                        }
                    });
                    self.clamp_selected_cell();
                }
            });
        }
        if ui.button("Add cell").clicked() {
            self.apply(|document| {
                if let Some(sheet) = document.active_sheet_mut() {
                    sheet.cells.push(mathcraft::document::Cell::new(""));
                }
            });
            self.selected_cell = self
                .host
                .document()
                .active_sheet()
                .map_or(0, |sheet| sheet.cells.len() - 1);
        }

        ui.collapsing("Syntax help", |ui| {
            ui.label(RichText::new("Operators: + - * / ^ · implicit products: 2x, 2(x+1) · constants: pi, e").color(theme::MUTED));
            ui.label(RichText::new(format!("Functions: {}", FUNCTION_NAMES.join(", "))).color(theme::MUTED));
            ui.label(RichText::new("Cell references: A1, A2 (case-insensitive) · cross-sheet: Plot.A1 or 'My Sheet'.A1 · scientific notation: 1.5e-3.").color(theme::MUTED));
        });
    }

    fn plot_curves(&self) -> Vec<(String, Vec<Sample>)> {
        let document = self.host.document();
        let variables = self.evaluation_variables();
        let options = PlotOptions {
            x_min: self.plot_x_min,
            x_max: self.plot_x_max,
            samples: PLOT_SAMPLES,
        };
        let mut curves = Vec::new();
        if let Some(sheet) = document.active_sheet() {
            if self.show_all_curves {
                for (index, cell) in sheet.cells.iter().enumerate() {
                    if cell.expression.trim().is_empty() {
                        continue;
                    }
                    if let Ok(points) = plot::sample(&cell.expression, &variables, options) {
                        curves.push((mathcraft::document::Cell::name(index), points));
                    }
                }
            } else if let Some(cell) = sheet.cells.get(self.selected_cell)
                && !cell.expression.trim().is_empty()
                && let Ok(points) = plot::sample(&cell.expression, &variables, options)
            {
                curves.push((mathcraft::document::Cell::name(self.selected_cell), points));
            }
        }
        for series in &document.series {
            let points = series
                .points
                .iter()
                .map(|(x, y)| Sample { x: *x, y: *y })
                .collect();
            curves.push((series.name.clone(), points));
        }
        curves
    }

    fn plot_panel(&mut self, ui: &mut egui::Ui) {
        let previous_x = (self.plot_x_min, self.plot_x_max);
        let previous_y = (self.plot_y_min, self.plot_y_max);
        ui.horizontal(|ui| {
            ui.label("Plot y = f(x)");
            ui.checkbox(&mut self.show_all_curves, "All cells");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Reset view").clicked() {
                    self.plot_x_min = -10.0;
                    self.plot_x_max = 10.0;
                    self.plot_y_min = None;
                    self.plot_y_max = None;
                }
                ui.add(
                    egui::DragValue::new(&mut self.plot_x_max)
                        .speed(0.1)
                        .prefix("x max "),
                );
                ui.add(
                    egui::DragValue::new(&mut self.plot_x_min)
                        .speed(0.1)
                        .prefix("x min "),
                );
                let mut auto = self.plot_y_min.is_none() && self.plot_y_max.is_none();
                if ui.checkbox(&mut auto, "Auto Y").clicked() {
                    if auto {
                        self.plot_y_min = None;
                        self.plot_y_max = None;
                    } else {
                        let curves = self.plot_curves();
                        let points = curves
                            .iter()
                            .flat_map(|(_, points)| points.iter().copied())
                            .filter(|point| {
                                point.x >= self.plot_x_min && point.x <= self.plot_x_max
                            })
                            .collect::<Vec<_>>();
                        let (low, high) = plot::y_bounds(&points);
                        self.plot_y_min = Some(low);
                        self.plot_y_max = Some(high);
                    }
                }
                if !auto {
                    let mut low = self.plot_y_min.unwrap_or(-1.0);
                    let mut high = self.plot_y_max.unwrap_or(1.0);
                    ui.add(egui::DragValue::new(&mut low).speed(0.1).prefix("y min "));
                    ui.add(egui::DragValue::new(&mut high).speed(0.1).prefix("y max "));
                    self.plot_y_min = Some(low);
                    self.plot_y_max = Some(high);
                }
            });
        });

        if !valid_range(self.plot_x_min, self.plot_x_max) {
            (self.plot_x_min, self.plot_x_max) = previous_x;
            self.shell
                .set_status("X range must be finite and increasing");
        }
        let y_range = match (self.plot_y_min, self.plot_y_max) {
            (None, None) => None,
            (Some(low), Some(high)) if valid_range(low, high) => Some((low, high)),
            _ => {
                self.shell
                    .set_status("Y range must be finite and increasing");
                match previous_y {
                    (Some(low), Some(high)) if valid_range(low, high) => {
                        (self.plot_y_min, self.plot_y_max) = previous_y;
                        Some((low, high))
                    }
                    _ => {
                        self.plot_y_min = None;
                        self.plot_y_max = None;
                        None
                    }
                }
            }
        };

        let curves = self.plot_curves();
        let visible_points = curves
            .iter()
            .flat_map(|(_, points)| points.iter().copied())
            .filter(|point| {
                point.x.is_finite()
                    && point.y.is_finite()
                    && point.x >= self.plot_x_min
                    && point.x <= self.plot_x_max
            })
            .collect::<Vec<_>>();
        let (auto_low, auto_high) = plot::y_bounds(&visible_points);
        let (y_min, y_max) = y_range.unwrap_or((auto_low, auto_high));

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(1.0), PLOT_HEIGHT),
            egui::Sense::click_and_drag(),
        );

        if response.dragged() && rect.width() > 0.0 && rect.height() > 0.0 {
            let delta = response.drag_delta();
            if let Some((low, high)) = pan_range(
                self.plot_x_min,
                self.plot_x_max,
                -delta.x as f64 / rect.width() as f64,
            ) {
                self.plot_x_min = low;
                self.plot_x_max = high;
            }
            if let Some((low, high)) =
                pan_range(y_min, y_max, delta.y as f64 / rect.height() as f64)
            {
                self.plot_y_min = Some(low);
                self.plot_y_max = Some(high);
            }
        }
        if response.hovered() && rect.width() > 0.0 && rect.height() > 0.0 {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let factor = if scroll > 0.0 { 0.85_f64 } else { 1.0 / 0.85 };
                let pointer = response.hover_pos().unwrap_or(rect.center());
                let x_fraction =
                    ((pointer.x - rect.left()) as f64 / rect.width() as f64).clamp(0.0, 1.0);
                let y_fraction =
                    ((pointer.y - rect.top()) as f64 / rect.height() as f64).clamp(0.0, 1.0);
                let anchor_x = interpolate(self.plot_x_min, self.plot_x_max, x_fraction);
                if let Some((low, high)) =
                    zoom_range(self.plot_x_min, self.plot_x_max, anchor_x, factor)
                {
                    self.plot_x_min = low;
                    self.plot_x_max = high;
                }
                let anchor_y = interpolate(y_max, y_min, y_fraction);
                if let Some((low, high)) = zoom_range(y_min, y_max, anchor_y, factor) {
                    self.plot_y_min = Some(low);
                    self.plot_y_max = Some(high);
                }
            }
        }

        paint_plot(
            ui.painter(),
            rect,
            &curves,
            self.plot_x_min,
            self.plot_x_max,
            y_min,
            y_max,
        );
        ui.label(
            RichText::new("Drag to pan · scroll to zoom · zoom is anchored at the pointer")
                .color(theme::FAINT),
        );

        ui.horizontal(|ui| {
            if ui.button("Export plot CSV").clicked() {
                let columns = curves
                    .iter()
                    .map(|(name, points)| (name.clone(), points.as_slice()))
                    .collect::<Vec<_>>();
                let text = csv::export_curves(&columns);
                self.write_export("Export plot CSV", "plot.csv", "csv", &text, "plot data");
            }
            if ui.button("Export plot PNG").clicked() {
                self.export_plot_png(&curves, y_min, y_max);
            }
        });
    }

    fn export_plot_png(&mut self, curves: &[(String, Vec<Sample>)], y_min: f64, y_max: f64) {
        let Some(path) = dialogs::save_project("Export plot PNG", "plot.png", "png") else {
            return;
        };
        let named = curves
            .iter()
            .map(|(name, points)| (name.clone(), points.as_slice()))
            .collect::<Vec<_>>();
        let raster = raster::render_plot_named(
            &named,
            self.plot_x_min,
            self.plot_x_max,
            y_min,
            y_max,
            960,
            540,
        );
        match raster.save(&path) {
            Ok(()) => self
                .shell
                .set_status(format!("Exported PNG to {}", path.display())),
            Err(error) => self
                .shell
                .notify(Tone::Error, "Could not export PNG", error.to_string()),
        }
    }

    fn matrix_panel(&mut self, ui: &mut egui::Ui) {
        widgets::section_label(ui, "Matrices");
        ui.label(
            RichText::new("Rows separated by ';', coefficients by spaces or commas")
                .size(11.0)
                .color(theme::MUTED),
        );
        ui.horizontal(|ui| {
            ui.label("A");
            ui.add(
                egui::TextEdit::multiline(&mut self.matrix_a)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.horizontal(|ui| {
            ui.label("B");
            ui.add(
                egui::TextEdit::multiline(&mut self.matrix_b)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.horizontal(|ui| {
            if ui.button("det(A)").clicked() {
                self.run_matrix_action(MatrixAction::Determinant);
            }
            if ui.button("transpose(A)").clicked() {
                self.run_matrix_action(MatrixAction::Transpose);
            }
            if ui.button("A x B").clicked() {
                self.run_matrix_action(MatrixAction::Product);
            }
            if ui.button("A + B").clicked() {
                self.run_matrix_action(MatrixAction::Add);
            }
            if ui.button("A - B").clicked() {
                self.run_matrix_action(MatrixAction::Subtract);
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.matrix_name).desired_width(100.0));
            if ui.button("Save A").clicked() {
                self.save_current_matrix();
            }
        });
        if let Some(result) = &self.matrix_result {
            let (text, color) = match result {
                Ok(value) => (value.clone(), theme::SUCCESS),
                Err(message) => (format!("Error: {message}"), theme::ERROR),
            };
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .font(egui::FontId::monospace(13.0))
                        .color(color),
                )
                .wrap(),
            );
        }
    }

    fn csv_panel(&mut self, ui: &mut egui::Ui) {
        widgets::section_label(ui, "Import CSV points");
        ui.label(
            RichText::new("CSV columns x,y,z,... with an optional header; one series per Y column")
                .size(11.0)
                .color(theme::MUTED),
        );
        ui.add(
            egui::TextEdit::multiline(&mut self.csv_text)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.series_name).desired_width(100.0));
            if ui.button("Import").clicked() {
                self.import_csv();
            }
        });
        if let Some(status) = &self.import_status {
            let (text, color) = match status {
                Ok(message) => (message.clone(), theme::SUCCESS),
                Err(message) => (format!("Error: {message}"), theme::ERROR),
            };
            ui.label(RichText::new(text).color(color));
        }
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        let mut evaluate_requested = false;
        egui::CentralPanel::default().show(ui, |ui| {
            self.sheet_panel(ui, &mut evaluate_requested);
            ui.separator();
            self.plot_panel(ui);
            ui.separator();
            self.matrix_panel(ui);
        });
        if evaluate_requested {
            self.evaluate_sheet();
        }
    }

    fn unsaved_window(&mut self, ctx: &egui::Context) {
        if self.pending.is_none() {
            return;
        }
        let mut open = true;
        let mut choice = None;
        egui::Window::new("Unsaved changes")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.label("The current project has unsaved changes.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save and continue").clicked() {
                        choice = Some(PendingChoice::Save);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(PendingChoice::Discard);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(PendingChoice::Cancel);
                    }
                });
            });
        if let Some(choice) = choice {
            self.resolve_pending(choice);
        } else if !open {
            self.pending = None;
        }
    }
}

fn refresh_document(document: &mut Document) {
    let outcomes = sheet::evaluate_workbook(&document.sheets, &document.variables);
    for (sheet, outcomes) in document.sheets.iter_mut().zip(outcomes) {
        sheet::apply_outcomes(sheet, &outcomes);
    }
    document.sync_legacy_expression();
}

fn record_history(document: &mut Document, expression: String) {
    if expression.trim().is_empty() || document.history.last() == Some(&expression) {
        return;
    }
    document.history.push(expression);
    if document.history.len() > HISTORY_LIMIT {
        document.history.remove(0);
    }
}

fn sheet_name_available(document: &Document, name: &str, except: Option<usize>) -> bool {
    !name.trim().is_empty()
        && !document.sheets.iter().enumerate().any(|(index, sheet)| {
            Some(index) != except && sheet.name.eq_ignore_ascii_case(name.trim())
        })
}

fn next_sheet_name(document: &Document) -> String {
    for number in 1.. {
        let candidate = format!("Sheet {number}");
        if sheet_name_available(document, &candidate, None) {
            return candidate;
        }
    }
    String::new()
}

fn valid_range(minimum: f64, maximum: f64) -> bool {
    minimum.is_finite() && maximum.is_finite() && minimum < maximum
}

fn interpolate(minimum: f64, maximum: f64, fraction: f64) -> f64 {
    if !valid_range(minimum, maximum) || !fraction.is_finite() {
        return minimum;
    }
    let span = maximum - minimum;
    if span.is_finite() {
        minimum + span * fraction
    } else {
        minimum * (1.0 - fraction) + maximum * fraction
    }
}

fn pan_range(minimum: f64, maximum: f64, fraction: f64) -> Option<(f64, f64)> {
    if !valid_range(minimum, maximum) || !fraction.is_finite() {
        return None;
    }
    let span = maximum - minimum;
    if !span.is_finite() {
        return None;
    }
    let shift = span * fraction;
    let low = minimum - shift;
    let high = maximum - shift;
    valid_range(low, high).then_some((low, high))
}

fn zoom_range(minimum: f64, maximum: f64, anchor: f64, factor: f64) -> Option<(f64, f64)> {
    if !valid_range(minimum, maximum) || !anchor.is_finite() || !factor.is_finite() || factor <= 0.0
    {
        return None;
    }
    let low = anchor + (minimum - anchor) * factor;
    let high = anchor + (maximum - anchor) * factor;
    valid_range(low, high).then_some((low, high))
}

#[allow(clippy::too_many_arguments)]
fn paint_plot(
    painter: &egui::Painter,
    rect: egui::Rect,
    curves: &[(String, Vec<Sample>)],
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let to_screen = |x: f64, y: f64| {
        egui::pos2(
            rect.left() + plot::normalized(x, x_min, x_max) as f32 * rect.width(),
            rect.bottom() - plot::normalized(y, y_min, y_max) as f32 * rect.height(),
        )
    };

    painter.rect_filled(rect, 4.0, theme::SUNKEN);
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    let grid = egui::Stroke::new(1.0, theme::BORDER);
    for x in plot::axis_ticks(x_min, x_max, 9) {
        let top = to_screen(x, y_max);
        painter.line_segment(
            [
                egui::pos2(top.x, rect.top()),
                egui::pos2(top.x, rect.bottom()),
            ],
            grid,
        );
        painter.text(
            egui::pos2(top.x, rect.bottom() - 3.0),
            egui::Align2::CENTER_BOTTOM,
            expr::format_value(x),
            egui::FontId::proportional(10.0),
            theme::FAINT,
        );
    }
    for y in plot::axis_ticks(y_min, y_max, 5) {
        let left = to_screen(x_min, y);
        painter.line_segment(
            [
                egui::pos2(rect.left(), left.y),
                egui::pos2(rect.right(), left.y),
            ],
            grid,
        );
        painter.text(
            egui::pos2(rect.left() + 4.0, left.y),
            egui::Align2::LEFT_CENTER,
            expr::format_value(y),
            egui::FontId::proportional(10.0),
            theme::FAINT,
        );
    }
    if (x_min..=x_max).contains(&0.0) {
        let x = to_screen(0.0, y_min).x;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.2, theme::BORDER_STRONG),
        );
    }
    if (y_min..=y_max).contains(&0.0) {
        let y = to_screen(x_min, 0.0).y;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.2, theme::BORDER_STRONG),
        );
    }
    let legend = [
        theme::INFO,
        theme::SUCCESS,
        theme::WARNING,
        theme::ERROR,
        theme::ACCENT_LIGHT,
        theme::MUTED,
    ];
    for (index, (name, points)) in curves.iter().enumerate() {
        let color = legend[index % legend.len()];
        let curve = egui::Stroke::new(2.0, color);
        for pair in points.windows(2) {
            if [pair[0].x, pair[0].y, pair[1].x, pair[1].y]
                .iter()
                .all(|value| value.is_finite())
            {
                let start = to_screen(pair[0].x, pair[0].y);
                let end = to_screen(pair[1].x, pair[1].y);
                if [start.x, start.y, end.x, end.y]
                    .iter()
                    .all(|value| value.is_finite())
                {
                    painter.line_segment([start, end], curve);
                }
            }
        }
        let column = index % 6;
        let row = index / 6;
        painter.text(
            egui::pos2(
                rect.left() + 8.0 + column as f32 * 74.0,
                rect.top() + 10.0 + row as f32 * 14.0,
            ),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(10.0),
            color,
        );
    }
}

impl eframe::App for MathApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.gesture_active {
            self.host.history_mut().begin_gesture();
        }
        self.handle_shortcuts(ui);
        self.top_bar(ui);
        self.status_bar(ui);
        self.left_panel(ui);
        self.central_panel(ui);
        self.unsaved_window(ui.ctx());
        self.shell.show_windows(ui.ctx());
        self.shell
            .update_title(ui.ctx(), &self.host.display_name(), self.host.is_dirty());
    }
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    for character in title.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "untitled".to_owned()
    } else {
        slug.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft::engine::matrix::Matrix;

    #[test]
    fn history_is_bounded() {
        let mut document = Document::default();
        for index in 0..(HISTORY_LIMIT + 5) {
            record_history(&mut document, format!("expr {index}"));
        }
        assert_eq!(document.history.len(), HISTORY_LIMIT);
        assert_eq!(document.history[0], "expr 5");
    }

    #[test]
    fn matrix_display_can_be_reloaded_from_text() {
        let matrix = Matrix::from_2x2(1.0, 2.0, 3.0, 4.0);
        assert_eq!(matrix::parse_matrix(&matrix.to_string()).unwrap(), matrix);
    }

    #[test]
    fn png_export_raster_has_expected_size() {
        let points = vec![Sample { x: 0.0, y: 0.0 }, Sample { x: 1.0, y: 1.0 }];
        let named = [("A1".to_owned(), points.as_slice())];
        let raster = raster::render_plot_named(&named, 0.0, 1.0, 0.0, 1.0, 960, 540);
        let bytes = raster.to_png().unwrap();
        assert_eq!(&bytes[16..24], &[0, 0, 3, 192, 0, 0, 2, 28]);
    }

    #[test]
    fn history_deduplicates_consecutive_evaluations() {
        let mut document = Document::default();
        record_history(&mut document, "1+1".to_owned());
        record_history(&mut document, "1+1".to_owned());
        record_history(&mut document, "2+2".to_owned());
        assert_eq!(document.history, vec!["1+1", "2+2"]);
    }

    #[test]
    fn sheet_names_are_unique_and_auto_names_reuse_gaps() {
        let mut document = Document::default();
        document.sheets.push(Sheet::new("Data"));
        assert!(sheet_name_available(&document, "Notes", None));
        assert!(!sheet_name_available(&document, "data", None));
        assert!(sheet_name_available(&document, "DATA", Some(1)));
        document.sheets.remove(0);
        assert_eq!(next_sheet_name(&document), "Sheet 1");
    }

    #[test]
    fn plot_pan_and_zoom_preserve_valid_ranges_and_the_anchor() {
        assert_eq!(pan_range(0.0, 10.0, 0.1), Some((-1.0, 9.0)));
        assert_eq!(zoom_range(0.0, 10.0, 4.0, 0.5), Some((2.0, 7.0)));
        assert!(pan_range(0.0, 10.0, f64::INFINITY).is_none());
        assert!(zoom_range(0.0, 10.0, 4.0, 0.0).is_none());
        assert_eq!(interpolate(-1e308, 1e308, 0.5), 0.0);
    }

    #[test]
    fn refresh_document_updates_cross_sheet_results_and_legacy_expression() {
        let mut first = Sheet::new("First");
        first.cells = vec![mathcraft::document::Cell::new("2")];
        let mut second = Sheet::new("Second");
        second.cells = vec![mathcraft::document::Cell::new("First.A1 * 3")];
        let mut document = Document {
            sheets: vec![first, second],
            active_sheet: 1,
            ..Document::default()
        };
        refresh_document(&mut document);
        assert_eq!(document.sheets[0].cells[0].result, Some(2.0));
        assert_eq!(document.sheets[1].cells[0].result, Some(6.0));
        assert_eq!(document.expression, "First.A1 * 3");
    }
}
