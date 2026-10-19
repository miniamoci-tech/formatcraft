use std::path::PathBuf;

/// Every failure the engine can report. Each variant says what went wrong in
/// terms a user (or an agent) can act on.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("FFmpeg was not found. Install it or set FORMATCRAFT_FFMPEG to the ffmpeg executable")]
    FfmpegMissing,
    #[error("{tool} failed (exit code {code:?}): {message}")]
    ToolFailed {
        tool: &'static str,
        code: Option<i32>,
        message: String,
    },
    #[error("the job was cancelled")]
    Cancelled,
    #[error("unknown output format `{0}`; run `formatcraft-cli formats` for the list")]
    UnknownFormat(String),
    #[error("format `{format}` cannot be used for {what}")]
    WrongCategory { format: String, what: &'static str },
    #[error("invalid option: {0}")]
    InvalidOption(String),
    #[error("input file does not exist: {0}")]
    MissingInput(PathBuf),
    #[error("{what} needs at least {min} input files, got {got}")]
    TooFewInputs {
        what: &'static str,
        min: usize,
        got: usize,
    },
    #[error("could not read media information: {0}")]
    Probe(String),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("PDF error: {0}")]
    Pdf(String),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("I/O error: {0}")]
    IoBare(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<lopdf::Error> for Error {
    fn from(e: lopdf::Error) -> Self {
        Error::Pdf(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io_at(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.into();
    move |source| Error::Io { path, source }
}
