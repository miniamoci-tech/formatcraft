//! The "add task" dialogs: one per output format, plus the utilities.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, Ui};
use formatcraft_engine::formats::{self, Category, Format};
use formatcraft_engine::job::{Corner, Crop, Quality, Rotation, Watermark};
use formatcraft_engine::probe::{self, MediaInfo};
use formatcraft_engine::{ImageOptions, MediaOptions, Task};

use crate::i18n::{Lang, tr};
use crate::icons;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Convert(&'static Format),
    JoinVideo,
    JoinAudio,
    Mux,
    Split,
    ScreenRecord,
    MediaInfo,
    PdfMerge,
    PdfSplit,
    PdfCompress,
    ImagesToPdf,
}

impl Kind {
    pub const UTILITIES: [Kind; 6] = [
        Kind::JoinVideo,
        Kind::JoinAudio,
        Kind::Mux,
        Kind::Split,
        Kind::ScreenRecord,
        Kind::MediaInfo,
    ];
    pub const PDF_TOOLS: [Kind; 4] = [
        Kind::PdfMerge,
        Kind::PdfSplit,
        Kind::PdfCompress,
        Kind::ImagesToPdf,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Kind::Convert(_) => "convert_to",
            Kind::JoinVideo => "join_video",
            Kind::JoinAudio => "join_audio",
            Kind::Mux => "mux",
            Kind::Split => "split",
            Kind::ScreenRecord => "screen_record",
            Kind::MediaInfo => "media_info",
            Kind::PdfMerge => "pdf_merge",
            Kind::PdfSplit => "pdf_split",
            Kind::PdfCompress => "pdf_compress",
            Kind::ImagesToPdf => "images_to_pdf",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Kind::Convert(f) => category_icon(f.category),
            Kind::JoinVideo => icons::FILM,
            Kind::JoinAudio => icons::AUDIO_LINES,
            Kind::Mux => icons::COMBINE,
            Kind::Split => icons::SCISSORS,
            Kind::ScreenRecord => icons::RECORD,
            Kind::MediaInfo => icons::INFO,
            Kind::PdfMerge => icons::MERGE,
            Kind::PdfSplit => icons::SPLIT,
            Kind::PdfCompress => icons::SHRINK,
            Kind::ImagesToPdf => icons::IMAGE,
        }
    }

    pub fn title(self, lang: Lang) -> String {
        match self {
            Kind::Convert(f) => format!("{} {}", tr(lang, "convert_to"), f.label),
            other => tr(lang, other.key()).to_string(),
        }
    }

    /// Input file types for the picker.
    fn input_filter(self) -> (&'static str, Vec<&'static str>) {
        let exts = |c| formats::input_extensions(c).to_vec();
        match self {
            Kind::Convert(f) => match f.category {
                Category::Video => ("Video", exts(Category::Video)),
                Category::Audio => ("Audio / Video", exts(Category::Audio)),
                Category::Image => ("Image", exts(Category::Image)),
                Category::Document => ("PDF", vec!["pdf"]),
            },
            Kind::JoinVideo | Kind::Split | Kind::MediaInfo => ("Media", {
                let mut v = exts(Category::Video);
                v.extend(exts(Category::Audio));
                v
            }),
            Kind::JoinAudio => ("Audio", exts(Category::Audio)),
            Kind::PdfMerge | Kind::PdfSplit | Kind::PdfCompress => ("PDF", vec!["pdf"]),
            Kind::ImagesToPdf => ("Image", exts(Category::Image)),
            Kind::Mux | Kind::ScreenRecord => ("Media", vec![]),
        }
    }

    fn uses_file_list(self) -> bool {
        !matches!(self, Kind::Mux | Kind::ScreenRecord)
    }

    fn ordered(self) -> bool {
        matches!(
            self,
            Kind::JoinVideo | Kind::JoinAudio | Kind::PdfMerge | Kind::ImagesToPdf
        )
    }
}

pub fn category_icon(c: Category) -> &'static str {
    match c {
        Category::Video => icons::CLAPPERBOARD,
        Category::Audio => icons::MUSIC,
        Category::Image => icons::IMAGE,
        Category::Document => icons::FILE_TEXT,
    }
}

const RESOLUTIONS: &[(&str, u32, u32)] = &[
    ("3840×2160 (4K)", 3840, 2160),
    ("2560×1440 (QHD)", 2560, 1440),
    ("1920×1080 (Full HD)", 1920, 1080),
    ("1280×720 (HD)", 1280, 720),
    ("854×480", 854, 480),
    ("640×360", 640, 360),
    ("1080×1920 (9:16)", 1080, 1920),
    ("1080×1080 (1:1)", 1080, 1080),
];

pub enum Action {
    None,
    Cancel,
    Submit(Vec<Task>),
}

pub struct Dialog {
    pub kind: Kind,
    pub files: Vec<PathBuf>,
    selected: Option<usize>,
    media: MediaOptions,
    image: ImageOptions,
    format_id: String,
    res_mode: usize, // 0 original, 1..=RESOLUTIONS.len() preset, last custom
    w: u32,
    h: u32,
    trim: bool,
    start: f64,
    end: f64,
    crop_on: bool,
    crop: Crop,
    fps_on: bool,
    fps: f64,
    vb_on: bool,
    vb: u32,
    ab_on: bool,
    ab: u32,
    sr: Option<u32>,
    ch: Option<u8>,
    vol: f64,
    speed: f64,
    wm_on: bool,
    wm_path: Option<PathBuf>,
    wm_opacity: f32,
    wm_corner: Corner,
    scale_on: bool,
    scale_pct: u32,
    resize_on: bool,
    jpeg_q: u8,
    video: Option<PathBuf>,
    audio: Option<PathBuf>,
    segment: f64,
    rec_fps: u32,
    rec_max: f64,
    rec_audio: bool,
    pub error: Option<String>,
    info: Option<(PathBuf, Result<MediaInfo, String>)>,
}

impl Dialog {
    pub fn new(kind: Kind, files: Vec<PathBuf>) -> Dialog {
        let format_id = match kind {
            Kind::JoinAudio => "mp3",
            Kind::JoinVideo | Kind::Mux | Kind::ScreenRecord => "mp4",
            Kind::Convert(f) => f.id,
            _ => "pdf",
        }
        .to_string();
        let mut d = Dialog {
            kind,
            files: Vec::new(),
            selected: None,
            media: MediaOptions::default(),
            image: ImageOptions::default(),
            format_id,
            res_mode: 0,
            w: 1280,
            h: 720,
            trim: false,
            start: 0.0,
            end: 10.0,
            crop_on: false,
            crop: Crop {
                x: 0,
                y: 0,
                width: 640,
                height: 360,
            },
            fps_on: false,
            fps: 30.0,
            vb_on: false,
            vb: 4000,
            ab_on: false,
            ab: 192,
            sr: None,
            ch: None,
            vol: 0.0,
            speed: 1.0,
            wm_on: false,
            wm_path: None,
            wm_opacity: 0.8,
            wm_corner: Corner::BottomRight,
            scale_on: false,
            scale_pct: 50,
            resize_on: false,
            jpeg_q: 90,
            video: None,
            audio: None,
            segment: 60.0,
            rec_fps: 30,
            rec_max: 0.0,
            rec_audio: false,
            error: None,
            info: None,
        };
        d.add_files(files);
        d
    }

    pub fn add_files(&mut self, files: Vec<PathBuf>) {
        for f in files {
            if f.is_dir() {
                self.add_folder(&f);
            } else if !self.files.contains(&f) {
                self.files.push(f);
            }
        }
        if self.kind == Kind::MediaInfo
            && let Some(f) = self.files.last().cloned()
        {
            self.load_info(&f);
        }
        if self.kind == Kind::Mux {
            for f in std::mem::take(&mut self.files) {
                let ext = ext_of(&f);
                if formats::input_extensions(Category::Image).contains(&ext.as_str()) {
                    continue;
                }
                let audio_only = matches!(
                    ext.as_str(),
                    "mp3"
                        | "aac"
                        | "m4a"
                        | "wav"
                        | "flac"
                        | "ogg"
                        | "opus"
                        | "wma"
                        | "aiff"
                        | "ac3"
                );
                if audio_only {
                    self.audio = Some(f);
                } else if self.video.is_none() {
                    self.video = Some(f);
                } else {
                    self.audio = Some(f);
                }
            }
        }
    }

    fn add_folder(&mut self, dir: &Path) {
        let (_, exts) = self.kind.input_filter();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut found: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && exts.contains(&ext_of(p).as_str()))
            .take(10_000)
            .collect();
        found.sort();
        for f in found {
            if !self.files.contains(&f) {
                self.files.push(f);
            }
        }
    }

    fn load_info(&mut self, f: &Path) {
        let r = probe::probe(f).map_err(|e| e.to_string());
        self.info = Some((f.to_path_buf(), r));
    }

    fn pick_files(&mut self) {
        let (name, exts) = self.kind.input_filter();
        let mut dlg = rfd::FileDialog::new();
        if !exts.is_empty() {
            dlg = dlg.add_filter(name, &exts);
        }
        dlg = dlg.add_filter("*", &["*"]);
        if let Some(files) = dlg.pick_files() {
            self.add_files(files);
        }
    }

    fn pick_one(exts: &[&str]) -> Option<PathBuf> {
        let mut dlg = rfd::FileDialog::new();
        if !exts.is_empty() {
            dlg = dlg.add_filter("", exts);
        }
        dlg.add_filter("*", &["*"]).pick_file()
    }

    /// Draws the dialog. Returns what the user decided.
    pub fn show(&mut self, ui: &mut Ui, lang: Lang) -> Action {
        let t = |k| tr(lang, k);
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.kind.icon()).size(26.0));
            ui.heading(self.kind.title(lang));
        });
        ui.separator();

        if self.kind.uses_file_list() {
            self.file_list(ui, lang);
            ui.add_space(6.0);
        }

        egui::ScrollArea::vertical()
            .id_salt("dialog-options")
            .max_height(360.0)
            .auto_shrink([false, true])
            .show(ui, |ui| match self.kind {
                Kind::Convert(f) if f.category == Category::Image => self.image_options(ui, lang),
                Kind::Convert(f) => self.media_options(ui, lang, f),
                Kind::JoinVideo | Kind::JoinAudio => {
                    let cat = if self.kind == Kind::JoinVideo {
                        Category::Video
                    } else {
                        Category::Audio
                    };
                    format_combo(ui, lang, &mut self.format_id, cat);
                    quality_combo(ui, lang, &mut self.media.quality);
                }
                Kind::Mux => self.mux_options(ui, lang),
                Kind::Split => {
                    ui.horizontal(|ui| {
                        ui.label(t("segment"));
                        ui.add(
                            egui::DragValue::new(&mut self.segment)
                                .range(1.0..=86_400.0)
                                .speed(1.0),
                        );
                    });
                }
                Kind::ScreenRecord => {
                    format_combo(ui, lang, &mut self.format_id, Category::Video);
                    ui.horizontal(|ui| {
                        ui.label(t("fps"));
                        ui.add(egui::DragValue::new(&mut self.rec_fps).range(1..=60));
                    });
                    ui.horizontal(|ui| {
                        ui.label(t("max_secs"));
                        ui.add(
                            egui::DragValue::new(&mut self.rec_max)
                                .range(0.0..=86_400.0)
                                .speed(1.0),
                        );
                    });
                    if cfg!(target_os = "macos") {
                        ui.checkbox(&mut self.rec_audio, t("record_audio"));
                    }
                    ui.label(RichText::new(t("recording_hint")).italics());
                }
                Kind::MediaInfo => self.info_view(ui, lang),
                _ => {}
            });

        if let Some(e) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.separator();
        let mut action = Action::None;
        ui.horizontal(|ui| {
            if self.kind == Kind::MediaInfo {
                if ui.button(t("close")).clicked() {
                    action = Action::Cancel;
                }
                return;
            }
            if ui
                .add(
                    egui::Button::new(RichText::new(t("ok")).strong())
                        .min_size(egui::vec2(90.0, 0.0)),
                )
                .clicked()
            {
                match self.build_tasks(lang) {
                    Ok(tasks) => action = Action::Submit(tasks),
                    Err(e) => self.error = Some(e),
                }
            }
            if ui.button(t("cancel")).clicked() {
                action = Action::Cancel;
            }
        });
        action
    }

    fn file_list(&mut self, ui: &mut Ui, lang: Lang) {
        let t = |k| tr(lang, k);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", icons::PLUS, t("add_files")))
                .clicked()
            {
                self.pick_files();
            }
            if self.kind != Kind::MediaInfo
                && ui
                    .button(format!("{} {}", icons::FOLDER_PLUS, t("add_folder")))
                    .clicked()
                && let Some(d) = rfd::FileDialog::new().pick_folder()
            {
                self.add_folder(&d);
            }
            let sel = self.selected.filter(|i| *i < self.files.len());
            if ui
                .add_enabled(sel.is_some(), egui::Button::new(t("remove")))
                .clicked()
                && let Some(i) = sel
            {
                self.files.remove(i);
                self.selected = None;
            }
            if self.kind.ordered() {
                if ui
                    .add_enabled(
                        sel.is_some_and(|i| i > 0),
                        egui::Button::new(format!("{} {}", icons::ARROW_UP, t("up"))),
                    )
                    .clicked()
                    && let Some(i) = sel
                {
                    self.files.swap(i, i - 1);
                    self.selected = Some(i - 1);
                }
                if ui
                    .add_enabled(
                        sel.is_some_and(|i| i + 1 < self.files.len()),
                        egui::Button::new(format!("{} {}", icons::ARROW_DOWN, t("down"))),
                    )
                    .clicked()
                    && let Some(i) = sel
                {
                    self.files.swap(i, i + 1);
                    self.selected = Some(i + 1);
                }
            }
            if ui.button(t("clear")).clicked() {
                self.files.clear();
                self.selected = None;
            }
        });
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("dialog-files")
                .max_height(150.0)
                .min_scrolled_height(150.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.files.is_empty() {
                        ui.weak(t("no_files"));
                    }
                    let mut clicked = None;
                    for (i, f) in self.files.iter().enumerate() {
                        let name = f
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let r = ui
                            .selectable_label(
                                self.selected == Some(i),
                                format!("{:>3}. {name}", i + 1),
                            )
                            .on_hover_text(f.display().to_string());
                        if r.clicked() {
                            clicked = Some(i);
                        }
                    }
                    if let Some(i) = clicked {
                        self.selected = Some(i);
                        if self.kind == Kind::MediaInfo
                            && let Some(f) = self.files.get(i).cloned()
                        {
                            self.load_info(&f);
                        }
                    }
                });
        });
    }

    fn media_options(&mut self, ui: &mut Ui, lang: Lang, f: &Format) {
        let t = |k| tr(lang, k);
        let video = f.category == Category::Video;
        egui::Grid::new("media-opts")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(t("quality"));
                quality_combo(ui, lang, &mut self.media.quality);
                ui.end_row();
                if video {
                    ui.label(t("resolution"));
                    ui.horizontal(|ui| {
                        let label = |m: usize| match m {
                            0 => t("original").to_string(),
                            m if m <= RESOLUTIONS.len() => RESOLUTIONS
                                .get(m - 1)
                                .map(|r| r.0.to_string())
                                .unwrap_or_default(),
                            _ => t("custom").to_string(),
                        };
                        egui::ComboBox::from_id_salt("res")
                            .selected_text(label(self.res_mode))
                            .show_ui(ui, |ui| {
                                for m in 0..=RESOLUTIONS.len() + 1 {
                                    ui.selectable_value(&mut self.res_mode, m, label(m));
                                }
                            });
                        if self.res_mode == RESOLUTIONS.len() + 1 {
                            ui.add(
                                egui::DragValue::new(&mut self.w)
                                    .range(2..=16384)
                                    .prefix("W "),
                            );
                            ui.add(
                                egui::DragValue::new(&mut self.h)
                                    .range(2..=16384)
                                    .prefix("H "),
                            );
                        }
                    });
                    ui.end_row();
                    ui.checkbox(&mut self.fps_on, t("fps"));
                    ui.add_enabled(
                        self.fps_on,
                        egui::DragValue::new(&mut self.fps)
                            .range(1.0..=240.0)
                            .speed(0.5),
                    );
                    ui.end_row();
                    ui.checkbox(&mut self.vb_on, t("video_bitrate"));
                    ui.add_enabled(
                        self.vb_on,
                        egui::DragValue::new(&mut self.vb)
                            .range(50..=200_000)
                            .speed(50),
                    );
                    ui.end_row();
                }
                ui.checkbox(&mut self.ab_on, t("audio_bitrate"));
                ui.add_enabled(
                    self.ab_on,
                    egui::DragValue::new(&mut self.ab).range(8..=512).speed(8),
                );
                ui.end_row();
                ui.label(t("sample_rate"));
                option_combo(
                    ui,
                    "sr",
                    &mut self.sr,
                    &[8000, 22050, 32000, 44100, 48000, 96000],
                    t("original"),
                );
                ui.end_row();
                ui.label(t("channels"));
                option_combo(ui, "ch", &mut self.ch, &[1, 2, 6], t("original"));
                ui.end_row();
                ui.checkbox(&mut self.trim, t("trim"));
                ui.add_enabled_ui(self.trim, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.start)
                                .range(0.0..=1e6)
                                .speed(0.1)
                                .prefix(format!("{} ", t("start_s")))
                                .custom_formatter(|v, _| hms(v)),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.end)
                                .range(0.0..=1e6)
                                .speed(0.1)
                                .prefix(format!("{} ", t("end_s")))
                                .custom_formatter(|v, _| hms(v)),
                        );
                    });
                });
                ui.end_row();
                ui.label(t("volume"));
                ui.add(egui::Slider::new(&mut self.vol, -30.0..=20.0).step_by(0.5));
                ui.end_row();
                ui.label(t("speed"));
                ui.add(
                    egui::Slider::new(&mut self.speed, 0.25..=4.0)
                        .step_by(0.05)
                        .suffix("x"),
                );
                ui.end_row();
                if video {
                    ui.label(t("rotate"));
                    rotation_combo(ui, lang, "rot", &mut self.media.rotate);
                    ui.end_row();
                    ui.checkbox(&mut self.crop_on, t("crop"));
                    ui.add_enabled_ui(self.crop_on, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.crop.x).prefix("X "));
                            ui.add(egui::DragValue::new(&mut self.crop.y).prefix("Y "));
                            ui.add(
                                egui::DragValue::new(&mut self.crop.width)
                                    .range(2..=16384)
                                    .prefix("W "),
                            );
                            ui.add(
                                egui::DragValue::new(&mut self.crop.height)
                                    .range(2..=16384)
                                    .prefix("H "),
                            );
                        });
                    });
                    ui.end_row();
                    ui.checkbox(&mut self.media.no_audio, t("no_audio"));
                    ui.end_row();
                    ui.checkbox(&mut self.wm_on, t("watermark"));
                    ui.add_enabled_ui(self.wm_on, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button(t("choose")).clicked()
                                && let Some(p) =
                                    Self::pick_one(formats::input_extensions(Category::Image))
                            {
                                self.wm_path = Some(p);
                            }
                            if let Some(p) = &self.wm_path {
                                ui.label(
                                    p.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default(),
                                );
                            }
                        });
                    });
                    ui.end_row();
                    if self.wm_on {
                        ui.label(t("opacity"));
                        ui.add(egui::Slider::new(&mut self.wm_opacity, 0.05..=1.0));
                        ui.end_row();
                        ui.label(t("position"));
                        egui::ComboBox::from_id_salt("wmpos")
                            .selected_text(corner_label(self.wm_corner))
                            .show_ui(ui, |ui| {
                                for c in [
                                    Corner::TopLeft,
                                    Corner::TopRight,
                                    Corner::BottomLeft,
                                    Corner::BottomRight,
                                    Corner::Center,
                                ] {
                                    ui.selectable_value(&mut self.wm_corner, c, corner_label(c));
                                }
                            });
                        ui.end_row();
                    }
                }
            });
    }

    fn image_options(&mut self, ui: &mut Ui, lang: Lang) {
        let t = |k| tr(lang, k);
        egui::Grid::new("img-opts")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.checkbox(&mut self.resize_on, t("resolution"));
                ui.add_enabled_ui(self.resize_on, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.w)
                                .range(1..=32768)
                                .prefix("W "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.h)
                                .range(1..=32768)
                                .prefix("H "),
                        );
                    });
                });
                ui.end_row();
                ui.checkbox(&mut self.scale_on, t("scale_percent"));
                ui.add_enabled(
                    self.scale_on && !self.resize_on,
                    egui::Slider::new(&mut self.scale_pct, 1..=400).suffix("%"),
                );
                ui.end_row();
                ui.label(t("rotate"));
                rotation_combo(ui, lang, "img-rot", &mut self.image.rotate);
                ui.end_row();
                ui.checkbox(&mut self.image.grayscale, t("grayscale"));
                ui.end_row();
                if self.format_id == "jpg" {
                    ui.label(t("jpeg_quality"));
                    ui.add(egui::Slider::new(&mut self.jpeg_q, 1..=100));
                    ui.end_row();
                }
            });
    }

    fn mux_options(&mut self, ui: &mut Ui, lang: Lang) {
        let t = |k| tr(lang, k);
        egui::Grid::new("mux-opts")
            .num_columns(3)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(t("video_file"));
                if ui.button(t("choose")).clicked()
                    && let Some(p) = Self::pick_one(formats::input_extensions(Category::Video))
                {
                    self.video = Some(p);
                }
                ui.label(file_name(self.video.as_deref()));
                ui.end_row();
                ui.label(t("audio_file"));
                if ui.button(t("choose")).clicked()
                    && let Some(p) = Self::pick_one(formats::input_extensions(Category::Audio))
                {
                    self.audio = Some(p);
                }
                ui.label(file_name(self.audio.as_deref()));
                ui.end_row();
            });
        format_combo(ui, lang, &mut self.format_id, Category::Video);
    }

    fn info_view(&mut self, ui: &mut Ui, lang: Lang) {
        let t = |k| tr(lang, k);
        let Some((path, info)) = &self.info else {
            return;
        };
        ui.strong(path.display().to_string());
        match info {
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Ok(i) => {
                egui::Grid::new("info")
                    .striped(true)
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(t("container"));
                        ui.label(&i.container);
                        ui.end_row();
                        ui.label(t("duration"));
                        ui.label(i.duration_secs.map(hms).unwrap_or_else(|| "—".into()));
                        ui.end_row();
                        ui.label(t("size"));
                        ui.label(i.size_bytes.map(human_size).unwrap_or_else(|| "—".into()));
                        ui.end_row();
                        ui.label("Bitrate");
                        ui.label(
                            i.bit_rate
                                .map(|b| format!("{} kbit/s", b / 1000))
                                .unwrap_or_else(|| "—".into()),
                        );
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.strong(t("streams"));
                for s in &i.streams {
                    let mut parts = vec![format!("#{} {} · {}", s.index, s.kind, s.codec)];
                    if let (Some(w), Some(h)) = (s.width, s.height) {
                        parts.push(format!("{w}×{h}"));
                    }
                    if let Some(f) = s.fps {
                        parts.push(format!("{f:.2} fps"));
                    }
                    if let Some(r) = s.sample_rate {
                        parts.push(format!("{r} Hz"));
                    }
                    if let Some(c) = s.channels {
                        parts.push(format!("{c} ch"));
                    }
                    if let Some(b) = s.bit_rate {
                        parts.push(format!("{} kbit/s", b / 1000));
                    }
                    if let Some(l) = &s.language {
                        parts.push(l.clone());
                    }
                    ui.label(parts.join(" · "));
                }
            }
        }
    }

    fn collect_media(&self) -> MediaOptions {
        let mut m = self.media.clone();
        let (w, h) = match self.res_mode {
            0 => (None, None),
            n if n <= RESOLUTIONS.len() => RESOLUTIONS
                .get(n - 1)
                .map(|r| (Some(r.1), Some(r.2)))
                .unwrap_or((None, None)),
            _ => (Some(self.w), Some(self.h)),
        };
        m.width = w;
        m.height = h;
        m.fps = self.fps_on.then_some(self.fps);
        m.video_bitrate_k = self.vb_on.then_some(self.vb);
        m.audio_bitrate_k = self.ab_on.then_some(self.ab);
        m.sample_rate = self.sr;
        m.channels = self.ch;
        if self.trim {
            m.start = Some(self.start);
            m.end = (self.end > self.start).then_some(self.end);
        }
        m.crop = self.crop_on.then_some(self.crop);
        m.volume_db = (self.vol.abs() > 0.01).then_some(self.vol);
        m.speed = ((self.speed - 1.0).abs() > 0.001).then_some(self.speed);
        m.watermark = match (&self.wm_path, self.wm_on) {
            (Some(p), true) => Some(Watermark {
                image: p.clone(),
                corner: self.wm_corner,
                opacity: self.wm_opacity,
            }),
            _ => None,
        };
        m
    }

    fn build_tasks(&self, lang: Lang) -> Result<Vec<Task>, String> {
        let need = |n: usize| {
            if self.files.len() < n {
                Err(tr(lang, if n > 1 { "need_two" } else { "no_files" }).to_string())
            } else {
                Ok(())
            }
        };
        let tasks = match self.kind {
            Kind::Convert(f) => {
                need(1)?;
                match f.category {
                    Category::Image => {
                        let mut o = self.image.clone();
                        if self.resize_on {
                            o.width = Some(self.w);
                            o.height = Some(self.h);
                        } else if self.scale_on {
                            o.scale_percent = Some(self.scale_pct);
                        }
                        o.jpeg_quality = Some(self.jpeg_q);
                        self.files
                            .iter()
                            .map(|p| Task::ConvertImage {
                                input: p.clone(),
                                format: f.id.into(),
                                options: o.clone(),
                            })
                            .collect()
                    }
                    _ => {
                        let o = self.collect_media();
                        o.check().map_err(|e| e.to_string())?;
                        self.files
                            .iter()
                            .map(|p| Task::Convert {
                                input: p.clone(),
                                format: f.id.into(),
                                options: o.clone(),
                            })
                            .collect()
                    }
                }
            }
            Kind::JoinVideo | Kind::JoinAudio => {
                need(2)?;
                vec![Task::Join {
                    inputs: self.files.clone(),
                    format: self.format_id.clone(),
                    options: MediaOptions {
                        quality: self.media.quality,
                        ..Default::default()
                    },
                }]
            }
            Kind::Mux => match (&self.video, &self.audio) {
                (Some(v), Some(a)) => vec![Task::Mux {
                    video: v.clone(),
                    audio: a.clone(),
                    format: self.format_id.clone(),
                }],
                _ => return Err(tr(lang, "need_two").to_string()),
            },
            Kind::Split => {
                need(1)?;
                self.files
                    .iter()
                    .map(|p| Task::Split {
                        input: p.clone(),
                        segment_secs: self.segment,
                    })
                    .collect()
            }
            Kind::ScreenRecord => vec![Task::ScreenRecord {
                format: self.format_id.clone(),
                fps: Some(self.rec_fps),
                max_secs: (self.rec_max > 0.0).then_some(self.rec_max),
                record_audio: self.rec_audio,
            }],
            Kind::PdfMerge => {
                need(2)?;
                vec![Task::PdfMerge {
                    inputs: self.files.clone(),
                }]
            }
            Kind::PdfSplit => {
                need(1)?;
                self.files
                    .iter()
                    .map(|p| Task::PdfSplit { input: p.clone() })
                    .collect()
            }
            Kind::PdfCompress => {
                need(1)?;
                self.files
                    .iter()
                    .map(|p| Task::PdfCompress { input: p.clone() })
                    .collect()
            }
            Kind::ImagesToPdf => {
                need(1)?;
                vec![Task::ImagesToPdf {
                    inputs: self.files.clone(),
                }]
            }
            Kind::MediaInfo => Vec::new(),
        };
        for t in &tasks {
            t.validate().map_err(|e| e.to_string())?;
        }
        Ok(tasks)
    }
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn file_name(p: Option<&Path>) -> String {
    p.and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "—".into())
}

fn quality_combo(ui: &mut Ui, lang: Lang, q: &mut Quality) {
    let label = |q: Quality| match q {
        Quality::High => tr(lang, "high"),
        Quality::Medium => tr(lang, "medium"),
        Quality::Low => tr(lang, "low"),
    };
    egui::ComboBox::from_id_salt("quality")
        .selected_text(label(*q))
        .show_ui(ui, |ui| {
            for v in [Quality::High, Quality::Medium, Quality::Low] {
                ui.selectable_value(q, v, label(v));
            }
        });
}

fn format_combo(ui: &mut Ui, lang: Lang, id: &mut String, cat: Category) {
    ui.horizontal(|ui| {
        ui.label(tr(lang, "format"));
        let label = formats::find(id).map(|f| f.label).unwrap_or("?");
        egui::ComboBox::from_id_salt(("fmt", cat.id()))
            .selected_text(label)
            .show_ui(ui, |ui| {
                for f in
                    formats::in_category(cat).filter(|f| !f.id.contains("anim") && f.id != "gif")
                {
                    ui.selectable_value(id, f.id.to_string(), f.label);
                }
            });
    });
}

fn rotation_combo(ui: &mut Ui, lang: Lang, salt: &str, r: &mut Rotation) {
    let label = |r: Rotation| {
        tr(
            lang,
            match r {
                Rotation::None => "none",
                Rotation::Cw90 => "cw90",
                Rotation::Ccw90 => "ccw90",
                Rotation::R180 => "r180",
                Rotation::FlipH => "fliph",
                Rotation::FlipV => "flipv",
            },
        )
    };
    egui::ComboBox::from_id_salt(salt)
        .selected_text(label(*r))
        .show_ui(ui, |ui| {
            for v in [
                Rotation::None,
                Rotation::Cw90,
                Rotation::Ccw90,
                Rotation::R180,
                Rotation::FlipH,
                Rotation::FlipV,
            ] {
                ui.selectable_value(r, v, label(v));
            }
        });
}

fn option_combo<T: Copy + PartialEq + std::fmt::Display>(
    ui: &mut Ui,
    salt: &str,
    v: &mut Option<T>,
    choices: &[T],
    none: &str,
) {
    let text = v.map(|x| x.to_string()).unwrap_or_else(|| none.to_string());
    egui::ComboBox::from_id_salt(salt)
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(v, None, none);
            for c in choices {
                ui.selectable_value(v, Some(*c), c.to_string());
            }
        });
}

fn corner_label(c: Corner) -> &'static str {
    match c {
        Corner::TopLeft => "TL",
        Corner::TopRight => "TR",
        Corner::BottomLeft => "BL",
        Corner::BottomRight => "BR",
        Corner::Center => "C",
    }
}

pub fn hms(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "—".into();
    }
    let total = secs.min(1e9);
    let h = (total / 3600.0).floor();
    let m = ((total - h * 3600.0) / 60.0).floor();
    let s = total - h * 3600.0 - m * 60.0;
    format!("{:02}:{:02}:{:04.1}", h as u64, m as u64, s)
}

pub fn human_size(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < units.len() {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", units.get(i).copied().unwrap_or("B"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_helpers() {
        assert_eq!(hms(3725.5), "01:02:05.5");
        assert_eq!(hms(f64::NAN), "—");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(u64::MAX), "16777216.0 TB");
    }

    #[test]
    fn convert_dialog_builds_tasks() {
        let d = std::env::temp_dir().join(format!("fc-dlg-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.mkv");
        std::fs::write(&f, b"x").unwrap();
        let mut dlg = Dialog::new(
            Kind::Convert(formats::find("mp4").unwrap()),
            vec![f.clone(), f.clone()],
        );
        assert_eq!(dlg.files.len(), 1, "duplicates are ignored");
        dlg.trim = true;
        dlg.start = 5.0;
        dlg.end = 2.0;
        let tasks = dlg.build_tasks(Lang::En).unwrap();
        match &tasks[0] {
            Task::Convert { options, .. } => assert_eq!(options.end, None),
            _ => panic!(),
        }
        let empty = Dialog::new(Kind::PdfMerge, vec![]);
        assert!(empty.build_tasks(Lang::It).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
