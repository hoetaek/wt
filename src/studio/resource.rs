use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct FileFingerprint {
    pub mtime_ns: Option<String>,
    pub hash: String,
}

#[derive(Debug)]
pub struct ResourceSnapshot {
    pub content: String,
    pub fingerprint: FileFingerprint,
    pub exists: bool,
}

#[derive(Debug)]
pub struct ResourceError {
    pub status: StatusCode,
    pub message: String,
}

impl ResourceError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub struct PreconditionFailed {
    pub current: ResourceSnapshot,
}

pub fn read_fingerprint(path: &Path, label: &str) -> Result<ResourceSnapshot, ResourceError> {
    match fs::read(path) {
        Ok(bytes) => {
            let content = String::from_utf8(bytes).map_err(|err| {
                ResourceError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!("{label} is not UTF-8: {err}"),
                )
            })?;
            let metadata = fs::metadata(path).map_err(|err| {
                ResourceError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to stat {label}: {err}"),
                )
            })?;
            Ok(ResourceSnapshot {
                fingerprint: fingerprint(&content, Some(&metadata)),
                content,
                exists: true,
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ResourceSnapshot {
            content: String::new(),
            fingerprint: empty_fingerprint(),
            exists: false,
        }),
        Err(err) => Err(ResourceError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read {label}: {err}"),
        )),
    }
}

pub fn atomic_write(
    path: &Path,
    contents: &str,
    label: &str,
) -> Result<FileFingerprint, ResourceError> {
    let parent = path.parent().ok_or_else(|| {
        ResourceError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{label} path has no parent directory"),
        )
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        ResourceError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create {label} parent directory: {err}"),
        )
    })?;

    let temp_path = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("resource"),
        temp_suffix()
    ));
    let mut temp_file = fs::File::create(&temp_path).map_err(|err| {
        ResourceError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create temporary {label}: {err}"),
        )
    })?;
    temp_file.write_all(contents.as_bytes()).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        ResourceError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write temporary {label}: {err}"),
        )
    })?;
    temp_file.sync_all().map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        ResourceError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fsync temporary {label}: {err}"),
        )
    })?;
    drop(temp_file);

    fs::rename(&temp_path, path).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        ResourceError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to commit {label}: {err}"),
        )
    })?;
    if let Ok(parent_dir) = fs::File::open(parent) {
        let _ = parent_dir.sync_all();
    }

    read_fingerprint(path, label).map(|snapshot| snapshot.fingerprint)
}

pub fn check_precondition(
    path: &Path,
    expected: &FileFingerprint,
    label: &str,
) -> Result<ResourceSnapshot, ResourceErrorOrPrecondition> {
    let current = read_fingerprint(path, label).map_err(ResourceErrorOrPrecondition::Error)?;
    if &current.fingerprint == expected {
        Ok(current)
    } else {
        Err(ResourceErrorOrPrecondition::Precondition(
            PreconditionFailed { current },
        ))
    }
}

#[derive(Debug)]
pub enum ResourceErrorOrPrecondition {
    Error(ResourceError),
    Precondition(PreconditionFailed),
}

pub fn empty_fingerprint() -> FileFingerprint {
    fingerprint("", None)
}

pub fn diff_text(path: &str, before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }

    let old_lines = split_lines(before);
    let new_lines = split_lines(after);
    let ops = diff_ops(&old_lines, &new_lines);
    let old_start = if old_lines.is_empty() { 0 } else { 1 };
    let new_start = if new_lines.is_empty() { 0 } else { 1 };
    let mut diff = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{old_start},{} +{new_start},{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for op in ops {
        match op {
            DiffOp::Equal(line) => push_diff_line(&mut diff, ' ', line),
            DiffOp::Delete(line) => push_diff_line(&mut diff, '-', line),
            DiffOp::Insert(line) => push_diff_line(&mut diff, '+', line),
        }
    }
    diff
}

fn fingerprint(content: &str, metadata: Option<&fs::Metadata>) -> FileFingerprint {
    FileFingerprint {
        mtime_ns: metadata.and_then(mtime_ns),
        hash: sha256_hex(content.as_bytes()),
    }
}

fn mtime_ns(metadata: &fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let elapsed = modified.duration_since(UNIX_EPOCH).ok()?;
    let nanos = u64::from(elapsed.subsec_nanos());
    elapsed
        .as_secs()
        .checked_mul(1_000_000_000)
        .and_then(|secs| secs.checked_add(nanos))
        .map(|mtime| mtime.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn temp_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{}.{}", std::process::id(), nanos)
}

fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        Vec::new()
    } else {
        content.split_inclusive('\n').collect()
    }
}

#[derive(Clone, Copy, Debug)]
enum DiffOp<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

fn diff_ops<'a>(old_lines: &[&'a str], new_lines: &[&'a str]) -> Vec<DiffOp<'a>> {
    let old_len = old_lines.len();
    let new_len = new_lines.len();
    let mut lcs = vec![0usize; (old_len + 1) * (new_len + 1)];
    for old_idx in (0..old_len).rev() {
        for new_idx in (0..new_len).rev() {
            let idx = lcs_index(new_idx, new_len, old_idx);
            lcs[idx] = if old_lines[old_idx] == new_lines[new_idx] {
                lcs[lcs_index(new_idx + 1, new_len, old_idx + 1)] + 1
            } else {
                lcs[lcs_index(new_idx, new_len, old_idx + 1)]
                    .max(lcs[lcs_index(new_idx + 1, new_len, old_idx)])
            };
        }
    }

    let mut ops = Vec::new();
    let mut old_idx = 0;
    let mut new_idx = 0;
    while old_idx < old_len && new_idx < new_len {
        if old_lines[old_idx] == new_lines[new_idx] {
            ops.push(DiffOp::Equal(old_lines[old_idx]));
            old_idx += 1;
            new_idx += 1;
        } else if lcs[lcs_index(new_idx, new_len, old_idx + 1)]
            >= lcs[lcs_index(new_idx + 1, new_len, old_idx)]
        {
            ops.push(DiffOp::Delete(old_lines[old_idx]));
            old_idx += 1;
        } else {
            ops.push(DiffOp::Insert(new_lines[new_idx]));
            new_idx += 1;
        }
    }
    while old_idx < old_len {
        ops.push(DiffOp::Delete(old_lines[old_idx]));
        old_idx += 1;
    }
    while new_idx < new_len {
        ops.push(DiffOp::Insert(new_lines[new_idx]));
        new_idx += 1;
    }
    ops
}

fn lcs_index(new_idx: usize, new_len: usize, old_idx: usize) -> usize {
    old_idx * (new_len + 1) + new_idx
}

fn push_diff_line(diff: &mut String, prefix: char, line: &str) {
    diff.push(prefix);
    diff.push_str(line);
    if !line.ends_with('\n') {
        diff.push('\n');
    }
}
