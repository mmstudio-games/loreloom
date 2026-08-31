use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid command line: {0}")]
    Arguments(&'static str),
    #[error("application runtime could not be created")]
    Tokio(#[source] std::io::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Content(#[from] loreloom_content::ContentError),
    #[error(transparent)]
    Package(#[from] loreloom_content::PackageError),
    #[error("built-in demo content could not be encoded")]
    DemoCodec(#[source] serde_json::Error),
    #[error(transparent)]
    Store(#[from] loreloom_store::StoreError),
    #[error(transparent)]
    Runtime(#[from] loreloom_runtime::RuntimeError),
    #[error(transparent)]
    Tui(#[from] loreloom_tui::TuiError),
    #[error(transparent)]
    Identity(#[from] loreloom_core::IdentityError),
    #[error(transparent)]
    Text(#[from] loreloom_core::TextError),
    #[error(transparent)]
    Fixed(#[from] loreloom_core::FixedError),
}
