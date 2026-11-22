//! MathCraft application UI and project lifecycle.

use std::path::PathBuf;

use craftkit_io::DocumentHost;
use craftkit_ui::cli::CliOptions;
use craftkit_ui::eframe::{self, egui};
use craftkit_ui::egui::{Align, Layout, RichText};
use craftkit_ui::{AppIdentity, AppShell, Tone, dialogs, theme, widgets};

use mathcraft::document::Document;
use mathcraft::engine::expr;
use mathcraft::engine::matrix::{Matrix, MatrixError};
use mathcraft::engine::plot::{self, PlotOptions, Sample};

const FORMAT: &str = "mathcraft";
const SCHEMA_VERSION: u32 = 1;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const HISTORY_LIMIT: usize = 100;
const PLOT_SAMPLES: usize = 401;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    New,
    Open,
    Save,
    SaveAs,
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

/// Result of the last expression evaluation.
#[derive(Debug, Clone)]
enum Evaluation {
    /// A numeric value ready to display.
    Value(String),
    /// A syntax or evaluation error ready to display.
    Error(String),
}

/// The application state.
pub struct MathApp {
    shell: AppShell,
    host: DocumentHost<Document>,
    expression_input: String,
    evaluation: Option<Evaluation>,
    new_variable_name: String,
    new_variable_value: f64,
    matrix_a: String,
    matrix_b: String,
    matrix_result: Option<Result<String, String>>,
    plot_x_min: f64,
    plot_x_max: f64,
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
            expression_input: String::new(),
            evaluation: None,
            new_variable_name: String::new(),
            new_variable_value: 0.0,
            matrix_a: "1 2; 3 4".to_owned(),
            matrix_b: "5 6; 7 8".to_owned(),
            matrix_result: None,
            plot_x_min: -10.0,
            plot_x_max: 10.0,
            pending: None,
            gesture_active: false,
        };
        app.shell.install_theme(&context.egui_ctx);
        app.expression_input = app.host.document().expression.clone();
        if let Some(path) = options.open {
            app.load_path(path);
        }
        app
    }

    fn load_path(&mut self, path: PathBuf) {
        match DocumentHost::from_file(path.clone(), FORMAT, SCHEMA_VERSION, APP_VERSION) {
            Ok(host) => {
                self.host = host;
                self.expression_input = self.host.document().expression.clone();
                self.evaluation = None;
                self.matrix_result = None;
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
        self.expression_input.clear();
        self.evaluation = None;
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
        self.sync_expression();
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
        self.sync_expression();
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

    fn sync_expression(&mut self) {
        let expression = self.expression_input.clone();
        self.host.apply(|document| document.expression = expression);
    }

    fn request_new(&mut self) {
        self.finish_gesture();
        self.sync_expression();
        if self.host.is_dirty() {
            self.pending = Some(PendingAction::New);
        } else {
            self.new_document();
        }
    }

    fn request_open(&mut self) {
        self.finish_gesture();
        self.sync_expression();
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
            Action::Undo => {
                self.finish_gesture();
                if self.host.undo() {
                    self.expression_input = self.host.document().expression.clone();
                    self.shell.set_status("Undo");
                }
            }
            Action::Redo => {
                self.finish_gesture();
                if self.host.redo() {
                    self.expression_input = self.host.document().expression.clone();
                    self.shell.set_status("Redo");
                }
            }
            Action::About => self.shell.open_about(),
            Action::Settings => self.shell.open_settings(),
        }
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

    fn evaluate(&mut self) {
        let expression = self.expression_input.trim().to_owned();
        if expression.is_empty() {
            self.evaluation = Some(Evaluation::Error("Enter an expression first".to_owned()));
            return;
        }
        let variables = self.host.document().variables.clone();
        self.evaluation = Some(match expr::evaluate(&expression, &variables) {
            Ok(value) => {
                self.host.apply(|document| {
                    document.expression = expression.clone();
                    record_history(document, expression.clone());
                });
                Evaluation::Value(expr::format_value(value))
            }
            Err(error) => Evaluation::Error(error.to_string()),
        });
    }

    fn run_matrix_action(&mut self, action: MatrixAction) {
        let left = match parse_matrix_input(&self.matrix_a) {
            Ok(matrix) => matrix,
            Err(error) => {
                self.matrix_result = Some(Err(error.to_string()));
                return;
            }
        };
        let outcome = match action {
            MatrixAction::Determinant => left.det().map(expr::format_value),
            MatrixAction::Transpose => Ok(left.transpose().to_string()),
            MatrixAction::Product => parse_matrix_input(&self.matrix_b)
                .and_then(|right| left.mul(&right).map(|product| product.to_string())),
        };
        self.matrix_result = Some(outcome.map_err(|error: MatrixError| error.to_string()));
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let title = self.host.display_name();
        let dirty = self.host.is_dirty();
        let can_undo = self.host.can_undo();
        let can_redo = self.host.can_redo();
        let mut action = None;
        self.shell.top_bar(ui, &title, dirty, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    action = Some(Action::New);
                    ui.close();
                }
                if ui.button("Open...").clicked() {
                    action = Some(Action::Open);
                    ui.close();
                }
                if ui.button("Save").clicked() {
                    action = Some(Action::Save);
                    ui.close();
                }
                if ui.button("Save As...").clicked() {
                    action = Some(Action::SaveAs);
                    ui.close();
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
            if ui.button("About").clicked() {
                action = Some(Action::About);
            }
            if ui.button("Settings").clicked() {
                action = Some(Action::Settings);
            }
        });
        if let Some(action) = action {
            self.handle_action(action);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        self.shell.status_bar(ui);
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        let MathApp {
            host,
            new_variable_name,
            new_variable_value,
            expression_input,
            ..
        } = self;
        let mut remove_index = None;
        let mut add_requested = false;
        let mut picked_expression = None;
        let mut variable_focus_gained = false;
        let mut variable_focus_lost = false;

        egui::Panel::left("variables-panel")
            .resizable(true)
            .default_size(240.0)
            .min_size(190.0)
            .show(ui, |ui| {
                widgets::section_label(ui, "Variables");
                {
                    let document = host.history_mut().current_mut();
                    for (index, (name, value)) in document.variables.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let name_response =
                                ui.add(egui::TextEdit::singleline(name).desired_width(70.0));
                            let value_response = ui.add(egui::DragValue::new(value).speed(0.1));
                            variable_focus_gained |=
                                name_response.gained_focus() || value_response.gained_focus();
                            variable_focus_lost |=
                                name_response.lost_focus() || value_response.lost_focus();
                            if ui.small_button("x").clicked() {
                                remove_index = Some(index);
                            }
                        });
                    }
                }
                if let Some(index) = remove_index {
                    host.apply(|document| {
                        document.variables.remove(index);
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(new_variable_name)
                            .hint_text("name")
                            .desired_width(70.0),
                    );
                    ui.add(egui::DragValue::new(new_variable_value).speed(0.1));
                    let enter = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if ui.button("Add").clicked() || enter {
                        add_requested = true;
                    }
                });
                if add_requested {
                    let name = new_variable_name.trim().to_owned();
                    if !name.is_empty() {
                        let value = *new_variable_value;
                        host.apply(|document| {
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
                        new_variable_name.clear();
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                widgets::section_label(ui, "Evaluated expressions");
                let history = host.document().history.clone();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(220.0)
                    .show(ui, |ui| {
                        if history.is_empty() {
                            ui.label(RichText::new("Nothing evaluated yet").color(theme::FAINT));
                        }
                        for expression in history.iter().rev() {
                            if ui.selectable_label(false, expression).clicked() {
                                picked_expression = Some(expression.clone());
                            }
                        }
                    });
            });

        // Direct edits of persisted fields are wrapped in one undo step each.
        if variable_focus_gained {
            host.history_mut().begin_gesture();
        }
        if variable_focus_lost {
            host.history_mut().end_gesture();
        }

        if let Some(expression) = picked_expression {
            *expression_input = expression;
        }
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        let options = PlotOptions {
            x_min: self.plot_x_min,
            x_max: self.plot_x_max,
            samples: PLOT_SAMPLES,
        };
        let variables = self.host.document().variables.clone();
        let plot = plot::sample(&self.expression_input, &variables, options);

        let mut evaluate_requested = false;
        let mut matrix_action = None;
        let mut gained_focus = false;
        let mut lost_focus = false;

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Expression");
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.expression_input)
                        .hint_text("e.g. sin(x) + x^2")
                        .desired_width(f32::INFINITY),
                );
                gained_focus |= response.gained_focus();
                lost_focus |= response.lost_focus();
                evaluate_requested |= response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ui.button("Evaluate").clicked() {
                    evaluate_requested = true;
                }
            });
            ui.label(
                RichText::new(
                    "Operators + - * / ^ · functions sin cos tan sqrt abs ln log exp · constants pi e · use x for plots",
                )
                .size(11.0)
                .color(theme::MUTED),
            );
            if let Some(result) = &self.evaluation {
                let (text, color) = match result {
                    Evaluation::Value(value) => (format!("= {value}"), theme::SUCCESS),
                    Evaluation::Error(message) => (format!("Error: {message}"), theme::ERROR),
                };
                ui.add_space(4.0);
                ui.label(RichText::new(text).font(egui::FontId::monospace(16.0)).color(color));
            }

            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Plot y = f(x)");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.plot_x_max)
                            .speed(0.1)
                            .prefix("x max: "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.plot_x_min)
                            .speed(0.1)
                            .prefix("x min: "),
                    );
                });
            });
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 260.0),
                egui::Sense::hover(),
            );
            match &plot {
                Ok(points) => {
                    let (y_min, y_max) = plot::y_bounds(points);
                    paint_plot(ui.painter(), rect, points, self.plot_x_min, self.plot_x_max, y_min, y_max);
                }
                Err(error) => {
                    ui.painter().rect_filled(rect, 4.0, theme::SUNKEN);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("No curve: {error}"),
                        egui::FontId::proportional(13.0),
                        theme::MUTED,
                    );
                }
            }

            ui.add_space(10.0);
            ui.separator();
            widgets::section_label(ui, "Matrices");
            ui.label(
                RichText::new("Enter rows separated by ';', coefficients by spaces")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.horizontal(|ui| {
                ui.label("A");
                ui.add(
                    egui::TextEdit::singleline(&mut self.matrix_a)
                        .desired_width(f32::INFINITY),
                );
            });
            ui.horizontal(|ui| {
                ui.label("B");
                ui.add(
                    egui::TextEdit::singleline(&mut self.matrix_b)
                        .desired_width(f32::INFINITY),
                );
            });
            ui.horizontal(|ui| {
                if ui.button("det(A)").clicked() {
                    matrix_action = Some(MatrixAction::Determinant);
                }
                if ui.button("transpose(A)").clicked() {
                    matrix_action = Some(MatrixAction::Transpose);
                }
                if ui.button("A x B").clicked() {
                    matrix_action = Some(MatrixAction::Product);
                }
            });
            if let Some(result) = &self.matrix_result {
                let (text, color) = match result {
                    Ok(value) => (value.clone(), theme::SUCCESS),
                    Err(message) => (format!("Error: {message}"), theme::ERROR),
                };
                ui.label(RichText::new(text).font(egui::FontId::monospace(13.0)).color(color));
            }
        });

        if gained_focus {
            self.gesture_active = true;
        }
        if lost_focus {
            self.finish_gesture();
        }
        if evaluate_requested {
            self.evaluate();
        }
        if let Some(action) = matrix_action {
            self.run_matrix_action(action);
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
    document.history.push(expression);
    if document.history.len() > HISTORY_LIMIT {
        document.history.remove(0);
    }
}

fn parse_matrix_input(text: &str) -> Result<Matrix, MatrixError> {
    let mut rows = Vec::new();
    for line in text.split([';', '\n']) {
        let mut row = Vec::new();
        for token in line.split(|character: char| character.is_whitespace() || character == ',') {
            if token.is_empty() {
                continue;
            }
            let value = token
                .parse::<f64>()
                .map_err(|_| MatrixError::InvalidNumber {
                    token: token.to_owned(),
                })?;
            row.push(value);
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    Matrix::new(rows)
}

#[allow(clippy::too_many_arguments)]
fn paint_plot(
    painter: &egui::Painter,
    rect: egui::Rect,
    points: &[Sample],
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
        let label_y = format!("{x:.1}");
        painter.text(
            egui::pos2(top.x, rect.bottom() - 3.0),
            egui::Align2::CENTER_BOTTOM,
            label_y,
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

    let curve = egui::Stroke::new(2.0, theme::INFO);
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

    #[test]
    fn matrix_text_accepts_semicolons_and_commas() {
        let matrix = parse_matrix_input("1, 2; 3 4").unwrap();
        assert_eq!(matrix, Matrix::from_2x2(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn matrix_text_reports_bad_coefficients() {
        assert!(matches!(
            parse_matrix_input("1 x; 3 4"),
            Err(MatrixError::InvalidNumber { .. })
        ));
        assert_eq!(parse_matrix_input(""), Err(MatrixError::Empty));
    }

    #[test]
    fn history_is_bounded() {
        let mut document = Document::default();
        for index in 0..(HISTORY_LIMIT + 5) {
            record_history(&mut document, format!("expr {index}"));
        }
        assert_eq!(document.history.len(), HISTORY_LIMIT);
        assert_eq!(document.history[0], "expr 5");
    }
}
