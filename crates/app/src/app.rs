//! Main window: format sidebar, task list, toolbar.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Ui};
use formatcraft_engine::ffmpeg::Ffmpeg;
use formatcraft_engine::formats::{self, Category};
use formatcraft_engine::queue::EntryView;
use formatcraft_engine::{Queue, Status, Task};
use serde::{Deserialize, Serialize};

use crate::dialogs::{self, Action, Dialog, Kind, category_icon};
use crate::i18n::{Lang, tr};
use crate::icons;

pub const ACCENT: Color32 = Color32::from_rgb(0x2E, 0x8B, 0xC0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub output_dir: PathBuf,
    pub same_as_source: bool,
    pub workers: usize,
    pub lang: Lang,
    pub auto_start: bool,
    pub dark: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let output_dir = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|h| {
                let videos = if cfg!(target_os = "macos") {
                    "Movies"
                } else {
                    "Videos"
                };
                PathBuf::from(h).join(videos).join("FormatCraft")
            })
            .unwrap_or_else(|| PathBuf::from("FormatCraft"));
        Settings {
            output_dir,
            same_as_source: false,
            workers: std::thread::available_parallelism()
                .map(|n| (n.get() / 4).clamp(1, 4))
                .unwrap_or(2),
            lang: Lang::from_system(),
            auto_start: true,
            dark: true,
        }
    }
}

const SETTINGS_KEY: &str = "formatcraft-settings-v1";

pub struct FormatCraftApp {
    settings: Settings,
    queue: Queue,
    dialog: Option<Dialog>,
    drop_pending: Vec<PathBuf>,
    settings_open: bool,
    about_open: bool,
    toast: Option<(String, Instant)>,
    ffmpeg: Result<String, String>,
    was_busy: bool,
}

impl FormatCraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings = cc
            .storage
            .and_then(|s| s.get_string(SETTINGS_KEY))
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let queue = Queue::new(settings.workers);
        let ctx = cc.egui_ctx.clone();
        queue.set_notify(move || ctx.request_repaint());
        icons::install(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, settings.dark);
        let ffmpeg = Ffmpeg::get()
            .map(|f| f.version.clone())
            .map_err(|e| e.to_string());
        let mut app = FormatCraftApp {
            settings,
            queue,
            dialog: None,
            drop_pending: Vec::new(),
            settings_open: false,
            about_open: false,
            toast: None,
            ffmpeg,
            was_busy: false,
        };
        // Files passed on the command line (e.g. "Open with").
        let args: Vec<PathBuf> = std::env::args_os()
            .skip(1)
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        if !args.is_empty() {
            app.drop_pending = args;
        }
        app
    }

    fn t(&self, key: &'static str) -> &'static str {
        tr(self.settings.lang, key)
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn open_dialog(&mut self, kind: Kind, files: Vec<PathBuf>) {
        let ffmpeg_needed = match kind {
            Kind::Convert(f) => matches!(f.category, Category::Video | Category::Audio),
            Kind::PdfMerge | Kind::PdfSplit | Kind::PdfCompress | Kind::ImagesToPdf => false,
            _ => true,
        };
        if ffmpeg_needed && self.ffmpeg.is_err() {
            self.toast(self.t("ffmpeg_missing"));
            return;
        }
        self.dialog = Some(Dialog::new(kind, files));
    }

    fn out_dir_for(&self, task: &Task) -> PathBuf {
        if self.settings.same_as_source
            && let Some(dir) = task.primary_input().and_then(Path::parent)
        {
            return dir.to_path_buf();
        }
        self.settings.output_dir.clone()
    }

    fn submit(&mut self, tasks: Vec<Task>) -> Result<(), String> {
        let recording = tasks.iter().any(|t| matches!(t, Task::ScreenRecord { .. }));
        let mut jobs = Vec::new();
        for task in tasks {
            let dir = self.out_dir_for(&task);
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            jobs.push(task.into_job(&dir).map_err(|e| e.to_string())?);
        }
        let n = jobs.len();
        for j in jobs {
            self.queue.add(j);
        }
        if self.settings.auto_start || recording {
            self.queue.start();
        }
        self.toast(format!("{n} {}", self.t("added")));
        Ok(())
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let running = self.queue.is_running();
            if ui
                .add_enabled(
                    !running,
                    egui::Button::new(
                        RichText::new(format!("{} {}", icons::PLAY, self.t("start")))
                            .strong()
                            .size(15.0),
                    )
                    .fill(ACCENT.gamma_multiply(0.85)),
                )
                .clicked()
            {
                self.queue.start();
            }
            if ui
                .add_enabled(
                    running,
                    egui::Button::new(
                        RichText::new(format!("{} {}", icons::STOP, self.t("stop"))).size(15.0),
                    ),
                )
                .clicked()
            {
                self.queue.stop();
            }
            if ui
                .button(format!("{} {}", icons::ERASER, self.t("clear_done")))
                .clicked()
            {
                self.queue.clear_finished();
            }
            ui.separator();
            ui.label(format!("{} {}:", icons::FOLDER, self.t("output_folder")));
            let shown = if self.settings.same_as_source {
                self.t("same_as_source").to_string()
            } else {
                self.settings.output_dir.display().to_string()
            };
            ui.add(egui::Label::new(RichText::new(shown).monospace()).truncate())
                .on_hover_text(self.settings.output_dir.display().to_string());
            if ui.button(self.t("browse")).clicked()
                && let Some(d) = rfd::FileDialog::new()
                    .set_directory(&self.settings.output_dir)
                    .pick_folder()
            {
                self.settings.output_dir = d;
                self.settings.same_as_source = false;
            }
            if ui.button(self.t("open")).clicked() {
                let _ = std::fs::create_dir_all(&self.settings.output_dir);
                reveal(&self.settings.output_dir, false);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(format!("{} {}", icons::INFO, self.t("about")))
                    .clicked()
                {
                    self.about_open = true;
                }
                if ui
                    .button(format!("{} {}", icons::SETTINGS, self.t("settings")))
                    .clicked()
                {
                    self.settings_open = true;
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let mut open: Option<Kind> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::LAYERS).color(ACCENT).size(22.0));
                ui.label(RichText::new("FormatCraft").size(20.0).strong());
            });
            ui.add_space(4.0);
            for cat in Category::ALL {
                let title = format!("{}  {}", category_icon(cat), self.t(cat.id()));
                egui::CollapsingHeader::new(RichText::new(title).size(16.0).strong())
                    .id_salt(cat.id())
                    .default_open(matches!(cat, Category::Video | Category::Audio))
                    .show(ui, |ui| {
                        let items: Vec<Kind> = if cat == Category::Document {
                            Kind::PDF_TOOLS.to_vec()
                        } else {
                            formats::in_category(cat).map(Kind::Convert).collect()
                        };
                        tile_grid(ui, cat.id(), &items, self.settings.lang, &mut open);
                    });
            }
            egui::CollapsingHeader::new(
                RichText::new(format!("{}  {}", icons::WRENCH, self.t("utilities")))
                    .size(16.0)
                    .strong(),
            )
            .id_salt("utilities")
            .default_open(true)
            .show(ui, |ui| {
                tile_grid(ui, "util", &Kind::UTILITIES, self.settings.lang, &mut open);
            });
        });
        if let Some(k) = open {
            self.open_dialog(k, Vec::new());
        }
    }

    fn task_list(&mut self, ui: &mut Ui) {
        let entries = self.queue.snapshot();
        if entries.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(format!("{}\n\n{}", icons::UPLOAD, self.t("empty_list")))
                        .size(18.0)
                        .weak(),
                );
            });
            return;
        }
        let lang = self.settings.lang;
        let avail = (ui.available_width() - 80.0).max(300.0);
        let widths = [avail * 0.34, avail * 0.26, avail * 0.26];
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("tasks")
                    .num_columns(4)
                    .striped(true)
                    .spacing([14.0, 8.0])
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        for (key, w) in ["file", "output", "status"].into_iter().zip(widths) {
                            ui.scope(|ui| {
                                ui.set_width(w);
                                ui.strong(tr(lang, key));
                            });
                        }
                        ui.label("");
                        ui.end_row();
                        for e in &entries {
                            self.task_row(ui, e, widths);
                            ui.end_row();
                        }
                    });
            });
    }

    fn task_row(&mut self, ui: &mut Ui, e: &EntryView, widths: [f32; 3]) {
        let lang = self.settings.lang;
        let task = &e.job.task;
        let source = match task.inputs().as_slice() {
            [] => tr(lang, "screen_record").to_string(),
            [one] => file_label(one),
            [first, rest @ ..] => format!("{} (+{})", file_label(first), rest.len()),
        };
        ui.horizontal(|ui| {
            ui.set_width(widths[0]);
            ui.label(task_icon(task));
            ui.add(egui::Label::new(source).truncate()).on_hover_text(
                task.inputs()
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        });
        let out_name = match &e.status {
            Status::Done(p) => file_label(p),
            _ => file_label(&e.job.output),
        };
        ui.horizontal(|ui| {
            ui.set_width(widths[1]);
            ui.label(
                RichText::new(task.format_id().to_uppercase())
                    .monospace()
                    .color(ACCENT),
            );
            ui.add(egui::Label::new(out_name).truncate());
        });
        ui.scope(|ui| {
            ui.set_width(widths[2]);
            match &e.status {
                Status::Pending => {
                    ui.weak(tr(lang, "pending"));
                }
                Status::Running => {
                    let mut text = format!("{:.0}%", e.progress * 100.0);
                    if let Some(el) = e.elapsed {
                        text.push_str(&format!("  ·  {}", dialogs::hms(el.as_secs_f64())));
                    }
                    ui.add(
                        egui::ProgressBar::new(e.progress)
                            .desired_width(widths[2])
                            .text(text)
                            .fill(ACCENT)
                            .animate(true),
                    );
                }
                Status::Done(_) => {
                    let mut s = format!("{} {}", icons::CHECK, tr(lang, "done"));
                    if let Some(el) = e.elapsed {
                        s.push_str(&format!(" ({})", dialogs::hms(el.as_secs_f64())));
                    }
                    ui.colored_label(Color32::from_rgb(0x3C, 0xB3, 0x71), s);
                }
                Status::Failed(msg) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{} {}", icons::ALERT, tr(lang, "failed")),
                    )
                    .on_hover_text(msg);
                }
                Status::Cancelled => {
                    ui.weak(tr(lang, "cancelled"));
                }
            }
        });
        ui.horizontal(|ui| {
            match &e.status {
                Status::Done(p) => {
                    if ui
                        .small_button(icons::FOLDER_OPEN)
                        .on_hover_text(tr(lang, "open_folder"))
                        .clicked()
                    {
                        reveal(p, true);
                    }
                }
                Status::Failed(msg) => {
                    if ui.small_button(icons::COPY).on_hover_text(msg).clicked() {
                        ui.ctx().copy_text(msg.clone());
                    }
                    if ui
                        .small_button(icons::RETRY)
                        .on_hover_text(tr(lang, "retry"))
                        .clicked()
                    {
                        self.queue.retry(e.id);
                    }
                }
                Status::Cancelled => {
                    if ui
                        .small_button(icons::RETRY)
                        .on_hover_text(tr(lang, "retry"))
                        .clicked()
                    {
                        self.queue.retry(e.id);
                    }
                }
                Status::Running => {
                    let label = if matches!(task, Task::ScreenRecord { .. }) {
                        icons::STOP
                    } else {
                        icons::CIRCLE_STOP
                    };
                    if ui
                        .small_button(label)
                        .on_hover_text(tr(lang, "stop"))
                        .clicked()
                    {
                        self.queue.cancel(e.id);
                    }
                }
                Status::Pending => {}
            }
            if !matches!(e.status, Status::Running)
                && ui
                    .small_button(icons::X)
                    .on_hover_text(tr(lang, "remove"))
                    .clicked()
            {
                self.queue.remove(e.id);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let entries = self.queue.snapshot();
        let count = |f: fn(&Status) -> bool| entries.iter().filter(|e| f(&e.status)).count();
        let done = count(|s| matches!(s, Status::Done(_)));
        let failed = count(|s| matches!(s, Status::Failed(_)));
        let running = count(|s| matches!(s, Status::Running));
        ui.horizontal(|ui| {
            match &self.ffmpeg {
                Ok(v) => {
                    let short: String = v
                        .split(" Copyright")
                        .next()
                        .unwrap_or(v)
                        .chars()
                        .take(60)
                        .collect();
                    ui.weak(format!("{} {short}", icons::SLIDERS));
                }
                Err(_) => {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("{} {}", icons::WARNING, self.t("ffmpeg_missing")),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} {} · {} {running} · {} {done} · {} {failed}",
                    entries.len(),
                    self.t("tasks"),
                    icons::PLAY,
                    icons::CHECK,
                    icons::ALERT
                ));
            });
        });
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        if let Some(d) = self.dialog.as_mut() {
            d.add_files(dropped);
        } else {
            self.drop_pending = dropped;
        }
    }

    /// "Convert dropped files to…" chooser.
    fn drop_chooser(&mut self, ctx: &egui::Context) {
        if self.drop_pending.is_empty() || self.dialog.is_some() {
            return;
        }
        let lang = self.settings.lang;
        let guess = guess_category(&self.drop_pending);
        let mut chosen: Option<Kind> = None;
        let mut cancel = false;
        let n = self.drop_pending.len();
        egui::Modal::new(egui::Id::unique("drop-chooser")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(format!("{} ({n})", tr(lang, "drop_title")));
            ui.label(tr(lang, "drop_hint"));
            ui.add_space(6.0);
            let cats: Vec<Category> = match guess {
                Some(Category::Video) => vec![Category::Video, Category::Audio],
                Some(c) => vec![c],
                None => Category::ALL.to_vec(),
            };
            for c in cats {
                ui.strong(format!("{} {}", category_icon(c), tr(lang, c.id())));
                let items: Vec<Kind> = if c == Category::Document {
                    Kind::PDF_TOOLS.to_vec()
                } else {
                    formats::in_category(c).map(Kind::Convert).collect()
                };
                ui.horizontal_wrapped(|ui| {
                    for k in items {
                        let label = match k {
                            Kind::Convert(f) => f.label.to_string(),
                            other => other.title(lang),
                        };
                        if ui.button(label).clicked() {
                            chosen = Some(k);
                        }
                    }
                });
                ui.add_space(4.0);
            }
            ui.separator();
            if ui.button(tr(lang, "cancel")).clicked() {
                cancel = true;
            }
        });
        if let Some(k) = chosen {
            let files = std::mem::take(&mut self.drop_pending);
            self.open_dialog(k, files);
        } else if cancel {
            self.drop_pending.clear();
        }
    }

    fn dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let lang = self.settings.lang;
        let mut action = Action::None;
        let resp = egui::Modal::new(egui::Id::unique("task-dialog")).show(ctx, |ui| {
            ui.set_width(560.0);
            action = dialog.show(ui, lang);
        });
        if resp.should_close() && matches!(action, Action::None) {
            action = Action::Cancel;
        }
        match action {
            Action::None => {}
            Action::Cancel => self.dialog = None,
            Action::Submit(tasks) => match self.submit(tasks) {
                Ok(()) => self.dialog = None,
                Err(e) => {
                    if let Some(d) = self.dialog.as_mut() {
                        d.error = Some(e);
                    }
                }
            },
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let lang = self.settings.lang;
        let mut open = true;
        let mut dark_changed = false;
        egui::Window::new(format!("{} {}", icons::SETTINGS, tr(lang, "settings")))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("settings")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(tr(lang, "language"));
                        egui::ComboBox::from_id_salt("lang")
                            .selected_text(self.settings.lang.name())
                            .show_ui(ui, |ui| {
                                for l in [Lang::En, Lang::It] {
                                    ui.selectable_value(&mut self.settings.lang, l, l.name());
                                }
                            });
                        ui.end_row();
                        ui.label(tr(lang, "workers"));
                        if ui
                            .add(egui::Slider::new(&mut self.settings.workers, 1..=8))
                            .changed()
                        {
                            self.queue.set_workers(self.settings.workers);
                        }
                        ui.end_row();
                        ui.label(tr(lang, "output_folder"));
                        ui.checkbox(
                            &mut self.settings.same_as_source,
                            tr(lang, "same_as_source"),
                        );
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut self.settings.auto_start, tr(lang, "auto_start"));
                        ui.end_row();
                        ui.label("");
                        dark_changed = ui
                            .checkbox(&mut self.settings.dark, tr(lang, "dark"))
                            .changed();
                        ui.end_row();
                    });
            });
        if dark_changed {
            apply_theme(ctx, self.settings.dark);
        }
        self.settings_open = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let lang = self.settings.lang;
        let mut open = true;
        let ffmpeg = self.ffmpeg.clone();
        egui::Window::new(format!("{} {}", icons::INFO, tr(lang, "about")))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.heading(format!("FormatCraft {}", env!("CARGO_PKG_VERSION")));
                ui.label(tr(lang, "about_text"));
                ui.add_space(6.0);
                match ffmpeg {
                    Ok(v) => ui.weak(v),
                    Err(e) => ui.colored_label(ui.visuals().warn_fg_color, e),
                };
                ui.hyperlink("https://github.com/miniamoci-tech/formatcraft");
            });
        self.about_open = open;
    }

    fn toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = &self.toast else {
            return;
        };
        if at.elapsed() > Duration::from_secs(4) {
            self.toast = None;
            return;
        }
        let msg = msg.clone();
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(msg).size(15.0)).extend());
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

impl eframe::App for FormatCraftApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_drops(&ctx);

        let busy = self.queue.is_running() || self.queue.active_workers() > 0;
        if self.was_busy && !busy {
            let entries = self.queue.snapshot();
            if !entries.is_empty() && entries.iter().all(|e| e.status.is_finished()) {
                self.toast(format!("{} {}", icons::CHECK, self.t("all_done")));
            }
        }
        self.was_busy = busy;
        if busy {
            // Keeps elapsed times ticking even without progress updates.
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("statusbar").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("formats")
            .resizable(true)
            .default_size(320.0)
            .min_size(220.0)
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
            self.task_list(ui);
            if hovering {
                let r = ui.max_rect();
                ui.painter().rect_stroke(
                    r.shrink(6.0),
                    8.0,
                    egui::Stroke::new(3.0, ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
        });

        self.drop_chooser(&ctx);
        self.dialog_window(&ctx);
        self.settings_window(&ctx);
        self.about_window(&ctx);
        self.toast_overlay(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Ok(s) = serde_json::to_string(&self.settings) {
            storage.set_string(SETTINGS_KEY, s);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Stop FFmpeg children so no half-written temp files linger.
        self.queue.stop();
        let t0 = Instant::now();
        while self.queue.active_workers() > 0 && t0.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn tile_grid(ui: &mut Ui, salt: &str, items: &[Kind], lang: Lang, open: &mut Option<Kind>) {
    let w = ((ui.available_width() - 8.0) / 2.0).max(80.0);
    egui::Grid::new(("tiles", salt))
        .num_columns(2)
        .spacing([6.0, 6.0])
        .show(ui, |ui| {
            for (i, k) in items.iter().enumerate() {
                let label = match k {
                    Kind::Convert(f) => f.label.to_string(),
                    other => other.title(lang),
                };
                let btn =
                    egui::Button::new(RichText::new(format!("{}  {label}", k.icon())).size(13.5))
                        .min_size(egui::vec2(w, 34.0))
                        .wrap_mode(egui::TextWrapMode::Truncate);
                let tooltip = match k {
                    Kind::Convert(f) => format!("{} .{}", tr(lang, "convert_to"), f.ext),
                    other => other.title(lang),
                };
                if ui.add(btn).on_hover_text(tooltip).clicked() {
                    *open = Some(*k);
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
}

fn task_icon(t: &Task) -> &'static str {
    match t {
        Task::Convert { format, .. } => formats::find(format)
            .map(|f| category_icon(f.category))
            .unwrap_or(icons::CLAPPERBOARD),
        Task::ConvertImage { .. } => icons::IMAGE,
        Task::Join { .. } => icons::FILM,
        Task::Mux { .. } => icons::COMBINE,
        Task::Split { .. } => icons::SCISSORS,
        Task::ScreenRecord { .. } => icons::RECORD,
        Task::PdfMerge { .. }
        | Task::PdfSplit { .. }
        | Task::PdfCompress { .. }
        | Task::ImagesToPdf { .. } => icons::FILE_TEXT,
    }
}

fn file_label(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

fn guess_category(files: &[PathBuf]) -> Option<Category> {
    let ext = files
        .first()?
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase();
    if ext == "pdf" {
        return Some(Category::Document);
    }
    [Category::Image, Category::Video, Category::Audio]
        .into_iter()
        .find(|c| formats::input_extensions(*c).contains(&ext.as_str()))
}

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let mut visuals = theme.default_visuals();
        visuals.selection.bg_fill = ACCENT;
        visuals.hyperlink_color = ACCENT;
        ctx.set_visuals_of(theme, visuals);
    }
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

/// Shows a file (selected) or folder in the system file manager.
pub fn reveal(path: &Path, select: bool) {
    // Opening the file manager is best-effort.
    let _ = spawn_file_manager(path, select && path.is_file());
}

#[cfg(windows)]
fn spawn_file_manager(path: &Path, select: bool) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("explorer");
    if select {
        c.raw_arg(format!("/select,\"{}\"", path.display()));
    } else {
        c.arg(path);
    }
    c.spawn()
}

#[cfg(target_os = "macos")]
fn spawn_file_manager(path: &Path, select: bool) -> std::io::Result<std::process::Child> {
    let mut c = std::process::Command::new("open");
    if select {
        c.arg("-R");
    }
    c.arg(path).spawn()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn spawn_file_manager(path: &Path, select: bool) -> std::io::Result<std::process::Child> {
    let target = if select {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    std::process::Command::new("xdg-open").arg(target).spawn()
}
