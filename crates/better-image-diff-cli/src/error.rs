use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;

use better_image_diff_core::{CompareError, RenderError};

#[derive(Debug)]
pub(crate) enum CliError {
    MissingArgument(&'static str),
    Compare(CompareError),
    Render(RenderError),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
    Encode {
        path: PathBuf,
        source: image::ImageError,
    },
    NotPng(PathBuf),
    ArtifactExists(PathBuf),
    ArtifactOverwritesInput(PathBuf),
    Json(serde_json::Error),
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArgument(argument) => write!(formatter, "missing required {argument}"),
            Self::Compare(error) => Display::fmt(error, formatter),
            Self::Render(error) => Display::fmt(error, formatter),
            Self::Io {
                action,
                path,
                source,
            } => write!(
                formatter,
                "failed to {action} '{}': {source}",
                path.display()
            ),
            Self::Decode { path, source } => {
                write!(
                    formatter,
                    "failed to decode PNG '{}': {source}",
                    path.display()
                )
            }
            Self::Encode { path, source } => {
                write!(
                    formatter,
                    "failed to encode PNG '{}': {source}",
                    path.display()
                )
            }
            Self::NotPng(path) => write!(formatter, "input '{}' is not a PNG", path.display()),
            Self::ArtifactExists(path) => write!(
                formatter,
                "artifact '{}' already exists; pass --force to replace it",
                path.display()
            ),
            Self::ArtifactOverwritesInput(path) => write!(
                formatter,
                "artifact '{}' would overwrite an input image",
                path.display()
            ),
            Self::Json(error) => write!(formatter, "failed to serialize JSON report: {error}"),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Compare(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Decode { source, .. } | Self::Encode { source, .. } => Some(source),
            Self::Json(error) => Some(error),
            Self::MissingArgument(_)
            | Self::NotPng(_)
            | Self::ArtifactExists(_)
            | Self::ArtifactOverwritesInput(_) => None,
        }
    }
}

impl From<CompareError> for CliError {
    fn from(value: CompareError) -> Self {
        Self::Compare(value)
    }
}

impl From<RenderError> for CliError {
    fn from(value: RenderError) -> Self {
        Self::Render(value)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<io::Error> for CliError {
    fn from(source: io::Error) -> Self {
        Self::Io {
            action: "write standard output",
            path: PathBuf::from("<stdout>"),
            source,
        }
    }
}
