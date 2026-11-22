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
use mathcraft::engine::raster::{Color, Raster};
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
                host.reset(document);
                self.host = host;
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

    fn save(&mut self) -> bool {
        self.finish_gesture();
        self.host.apply(Document::sync_legacy_expression);
        if self.host.path().is_none() {
            return self.save_as();
        }
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
        self.host.apply(Document::sync_legacy_expression);
        let default_name = format!("{}.json", slugify(&self.host.document().title));
        let Some(path) = dialogs::save_project("Save project", &default_name, "json") else {
            return false;
        };
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

    /// Variables visible to the active sheet, including other sheets' results.
    fn evaluation_variables(&self) -> Vec<(String, f64)> {
        let document = self.host.document();
        let mut variables = document.variables.clone();
        for sheet in &document.sheets {
            if let Some(result) = sheet.cells.iter().find_map(|cell| cell.result) {
                variables.push((sheet.name.clone(), result));
            }
            for (index, cell) in sheet.cells.iter().enumerate() {
                if let Some(result) = cell.result {
                    variables.push((
                        format!("{}.{}", sheet.name, mathcraft::document::Cell::name(index)),
                        result,
                    ));
                }
            }
        }
        variables
    }

    fn evaluate_sheet(&mut self) {
        let document = self.host.document().clone();
        let Some(sheet) = document.active_sheet() else {
            return;
        };
        let variables = self.evaluation_variables();
        let outcomes = sheet::evaluate_sheet(sheet, &variables);
        let index = document.active_sheet;
        let expressions = sheet
            .cells
            .iter()
            .zip(&outcomes)
            .filter(|(_, outcome)| outcome.value.is_some() || outcome.error.is_some())
            .map(|(cell, _)| cell.expression.clone())
            .collect::<Vec<_>>();
        self.host.apply(|document| {
            if let Some(sheet) = document.sheets.get_mut(index) {
                sheet::apply_outcomes(sheet, &outcomes);
            }
            for expression in expressions {
                record_history(document, expression);
            }
            document.sync_legacy_expression();
        });
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
        self.host.apply(|document| {
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
        let points = match csv::parse_points(&self.csv_text) {
            Ok(points) => points,
            Err(error) => {
                self.import_status = Some(Err(error.to_string()));
                return;
            }
        };
        let name = self.series_name.trim();
        let name = if name.is_empty() { "series" } else { name }.to_owned();
        self.host.apply(|document| {
            if let Some(series) = document
                .series
                .iter_mut()
                .find(|series| series.name == name)
            {
                series.points = points.clone();
            } else {
                document.series.push(NamedSeries {
                    name: name.clone(),
                    points: points.clone(),
                });
            }
        });
        self.import_status = Some(Ok(format!(
            "Imported {} points into '{name}'",
            points.len()
        )));
    }

    fn export_sheet_csv(&mut self) {
        self.finish_gesture();
        let document = self.host.document();
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
        self.host.apply(|document| {
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
        let mut load_matrix = None;
        let mut remove_series = None;
        let mut picked_history = None;
        let mut variable_focus_gained = false;
        let mut variable_focus_lost = false;

        egui::Panel::left("notebook-panel")
            .resizable(true)
            .default_size(260.0)
            .min_size(210.0)
            .show(ui, |ui| {
                widgets::section_label(ui, "Variables");
                for (index, (name, value)) in self
                    .host
                    .history_mut()
                    .current_mut()
                    .variables
                    .iter_mut()
                    .enumerate()
                {
                    ui.horizontal(|ui| {
                        let name_response =
                            ui.add(egui::TextEdit::singleline(name).desired_width(78.0));
                        let value_response = ui.add(egui::DragValue::new(value).speed(0.1));
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
                    self.host.apply(|document| {
                        document.variables.remove(index);
                    });
                }
                if add_variable {
                    let name = self.new_variable_name.trim().to_owned();
                    if !name.is_empty() {
                        let value = self.new_variable_value;
                        self.host.apply(|document| {
                            if let Some(entry) = document
                                .variables
                                .iter_mut()
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
                        if ui.small_button("load").clicked() {
                            load_matrix = Some(entry.matrix.to_string());
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
                    ui.label(RichText::new("Import CSV to add a curve").color(theme::FAINT));
                }
                for (index, entry) in series.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} ({} pts)", entry.name, entry.points.len()))
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
                            ui.label(RichText::new("Nothing evaluated yet").color(theme::FAINT));
                        }
                        for expression in history.iter().rev() {
                            if ui.selectable_label(false, expression).clicked() {
                                picked_history = Some(expression.clone());
                            }
                        }
                    });
            });

        if let Some(text) = load_matrix {
            self.matrix_a = matrix_text_from_rows(&text);
            self.matrix_result = Some(Ok("Matrix loaded into A".to_owned()));
        }
        if let Some(index) = remove_matrix {
            self.host.apply(|document| {
                document.matrices.remove(index);
            });
        }
        if let Some(index) = remove_series {
            self.host.apply(|document| {
                document.series.remove(index);
            });
        }
        if let Some(expression) = picked_history {
            let index = self.selected_cell;
            self.host.apply(|document| {
                if let Some(cell) = document
                    .active_sheet_mut()
                    .and_then(|sheet| sheet.cells.get_mut(index))
                {
                    cell.expression = expression;
                }
            });
        }
        if variable_focus_gained {
            self.host.history_mut().begin_gesture();
        }
        if variable_focus_lost {
            self.host.history_mut().end_gesture();
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
            for (index, sheet) in document.sheets.iter().enumerate() {
                if ui
                    .selectable_label(index == document.active_sheet, &sheet.name)
                    .clicked()
                {
                    self.host.apply(|document| document.active_sheet = index);
                    self.selected_cell = 0;
                }
            }
            if ui.small_button("+").clicked() {
                let name = if self.sheet_name_input.trim().is_empty() {
                    format!("Sheet {}", document.sheets.len() + 1)
                } else {
                    self.sheet_name_input.trim().to_owned()
                };
                self.host.apply(|document| {
                    document.sheets.push(Sheet::new(name));
                    document.active_sheet = document.sheets.len() - 1;
                });
                self.sheet_name_input.clear();
                self.selected_cell = 0;
            }
            if document.sheets.len() > 1 && ui.small_button("-").clicked() {
                let index = document.active_sheet;
                self.host.apply(|document| {
                    document.sheets.remove(index);
                    document.active_sheet = document.active_sheet.saturating_sub(1);
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
                if lost_focus {
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
                        ui.label(RichText::new(format!("Error: {error}")).color(theme::ERROR));
                    }
                    _ => {}
                }
                if sheet.cells.len() > 1 && ui.small_button("x").clicked() {
                    self.host.apply(|document| {
                        if let Some(sheet) = document.sheets.get_mut(document.active_sheet) {
                            sheet.cells.remove(cell_index);
                        }
                    });
                    self.clamp_selected_cell();
                }
            });
        }
        if ui.button("Add cell").clicked() {
            self.host.apply(|document| {
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
            ui.label(RichText::new("Cell references: A1, A2 (case-insensitive) · sheet names and variables are also in scope.").color(theme::MUTED));
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
                let mut auto = self.plot_y_min.is_none() || self.plot_y_max.is_none();
                if ui.checkbox(&mut auto, "Auto Y").clicked() {
                    if auto {
                        self.plot_y_min = None;
                        self.plot_y_max = None;
                    } else {
                        let bounds = plot::y_bounds(
                            &self
                                .plot_curves()
                                .into_iter()
                                .flat_map(|(_, points)| points)
                                .collect::<Vec<_>>(),
                        );
                        self.plot_y_min = Some(bounds.0);
                        self.plot_y_max = Some(bounds.1);
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

        let curves = self.plot_curves();
        let all_points = curves
            .iter()
            .flat_map(|(_, points)| points.iter().copied())
            .collect::<Vec<_>>();
        let (auto_low, auto_high) = plot::y_bounds(&all_points);
        let y_min = self.plot_y_min.unwrap_or(auto_low);
        let y_max = self.plot_y_max.unwrap_or(auto_high);

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), PLOT_HEIGHT),
            egui::Sense::click_and_drag(),
        );
        let span_x = (self.plot_x_max - self.plot_x_min).max(1e-9);
        let span_y = (y_max - y_min).max(1e-9);

        if response.dragged() {
            let delta = response.drag_delta();
            self.plot_x_min -= delta.x as f64 / rect.width() as f64 * span_x;
            self.plot_x_max -= delta.x as f64 / rect.width() as f64 * span_x;
            let shift = delta.y as f64 / rect.height() as f64 * span_y;
            if let (Some(low), Some(high)) = (self.plot_y_min, self.plot_y_max) {
                self.plot_y_min = Some(low + shift);
                self.plot_y_max = Some(high + shift);
            } else {
                self.plot_y_min = Some(auto_low + shift);
                self.plot_y_max = Some(auto_high + shift);
            }
        }
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let factor = if scroll > 0.0 {
                    0.85_f64
                } else {
                    1.0 / 0.85_f64
                };
                let anchor_x = response
                    .hover_pos()
                    .map_or((self.plot_x_min + self.plot_x_max) / 2.0, |pos| {
                        self.plot_x_min + ((pos.x - rect.left()) / rect.width()) as f64 * span_x
                    });
                self.plot_x_min = anchor_x + (self.plot_x_min - anchor_x) * factor;
                self.plot_x_max = anchor_x + (self.plot_x_max - anchor_x) * factor;
                if let (Some(low), Some(high)) = (self.plot_y_min, self.plot_y_max) {
                    let mid = (low + high) / 2.0;
                    self.plot_y_min = Some(mid + (low - mid) * factor);
                    self.plot_y_max = Some(mid + (high - mid) * factor);
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
        ui.label(RichText::new("Drag to pan · scroll to zoom").color(theme::FAINT));

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
        let slices = curves
            .iter()
            .map(|(_, points)| points.as_slice())
            .collect::<Vec<_>>();
        let raster = raster_plot(&slices, self.plot_x_min, self.plot_x_max, y_min, y_max);
        let Some(path) = dialogs::save_project("Export plot PNG", "plot.png", "png") else {
            return;
        };
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
            ui.add(egui::TextEdit::singleline(&mut self.matrix_a).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.label("B");
            ui.add(egui::TextEdit::singleline(&mut self.matrix_b).desired_width(f32::INFINITY));
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
            ui.label(
                RichText::new(text)
                    .font(egui::FontId::monospace(13.0))
                    .color(color),
            );
        }
    }

    fn csv_panel(&mut self, ui: &mut egui::Ui) {
        widgets::section_label(ui, "Import CSV points");
        ui.label(
            RichText::new("Two columns x,y with an optional header")
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
            ui.separator();
            self.csv_panel(ui);
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

fn record_history(document: &mut Document, expression: String) {
    if expression.trim().is_empty() {
        return;
    }
    document.history.push(expression);
    if document.history.len() > HISTORY_LIMIT {
        document.history.remove(0);
    }
}

fn matrix_text_from_rows(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_matches(['[', ']']).to_owned())
        .collect::<Vec<_>>()
        .join("; ")
}

const PLOT_COLORS: [Color; 6] = [
    [106, 168, 245, 255],
    [84, 209, 132, 255],
    [238, 180, 86, 255],
    [232, 87, 63, 255],
    [178, 132, 245, 255],
    [96, 214, 200, 255],
];

fn raster_plot(curves: &[&[Sample]], x_min: f64, x_max: f64, y_min: f64, y_max: f64) -> Raster {
    const WIDTH: usize = 960;
    const HEIGHT: usize = 540;
    let mut raster = Raster::new(WIDTH, HEIGHT, [16, 18, 24, 255]);
    let span_x = (x_max - x_min).max(1e-9);
    let span_y = (y_max - y_min).max(1e-9);
    let to_pixel = |x: f64, y: f64| -> (i32, i32) {
        (
            ((x - x_min) / span_x * (WIDTH - 1) as f64).round() as i32,
            ((y_max - y) / span_y * (HEIGHT - 1) as f64).round() as i32,
        )
    };
    for tick in 0..=8 {
        let (px, _) = to_pixel(x_min + span_x * tick as f64 / 8.0, y_min);
        raster.draw_line(px, 0, px, HEIGHT as i32 - 1, [40, 44, 54, 255]);
    }
    for tick in 0..=4 {
        let (_, py) = to_pixel(x_min, y_min + span_y * tick as f64 / 4.0);
        raster.draw_line(0, py, WIDTH as i32 - 1, py, [40, 44, 54, 255]);
    }
    if (x_min..=x_max).contains(&0.0) {
        let (px, _) = to_pixel(0.0, y_min);
        raster.draw_line(px, 0, px, HEIGHT as i32 - 1, [72, 80, 96, 255]);
    }
    if (y_min..=y_max).contains(&0.0) {
        let (_, py) = to_pixel(x_min, 0.0);
        raster.draw_line(0, py, WIDTH as i32 - 1, py, [72, 80, 96, 255]);
    }
    for (index, points) in curves.iter().enumerate() {
        let color = PLOT_COLORS[index % PLOT_COLORS.len()];
        for pair in points.windows(2) {
            if pair[0].y.is_finite() && pair[1].y.is_finite() {
                let (x0, y0) = to_pixel(pair[0].x, pair[0].y);
                let (x1, y1) = to_pixel(pair[1].x, pair[1].y);
                raster.draw_line(x0, y0, x1, y1, color);
            }
        }
    }
    raster
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
    let span_x = (x_max - x_min).max(1e-9);
    let span_y = (y_max - y_min).max(1e-9);
    let to_screen = |x: f64, y: f64| {
        egui::pos2(
            rect.left() + ((x - x_min) / span_x) as f32 * rect.width(),
            rect.bottom() - ((y - y_min) / span_y) as f32 * rect.height(),
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
    for tick in 0..=8 {
        let x = x_min + span_x * tick as f64 / 8.0;
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
            format!("{x:.1}"),
            egui::FontId::proportional(10.0),
            theme::FAINT,
        );
    }
    for tick in 0..=4 {
        let y = y_min + span_y * tick as f64 / 4.0;
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
            format!("{y:.1}"),
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
            if pair[0].y.is_finite() && pair[1].y.is_finite() {
                painter.line_segment(
                    [
                        to_screen(pair[0].x, pair[0].y),
                        to_screen(pair[1].x, pair[1].y),
                    ],
                    curve,
                );
            }
        }
        painter.text(
            egui::pos2(rect.right() - 6.0 - index as f32 * 74.0, rect.top() + 12.0),
            egui::Align2::RIGHT_CENTER,
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
        let text = matrix_text_from_rows(&matrix.to_string());
        assert_eq!(matrix::parse_matrix(&text).unwrap(), matrix);
    }

    #[test]
    fn png_export_raster_has_expected_size() {
        let points = vec![Sample { x: 0.0, y: 0.0 }, Sample { x: 1.0, y: 1.0 }];
        let raster = raster_plot(&[&points], 0.0, 1.0, 0.0, 1.0);
        let bytes = raster.to_png().unwrap();
        assert_eq!(&bytes[16..24], &[0, 0, 3, 192, 0, 0, 2, 28]);
    }
}
