//! Steps shared by HTTP and FTP downloads: choosing the destination, creating the part file,
//! and finalizing (fsync, size check, checksum, atomic rename).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::checksum;
use crate::error::EngineError;
use crate::filename::{self, numbered_name, part_path_for};
use crate::job::{JobShared, StopReason};
use crate::progress::{Completed, DownloadEvent, DownloadState};
use crate::request::{ConflictPolicy, Destination, DownloadRequest, ExpectedChecksum};
use crate::resume::ResumeState;
use crate::segmented::supervisor::Outcome;
use crate::segmented::writer::Writer;
use crate::storage;

/// The chosen paths and the created part file.
pub(crate) struct Prepared {
    pub final_path: PathBuf,
    pub part_path: PathBuf,
    pub file: File,
}

/// Picks the final path for `name` according to the conflict policy, checks free space and
/// creates (and pre-allocates) the part file. With `fixed` the paths of a previous run are
/// reused (restart of a non-resumable download).
pub(crate) fn prepare_destination(
    dest: &Destination,
    suggested_name: &str,
    suffix: &str,
    size: Option<u64>,
    fixed: Option<&Path>,
) -> Result<Prepared, EngineError> {
    let io_err = |e: std::io::Error| EngineError::from_io(&e, size.unwrap_or(0));
    if let Some(final_path) = fixed {
        let dir = final_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        std::fs::create_dir_all(&dir).map_err(io_err)?;
        storage::check_free_space(&dir, size.unwrap_or(0))?;
        let part_path = part_path_for(final_path, suffix);
        let file = storage::create_part_file(&part_path, size, false).map_err(io_err)?;
        return Ok(Prepared {
            final_path: final_path.to_path_buf(),
            part_path,
            file,
        });
    }
    std::fs::create_dir_all(&dest.dir).map_err(io_err)?;
    storage::check_free_space(&dest.dir, size.unwrap_or(0))?;
    let name = match &dest.file_name {
        Some(n) if !n.trim().is_empty() => filename::sanitize(n),
        _ => suggested_name.to_string(),
    };
    match dest.conflict {
        ConflictPolicy::Overwrite => {
            let final_path = dest.dir.join(&name);
            let part_path = part_path_for(&final_path, suffix);
            let file = storage::create_part_file(&part_path, size, false).map_err(io_err)?;
            Ok(Prepared {
                final_path,
                part_path,
                file,
            })
        }
        ConflictPolicy::Fail => {
            let final_path = dest.dir.join(&name);
            let part_path = part_path_for(&final_path, suffix);
            if final_path.exists() {
                return Err(EngineError::FileExists { path: final_path });
            }
            let file = storage::create_part_file(&part_path, size, true).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    EngineError::FileExists {
                        path: part_path.clone(),
                    }
                } else {
                    io_err(e)
                }
            })?;
            Ok(Prepared {
                final_path,
                part_path,
                file,
            })
        }
        ConflictPolicy::Rename => {
            // `create_new` on the part file claims the name atomically even when several
            // downloads pick the same name at the same moment.
            for _ in 0..64 {
                let final_path = filename::unique_path(&dest.dir, &name, suffix, false)
                    .ok_or_else(|| EngineError::FileExists {
                        path: dest.dir.join(&name),
                    })?;
                let part_path = part_path_for(&final_path, suffix);
                match storage::create_part_file(&part_path, size, true) {
                    Ok(file) => {
                        return Ok(Prepared {
                            final_path,
                            part_path,
                            file,
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(io_err(e)),
                }
            }
            Err(EngineError::FileExists {
                path: dest.dir.join(&name),
            })
        }
    }
}

/// Moves the finished part file into place according to the conflict policy. Returns the path
/// actually used.
fn place_file(
    part: &Path,
    final_path: &Path,
    conflict: ConflictPolicy,
) -> Result<PathBuf, EngineError> {
    let io = |e: std::io::Error| EngineError::Io {
        message: format!("cannot rename {}: {e}", part.display()),
    };
    match conflict {
        ConflictPolicy::Overwrite => {
            storage::rename_replace(part, final_path).map_err(io)?;
            Ok(final_path.to_path_buf())
        }
        ConflictPolicy::Fail => {
            if storage::rename_no_replace(part, final_path).map_err(io)? {
                Ok(final_path.to_path_buf())
            } else {
                Err(EngineError::FileExists {
                    path: final_path.to_path_buf(),
                })
            }
        }
        ConflictPolicy::Rename => {
            let dir = final_path.parent().unwrap_or(Path::new(""));
            let name = final_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            for n in 0..10_000u32 {
                let candidate = if n == 0 {
                    final_path.to_path_buf()
                } else {
                    dir.join(numbered_name(&name, n))
                };
                if storage::rename_no_replace(part, &candidate).map_err(io)? {
                    return Ok(candidate);
                }
            }
            Err(EngineError::FileExists {
                path: final_path.to_path_buf(),
            })
        }
    }
}

/// Finishes a segmented download: closes the writer (flush + fsync), checks the size,
/// verifies the checksum and renames the part file.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize(
    job: &Arc<JobShared>,
    writer: &Writer,
    part_path: &Path,
    final_path: &Path,
    expected_size: Option<u64>,
    conflict: ConflictPolicy,
    checksum: Option<&ExpectedChecksum>,
    transferred: u64,
    run_started: Instant,
) -> Result<Completed, EngineError> {
    job.set_state(DownloadState::Finalizing);
    writer.close().await?;
    check_cancelled(job, part_path)?;
    let size = std::fs::metadata(part_path)
        .map_err(|e| EngineError::from_io(&e, 0))?
        .len();
    if let Some(expected) = expected_size
        && size != expected
    {
        return Err(EngineError::internal(format!(
            "file has {size} bytes, expected {expected}"
        )));
    }
    let actual = match checksum {
        Some(c) => {
            let (p, algo) = (part_path.to_path_buf(), c.algo);
            let hashing_job = job.clone();
            let digest = tokio::task::spawn_blocking(move || {
                checksum::hash_file(&p, algo, || {
                    matches!(hashing_job.stop_reason(), Some(StopReason::Cancel { .. }))
                })
            })
            .await
            .map_err(EngineError::internal)?;
            check_cancelled(job, part_path)?;
            let digest = digest.map_err(|e| EngineError::from_io(&e, 0))?;
            Some((checksum::normalize_hex(&c.hex), digest))
        }
        None => None,
    };
    if let Some((expected, actual)) = actual
        && expected != actual
    {
        return Err(EngineError::ChecksumMismatch {
            expected,
            actual,
            path: part_path.to_path_buf(),
        });
    }
    // Publishing is the commit point: neither unverified nor cancelled bytes may replace
    // an existing destination. Keep checksum failures under the partial-file name.
    check_cancelled(job, part_path)?;
    let path = place_file(part_path, final_path, conflict)?;
    {
        let mut m = job.meta();
        m.final_path = Some(path.clone());
        m.total = Some(size);
    }
    Ok(Completed {
        path,
        size,
        checksum_ok: checksum.map(|_| true),
        elapsed: run_started.elapsed(),
        transferred,
        extra_files: Vec::new(),
        warning: None,
    })
}

fn check_cancelled(job: &JobShared, part_path: &Path) -> Result<(), EngineError> {
    if let Some(StopReason::Cancel { delete_file }) = job.stop_reason() {
        if delete_file {
            let _ = std::fs::remove_file(part_path);
        }
        return Err(EngineError::Cancelled);
    }
    Ok(())
}

/// Paths of the part and final file.
pub(crate) struct Paths {
    pub part: std::path::PathBuf,
    pub final_path: std::path::PathBuf,
}

/// Turns the supervisor outcome into the job result (shared with FTP).
pub(crate) async fn end_job(
    job: &Arc<JobShared>,
    writer: &Arc<Writer>,
    paths: &Paths,
    outcome: Result<Outcome, EngineError>,
    req: &DownloadRequest,
    transferred: u64,
    run_started: Instant,
) -> (Result<Completed, EngineError>, Option<ResumeState>) {
    let total = job.meta().total;
    // The supervisor captured its outcome before the writer drains. A newer cancel must
    // override that snapshot, including a pause already in flight.
    let outcome = job
        .stop_reason()
        .map_or(outcome, |reason| Ok(Outcome::Stopped(reason)));
    match outcome {
        Ok(Outcome::Completed) => {
            let r = finalize(
                job,
                writer,
                &paths.part,
                &paths.final_path,
                total,
                req.destination.conflict,
                req.checksum.as_ref(),
                transferred,
                run_started,
            )
            .await;
            let state = if matches!(&r, Err(EngineError::Cancelled))
                && job.stop_reason() == Some(StopReason::Cancel { delete_file: false })
            {
                job.resume_state()
            } else {
                None
            };
            (r, state)
        }
        Ok(Outcome::Stopped(reason)) => {
            if reason == (StopReason::Cancel { delete_file: true }) {
                writer.discard().await;
                let _ = std::fs::remove_file(&paths.part);
                return (Err(EngineError::Cancelled), None);
            }
            let state = writer
                .flush(true)
                .await
                .ok()
                .and_then(|segs| job.build_resume(segs));
            let closed = writer.close().await;
            // Cancellation can also arrive while flush/fsync is running.
            match job.stop_reason().unwrap_or(reason) {
                StopReason::Cancel { delete_file: true } => {
                    let _ = std::fs::remove_file(&paths.part);
                    (Err(EngineError::Cancelled), None)
                }
                StopReason::Cancel { delete_file: false } => (Err(EngineError::Cancelled), state),
                StopReason::Pause => match closed {
                    Ok(()) => (Err(EngineError::Paused), state),
                    Err(e) => (Err(e), state),
                },
            }
        }
        Err(e) => {
            if e == EngineError::LinkExpired {
                job.emit(DownloadEvent::LinkExpired);
            }
            // Keep what we have so the download can be resumed (e.g. after refreshing the link).
            let state = writer
                .flush(true)
                .await
                .ok()
                .and_then(|segs| job.build_resume(segs));
            let _ = writer.close().await;
            if let Err(cancelled) = check_cancelled(job, &paths.part) {
                let state = if job.stop_reason() == Some(StopReason::Cancel { delete_file: true }) {
                    None
                } else {
                    state
                };
                return (Err(cancelled), state);
            }
            (Err(e), state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn audit_cancel_wins_over_an_already_captured_pause_outcome() {
        use crate::segmented::table::SegmentTable;
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("test.bin.prismpart");
        let paths = Paths {
            part: part.clone(),
            final_path: dir.path().join("test.bin"),
        };
        let file = storage::create_part_file(&part, Some(1024), true).unwrap();
        let table = Arc::new(SegmentTable::new(Some(1024), 1024, true));
        let budget = Arc::new(tokio::sync::Semaphore::new(1 << 20));
        let writer = Writer::spawn(file, table, budget, 1 << 20, 64 << 10).unwrap();
        let req = DownloadRequest::new(
            url::Url::parse("http://example.test/file").unwrap(),
            dir.path(),
        );
        let (job, _) = JobShared::new(req.url.clone(), crate::resume::ResumeKind::Http, 0);
        job.request_stop(StopReason::Pause);
        let outcome = Ok(Outcome::Stopped(StopReason::Pause));
        job.request_stop(StopReason::Cancel { delete_file: true });
        let (result, state) =
            end_job(&job, &writer, &paths, outcome, &req, 0, Instant::now()).await;
        assert_eq!(result.unwrap_err(), EngineError::Cancelled);
        assert!(state.is_none());
        assert!(
            !part.exists(),
            "cancelling after pause must delete the partial file"
        );
    }

    #[tokio::test]
    async fn cancellation_before_publication_preserves_the_existing_destination() {
        use crate::segmented::table::SegmentTable;
        for delete_file in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let part = dir.path().join("file.bin.prismpart");
            let target = dir.path().join("file.bin");
            std::fs::write(&target, b"previous file").unwrap();
            let file = storage::create_part_file(&part, Some(1024), true).unwrap();
            let table = Arc::new(SegmentTable::new(Some(1024), 1024, true));
            let writer = Writer::spawn(
                file,
                table,
                Arc::new(tokio::sync::Semaphore::new(1 << 20)),
                1 << 20,
                64 << 10,
            )
            .unwrap();
            let (job, _) = JobShared::new(
                url::Url::parse("http://example.test/file").unwrap(),
                crate::resume::ResumeKind::Http,
                0,
            );
            job.request_stop(StopReason::Cancel { delete_file });
            let result = finalize(
                &job,
                &writer,
                &part,
                &target,
                Some(1024),
                ConflictPolicy::Overwrite,
                None,
                0,
                Instant::now(),
            )
            .await;
            assert_eq!(result.unwrap_err(), EngineError::Cancelled);
            assert_eq!(std::fs::read(&target).unwrap(), b"previous file");
            assert_eq!(part.exists(), !delete_file);
        }
    }

    #[test]
    fn destination_policies() {
        let dir = tempfile::tempdir().unwrap();
        let dest = |conflict| Destination {
            dir: dir.path().to_path_buf(),
            file_name: None,
            conflict,
        };
        let p = prepare_destination(
            &dest(ConflictPolicy::Rename),
            "a.bin",
            ".prismpart",
            Some(10),
            None,
        )
        .unwrap();
        assert_eq!(p.final_path, dir.path().join("a.bin"));
        assert_eq!(p.file.metadata().unwrap().len(), 10);
        // The part file now claims the name.
        let p2 = prepare_destination(
            &dest(ConflictPolicy::Rename),
            "a.bin",
            ".prismpart",
            None,
            None,
        )
        .unwrap();
        assert_eq!(p2.final_path, dir.path().join("a (1).bin"));
        drop((p.file, p2.file));
        std::fs::write(dir.path().join("b.bin"), b"x").unwrap();
        assert!(matches!(
            prepare_destination(
                &dest(ConflictPolicy::Fail),
                "b.bin",
                ".prismpart",
                None,
                None
            ),
            Err(EngineError::FileExists { .. })
        ));
        let o = prepare_destination(
            &dest(ConflictPolicy::Overwrite),
            "b.bin",
            ".prismpart",
            None,
            None,
        )
        .unwrap();
        drop(o.file);
        assert_eq!(
            place_file(&o.part_path, &o.final_path, ConflictPolicy::Overwrite).unwrap(),
            o.final_path
        );
        // Rename at completion when the final name appeared meanwhile.
        let r = prepare_destination(
            &dest(ConflictPolicy::Rename),
            "c.bin",
            ".prismpart",
            None,
            None,
        )
        .unwrap();
        drop(r.file);
        std::fs::write(dir.path().join("c.bin"), b"late").unwrap();
        assert_eq!(
            place_file(&r.part_path, &r.final_path, ConflictPolicy::Rename).unwrap(),
            dir.path().join("c (1).bin")
        );
        // Explicit file names are sanitized.
        let mut d = dest(ConflictPolicy::Rename);
        d.file_name = Some("x:y?.txt".into());
        let s = prepare_destination(&d, "ignored", ".prismpart", None, None).unwrap();
        assert_eq!(s.final_path, dir.path().join("x_y_.txt"));
    }
}
