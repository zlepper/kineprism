use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use better_image_diff_core::RenderedArtifacts;
use image::{ExtendedColorType, ImageFormat};

use crate::error::CliError;

static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(0);
const ARTIFACT_COUNT: usize = 4;

pub(crate) struct ArtifactPaths {
    pub(crate) expected: PathBuf,
    pub(crate) actual: PathBuf,
    pub(crate) diff: PathBuf,
    pub(crate) report: PathBuf,
    directory: PathBuf,
}

impl ArtifactPaths {
    pub(crate) fn new(directory: &Path) -> Self {
        Self {
            expected: directory.join("expected.png"),
            actual: directory.join("actual.png"),
            diff: directory.join("diff.png"),
            report: directory.join("report.json"),
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

    pub(crate) fn write(
        &self,
        rendered: &RenderedArtifacts,
        report: &[u8],
        force: bool,
    ) -> Result<(), CliError> {
        fs::create_dir_all(&self.directory)
            .map_err(|source| Self::io_error("create output directory", &self.directory, source))?;
        let transaction_directory =
            reserve_transaction_directory(&self.directory, std::process::id(), || {
                NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed)
            })
            .map_err(|source| {
                Self::io_error("reserve artifact transaction", &self.directory, source)
            })?;
        let temporary = temporary_paths(&transaction_directory);
        let backups = backup_paths(&transaction_directory);
        let buffers = [&rendered.expected, &rendered.actual, &rendered.diff];

        for (path, image) in temporary[..3].iter().zip(buffers) {
            if let Err(source) = image::save_buffer_with_format(
                path,
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
                ImageFormat::Png,
            ) {
                cleanup(&temporary);
                cleanup_transaction_directory(&transaction_directory);
                return Err(CliError::Encode {
                    path: path.clone(),
                    source,
                });
            }
        }
        if let Err(source) = fs::write(&temporary[3], report) {
            cleanup(&temporary);
            cleanup_transaction_directory(&transaction_directory);
            return Err(Self::io_error("write JSON report", &self.report, source));
        }

        let targets = self.targets();
        let mut backed_up = [false; ARTIFACT_COUNT];
        if let Err(error) = prepare_backups(targets, &backups, force, &mut backed_up) {
            restore_backups(targets, &backups, backed_up);
            cleanup(&temporary);
            cleanup_transaction_directory(&transaction_directory);
            return Err(error);
        }
        let mut committed = [false; ARTIFACT_COUNT];
        for index in 0..temporary.len() {
            if let Err(source) = fs::rename(&temporary[index], targets[index]) {
                rollback_committed(targets, committed);
                restore_backups(targets, &backups, backed_up);
                cleanup(&temporary);
                cleanup_transaction_directory(&transaction_directory);
                return Err(Self::io_error("commit artifact", targets[index], source));
            }
            committed[index] = true;
        }
        cleanup(&backups);
        cleanup_transaction_directory(&transaction_directory);
        Ok(())
    }

    fn targets(&self) -> [&Path; ARTIFACT_COUNT] {
        [&self.expected, &self.actual, &self.diff, &self.report]
    }

    fn io_error(action: &'static str, path: &Path, source: io::Error) -> CliError {
        CliError::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }
}

fn reserve_transaction_directory(
    output_directory: &Path,
    process: u32,
    mut next_sequence: impl FnMut() -> u64,
) -> io::Result<PathBuf> {
    const MAX_ATTEMPTS: usize = 128;
    for _ in 0..MAX_ATTEMPTS {
        let candidate =
            output_directory.join(format!(".better-image-diff.{process}.{}", next_sequence()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a unique artifact transaction directory",
    ))
}

fn temporary_paths(transaction_directory: &Path) -> [PathBuf; ARTIFACT_COUNT] {
    [
        transaction_directory.join("expected.tmp"),
        transaction_directory.join("actual.tmp"),
        transaction_directory.join("diff.tmp"),
        transaction_directory.join("report.tmp"),
    ]
}

fn backup_paths(transaction_directory: &Path) -> [PathBuf; ARTIFACT_COUNT] {
    [
        transaction_directory.join("expected.bak"),
        transaction_directory.join("actual.bak"),
        transaction_directory.join("diff.bak"),
        transaction_directory.join("report.bak"),
    ]
}

fn prepare_backups(
    targets: [&Path; ARTIFACT_COUNT],
    backups: &[PathBuf; ARTIFACT_COUNT],
    force: bool,
    backed_up: &mut [bool; ARTIFACT_COUNT],
) -> Result<(), CliError> {
    for index in 0..targets.len() {
        if !targets[index].exists() {
            continue;
        }
        if !force {
            return Err(CliError::ArtifactExists(targets[index].to_owned()));
        }
        let metadata = fs::symlink_metadata(targets[index]).map_err(|source| {
            ArtifactPaths::io_error("inspect artifact", targets[index], source)
        })?;
        if metadata.file_type().is_dir() {
            return Err(ArtifactPaths::io_error(
                "replace artifact",
                targets[index],
                io::Error::new(io::ErrorKind::IsADirectory, "artifact path is a directory"),
            ));
        }
        fs::rename(targets[index], &backups[index]).map_err(|source| {
            ArtifactPaths::io_error("prepare artifact replacement", targets[index], source)
        })?;
        backed_up[index] = true;
    }
    Ok(())
}

fn rollback_committed(targets: [&Path; ARTIFACT_COUNT], committed: [bool; ARTIFACT_COUNT]) {
    for (target, was_committed) in targets.into_iter().zip(committed) {
        if was_committed {
            let _result = fs::remove_file(target);
        }
    }
}

fn restore_backups(
    targets: [&Path; ARTIFACT_COUNT],
    backups: &[PathBuf; ARTIFACT_COUNT],
    backed_up: [bool; ARTIFACT_COUNT],
) {
    for index in 0..targets.len() {
        if backed_up[index] {
            let _result = fs::rename(&backups[index], targets[index]);
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

fn cleanup_transaction_directory(path: &Path) {
    let _result = fs::remove_dir(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_reservation_preserves_colliding_directory() {
        let root = std::env::temp_dir().join(format!(
            "better-image-diff-reservation-{}-{}",
            std::process::id(),
            NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("create test root");
        let process = 42;
        let collision = root.join(format!(".better-image-diff.{process}.7"));
        fs::create_dir(&collision).expect("create collision");
        let sentinel = collision.join("sentinel");
        fs::write(&sentinel, b"unrelated").expect("write sentinel");
        let mut sequence = 7;

        let reserved = reserve_transaction_directory(&root, process, || {
            let current = sequence;
            sequence += 1;
            current
        })
        .expect("reserve after collision");

        assert_eq!(reserved, root.join(".better-image-diff.42.8"));
        assert_eq!(fs::read(&sentinel).expect("read sentinel"), b"unrelated");
        fs::remove_dir(reserved).expect("remove reservation");
        fs::remove_file(sentinel).expect("remove sentinel");
        fs::remove_dir(collision).expect("remove collision");
        fs::remove_dir(root).expect("remove test root");
    }
}
