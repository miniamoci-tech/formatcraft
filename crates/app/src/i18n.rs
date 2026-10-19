//! Interface strings in English and Italian.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Lang {
    #[default]
    En,
    It,
}

impl Lang {
    pub fn from_system() -> Lang {
        match sys_locale::get_locale() {
            Some(l) if l.to_ascii_lowercase().starts_with("it") => Lang::It,
            _ => Lang::En,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::It => "Italiano",
        }
    }
}

/// (key, English, Italian)
static STRINGS: &[(&str, &str, &str)] = &[
    ("video", "Video", "Video"),
    ("audio", "Audio", "Audio"),
    ("image", "Image", "Immagini"),
    ("document", "Document", "Documenti"),
    ("utilities", "Utilities", "Utilità"),
    ("start", "Start", "Avvia"),
    ("stop", "Stop", "Ferma"),
    ("clear_done", "Clear finished", "Rimuovi completati"),
    ("remove", "Remove", "Rimuovi"),
    ("retry", "Retry", "Riprova"),
    ("cancel", "Cancel", "Annulla"),
    ("ok", "OK", "OK"),
    ("close", "Close", "Chiudi"),
    ("settings", "Settings", "Impostazioni"),
    ("about", "About", "Informazioni"),
    ("output_folder", "Output folder", "Cartella di destinazione"),
    (
        "same_as_source",
        "Same folder as the source",
        "Stessa cartella del file originale",
    ),
    ("browse", "Browse…", "Sfoglia…"),
    ("open", "Open", "Apri"),
    ("open_folder", "Open folder", "Apri cartella"),
    ("add_files", "Add files…", "Aggiungi file…"),
    ("add_folder", "Add folder…", "Aggiungi cartella…"),
    ("clear", "Clear", "Svuota"),
    ("files", "Files", "File"),
    ("options", "Options", "Opzioni"),
    ("quality", "Quality", "Qualità"),
    ("high", "High", "Alta"),
    ("medium", "Medium", "Media"),
    ("low", "Low", "Bassa"),
    ("resolution", "Resolution", "Risoluzione"),
    ("original", "Original", "Originale"),
    ("custom", "Custom", "Personalizzata"),
    ("width", "Width", "Larghezza"),
    ("height", "Height", "Altezza"),
    ("fps", "Frame rate", "Fotogrammi al secondo"),
    (
        "video_bitrate",
        "Video bitrate (kbit/s)",
        "Bitrate video (kbit/s)",
    ),
    (
        "audio_bitrate",
        "Audio bitrate (kbit/s)",
        "Bitrate audio (kbit/s)",
    ),
    ("sample_rate", "Sample rate (Hz)", "Frequenza (Hz)"),
    ("channels", "Channels", "Canali"),
    (
        "trim",
        "Cut (start/end, seconds)",
        "Taglia (inizio/fine, secondi)",
    ),
    ("start_s", "Start", "Inizio"),
    ("end_s", "End", "Fine"),
    ("crop", "Crop", "Ritaglia"),
    ("rotate", "Rotate / flip", "Ruota / specchia"),
    ("none", "None", "Nessuna"),
    ("cw90", "90° clockwise", "90° orario"),
    ("ccw90", "90° counter-clockwise", "90° antiorario"),
    ("r180", "180°", "180°"),
    ("fliph", "Mirror horizontally", "Specchia orizzontale"),
    ("flipv", "Mirror vertically", "Specchia verticale"),
    ("no_audio", "Remove audio", "Rimuovi audio"),
    ("volume", "Volume (dB)", "Volume (dB)"),
    ("speed", "Speed", "Velocità"),
    ("watermark", "Watermark image", "Immagine filigrana"),
    ("opacity", "Opacity", "Opacità"),
    ("position", "Position", "Posizione"),
    ("scale_percent", "Scale (%)", "Scala (%)"),
    ("grayscale", "Grayscale", "Scala di grigi"),
    ("jpeg_quality", "JPEG quality", "Qualità JPEG"),
    ("file", "File", "File"),
    ("output", "Output", "Uscita"),
    ("status", "Status", "Stato"),
    ("pending", "Waiting", "In attesa"),
    ("running", "Converting", "Conversione"),
    ("done", "Done", "Completato"),
    ("failed", "Failed", "Errore"),
    ("cancelled", "Cancelled", "Annullato"),
    (
        "empty_list",
        "Choose an output format on the left, or drop files here.",
        "Scegli un formato di destinazione a sinistra, oppure trascina qui i file.",
    ),
    ("join_video", "Video joiner", "Unisci video"),
    ("join_audio", "Audio joiner", "Unisci audio"),
    ("mux", "Mux (replace audio)", "Mux (sostituisci audio)"),
    ("split", "Split", "Dividi"),
    ("screen_record", "Screen recorder", "Registra schermo"),
    ("media_info", "Media info", "Informazioni file"),
    ("pdf_merge", "Merge PDF", "Unisci PDF"),
    ("pdf_split", "Split PDF", "Dividi PDF"),
    ("pdf_compress", "Compress PDF", "Comprimi PDF"),
    ("images_to_pdf", "Images → PDF", "Immagini → PDF"),
    ("video_file", "Video file", "File video"),
    ("audio_file", "Audio file", "File audio"),
    ("choose", "Choose…", "Scegli…"),
    (
        "segment",
        "Part length (seconds)",
        "Durata di ogni parte (secondi)",
    ),
    (
        "max_secs",
        "Stop after (seconds, 0 = until stopped)",
        "Ferma dopo (secondi, 0 = finché non fermi)",
    ),
    (
        "record_audio",
        "Record audio (macOS)",
        "Registra audio (macOS)",
    ),
    ("format", "Format", "Formato"),
    ("up", "Up", "Su"),
    ("down", "Down", "Giù"),
    (
        "workers",
        "Simultaneous conversions",
        "Conversioni simultanee",
    ),
    ("language", "Language", "Lingua"),
    (
        "auto_start",
        "Start converting as soon as files are added",
        "Avvia la conversione appena aggiungi i file",
    ),
    ("dark", "Dark theme", "Tema scuro"),
    (
        "ffmpeg_missing",
        "FFmpeg not found: audio/video features are unavailable. Images and PDFs still work.",
        "FFmpeg non trovato: le funzioni audio/video non sono disponibili. Immagini e PDF funzionano.",
    ),
    (
        "drop_title",
        "Convert dropped files",
        "Converti i file trascinati",
    ),
    (
        "drop_hint",
        "Choose the output format:",
        "Scegli il formato di destinazione:",
    ),
    (
        "all_done",
        "All tasks finished",
        "Tutte le attività completate",
    ),
    ("added", "tasks added", "attività aggiunte"),
    ("error", "Error", "Errore"),
    ("duration", "Duration", "Durata"),
    ("size", "Size", "Dimensione"),
    ("container", "Container", "Contenitore"),
    ("streams", "Streams", "Flussi"),
    (
        "about_text",
        "FormatCraft converts video, audio, images and PDFs. Free and open source (MIT OR Apache-2.0). Audio and video conversion uses FFmpeg, run as a separate program.",
        "FormatCraft converte video, audio, immagini e PDF. Libero e open source (MIT OR Apache-2.0). La conversione audio e video usa FFmpeg, eseguito come programma separato.",
    ),
    ("tasks", "tasks", "attività"),
    ("convert_to", "Convert to", "Converti in"),
    (
        "no_files",
        "Add at least one file.",
        "Aggiungi almeno un file.",
    ),
    (
        "need_two",
        "Add at least two files.",
        "Aggiungi almeno due file.",
    ),
    (
        "recording_hint",
        "Press Stop in the task list to end the recording.",
        "Premi Ferma nella lista attività per terminare la registrazione.",
    ),
];

pub fn tr(lang: Lang, key: &'static str) -> &'static str {
    STRINGS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, en, it)| match lang {
            Lang::En => *en,
            Lang::It => *it,
        })
        .unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_translated() {
        let mut seen = std::collections::HashSet::new();
        for (k, en, it) in STRINGS {
            assert!(seen.insert(*k), "duplicate key {k}");
            assert!(!en.is_empty() && !it.is_empty(), "{k}");
        }
        assert_eq!(tr(Lang::It, "start"), "Avvia");
        assert_eq!(tr(Lang::En, "missing-key"), "missing-key");
    }
}
