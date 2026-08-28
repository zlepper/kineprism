use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use better_image_diff_core::RenderedArtifacts;
use image::{ExtendedColorType, ImageFormat};

use crate::error::CliError;

pub(crate) struct ArtifactPaths {
    pub(crate) expected: PathBuf,
    pub(crate) actual: PathBuf,
    pub(crate) diff: PathBuf,
    directory: PathBuf,
}

impl ArtifactPaths {
    pub(crate) fn new(directory: &Path) -> Self {
        Self {
            expected: directory.join("expected.png"),
            actual: directory.join("actual.png"),
            diff: directory.join("diff.png"),
            directory: directory.to_owned(),
        }
    }

    pub(crate) fn preflight(
        &self,
        expected_input: &Path,
        actual_input: &Path,
        force: bool,
    ) -> Result<(), CliError> {
        for target in self.targets() {
            if paths_refer_to_same_file(target, expected_input)?
                || paths_refer_to_same_file(target, actual_input)?
            {
                return Err(CliError::ArtifactOverwritesInput(target.to_owned()));
            }
            if target.exists() && !force {
                return Err(CliError::ArtifactExists(target.to_owned()));
            }
        }
        Ok(())
    }

    pub(crate) fn write(&self, rendered: &RenderedArtifacts, force: bool) -> Result<(), CliError> {
        fs::create_dir_all(&self.directory)
            .map_err(|source| Self::io_error("create output directory", &self.directory, source))?;
        let temporary = self.temporary_paths();
        let buffers = [&rendered.expected, &rendered.actual, &rendered.diff];

        for (path, image) in temporary.iter().zip(buffers) {
            if let Err(source) = image::save_buffer_with_format(
                path,
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
                ImageFormat::Png,
            ) {
                cleanup(&temporary);
                return Err(CliError::Encode {
                    path: path.clone(),
                    source,
                });
            }
        }

        for (temporary_path, target) in temporary.iter().zip(self.targets()) {
            if force && target.exists() {
                if let Err(source) = fs::remove_file(target) {
                    cleanup(&temporary);
                    return Err(Self::io_error("replace artifact", target, source));
                }
            }
            if let Err(source) = fs::rename(temporary_path, target) {
                cleanup(&temporary);
                return Err(Self::io_error("commit artifact", target, source));
            }
        }
        Ok(())
    }

    fn targets(&self) -> [&Path; 3] {
        [&self.expected, &self.actual, &self.diff]
    }

    fn temporary_paths(&self) -> [PathBuf; 3] {
        let process = std::process::id();
        [
            self.directory.join(format!(".expected.{process}.tmp")),
            self.directory.join(format!(".actual.{process}.tmp")),
            self.directory.join(format!(".diff.{process}.tmp")),
        ]
    }

    fn io_error(action: &'static str, path: &Path, source: io::Error) -> CliError {
        CliError::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> Result<bool, CliError> {
    if left == right {
        return Ok(true);
    }
    if !left.exists() || !right.exists() {
        return Ok(false);
    }

    let canonical_left = fs::canonicalize(left)
        .map_err(|source| ArtifactPaths::io_error("resolve path", left, source))?;
    let canonical_right = fs::canonicalize(right)
        .map_err(|source| ArtifactPaths::io_error("resolve path", right, source))?;
    if canonical_left == canonical_right {
        return Ok(true);
    }

    same_file_identity(left, right)
}

#[cfg(unix)]
fn same_file_identity(left: &Path, right: &Path) -> Result<bool, CliError> {
    use std::os::unix::fs::MetadataExt;

    let left_metadata = fs::metadata(left)
        .map_err(|source| ArtifactPaths::io_error("inspect path", left, source))?;
    let right_metadata = fs::metadata(right)
        .map_err(|source| ArtifactPaths::io_error("inspect path", right, source))?;
    Ok(left_metadata.dev() == right_metadata.dev() && left_metadata.ino() == right_metadata.ino())
}

#[cfg(not(unix))]
fn same_file_identity(_left: &Path, _right: &Path) -> Result<bool, CliError> {
    Ok(false)
}

fn cleanup(paths: &[PathBuf]) {
    for path in paths {
        let _result = fs::remove_file(path);
    }
}
