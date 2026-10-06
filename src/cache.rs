//! Strict CDVN validation. Payload bytes are copied, never recompiled or modified.
use crate::{Result, atomic_json, fail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{FileExt, MetadataExt},
    path::{Path, PathBuf},
};

const WRAP: u64 = 1 << 32;
pub const SHARD_LIMIT: u64 = 1 << 31;
const MAGIC: [u8; 4] = [0x9d, 0xa1, 0x46, 0x98];

#[derive(Debug)]
pub struct Record {
    pub offset: u64,
    pub key: [u8; 16],
    pub size: u32,
    pub wrapped: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub file: String,
    pub records: usize,
    pub wrapped_offsets: usize,
    pub bin_bytes: u64,
    pub toc_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Stamp(u64, u64, u64, i64, i64, i64, i64);
fn stamp(path: &Path) -> Result<Stamp> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() {
        return fail(
            "A shader file is a link or is not a normal file. Use a copy of the actual shader files instead.",
        );
    }
    Ok(Stamp(
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    ))
}
fn u32le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("four-byte field"))
}

pub fn read_records(toc_path: &Path) -> Result<(Vec<u8>, Vec<Record>, u64)> {
    let bin_path = toc_path.with_extension("bin");
    let before = (stamp(toc_path)?, stamp(&bin_path)?);
    if before.0.2 > 512 * 1024 * 1024 {
        return fail(
            "A shader index file is larger than this version can safely check (512 MiB). The original file was kept.",
        );
    }
    let toc = fs::read(toc_path)?;
    if toc.len() < 32 || &toc[..4] != b"CDVN" || (toc.len() - 32) % 24 != 0 {
        return fail(
            "A shader index file is incomplete or uses a format this version cannot read. The original file was kept.",
        );
    }
    let bin = File::open(&bin_path)?;
    let length = bin.metadata()?.len();
    let mut header = [0u8; 32];
    bin.read_exact_at(&mut header, 0)?;
    if header != toc[..32] {
        return fail(
            "A shader data file and its index do not match. Use a complete copy of the same saved shader folder.",
        );
    }
    let mut rows = Vec::with_capacity((toc.len() - 32) / 24);
    for entry in toc[32..].chunks_exact(24) {
        let key: [u8; 16] = entry[..16].try_into()?;
        let offset = u64::from(u32le(&entry[16..20]));
        let size = u32le(&entry[20..24]);
        let mut candidate = offset;
        let mut found = None;
        while candidate < length {
            let end = candidate
                .checked_add(32)
                .and_then(|v| v.checked_add(u64::from(size)));
            if candidate >= 32 && size >= 4 && end.is_some_and(|e| e <= length) {
                bin.read_exact_at(&mut header, candidate)?;
                if header[..4] == MAGIC && header[4..20] == key && u32le(&header[28..32]) == size {
                    if found.is_some() {
                        return fail(
                            "More than one saved entry matches the same shader location. Copying stopped rather than guessing.",
                        );
                    }
                    found = Some(candidate);
                }
            }
            let Some(next) = candidate.checked_add(WRAP) else {
                break;
            };
            candidate = next;
        }
        let actual = found.ok_or("A listed shader entry could not be found in the data file. It cannot be copied safely.")?;
        rows.push(Record {
            offset: actual,
            key,
            size,
            wrapped: actual != offset,
        });
    }
    rows.sort_by_key(|r| r.offset);
    let mut cursor = 32u64;
    for row in &rows {
        if row.offset != cursor {
            return fail(
                "Some saved shader entries are missing, duplicated or overlap. These files cannot be copied safely.",
            );
        }
        cursor = row
            .offset
            .checked_add(32 + u64::from(row.size))
            .ok_or("A shader entry is too large for this version to check safely.")?;
    }
    if cursor != length {
        return fail(
            "The shader data file contains entries missing from its index. These files cannot be copied safely.",
        );
    }
    if before != (stamp(toc_path)?, stamp(&bin_path)?) {
        return fail(
            "Shader files changed during the check. Fully exit Steam and any running games, then try again.",
        );
    }
    Ok((toc, rows, length))
}

fn walk(
    path: &Path,
    depth: usize,
    toc: &mut BTreeSet<PathBuf>,
    bins: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if depth > 12 {
        return fail(
            "The shader folder has too many nested folders. Choose the game's NVIDIA shader folder, not the entire Steam library.",
        );
    }
    for e in fs::read_dir(path)? {
        let e = e?;
        let p = e.path();
        let kind = e.file_type()?;
        if kind.is_symlink() {
            return fail(
                "The shader folder contains a symbolic link. Use a copy of the actual files instead.",
            );
        }
        if kind.is_dir() {
            walk(&p, depth + 1, toc, bins)?;
        } else if kind.is_file() {
            match p.extension().and_then(|v| v.to_str()) {
                Some("toc") => {
                    toc.insert(p);
                }
                Some("bin") => {
                    bins.insert(p);
                }
                _ => (),
            }
        }
    }
    Ok(())
}

pub fn pairs(source: &Path) -> Result<Vec<PathBuf>> {
    let (mut toc, mut bins) = (BTreeSet::new(), BTreeSet::new());
    walk(source, 0, &mut toc, &mut bins)?;
    if toc.is_empty() {
        return fail(
            "No saved NVIDIA shader files were found. You can skip copying and set up the game with enable instead.",
        );
    }
    if toc
        .iter()
        .map(|p| p.with_extension("bin"))
        .collect::<BTreeSet<_>>()
        != bins
    {
        return fail(
            "A shader file is missing its matching data or index file. Use a complete copy of the saved shader folder.",
        );
    }
    Ok(toc.into_iter().collect())
}

pub fn scan(source: &Path) -> Result<Vec<Report>> {
    let mut out = Vec::new();
    let files = pairs(source)?;
    for (i, path) in files.iter().enumerate() {
        crate::progress(format_args!(
            "Checking saved shader file {}/{}: {}",
            i + 1,
            files.len(),
            path.strip_prefix(source)?.display()
        ));
        let (toc, rows, length) = read_records(path)?;
        out.push(Report {
            file: path.strip_prefix(source)?.to_string_lossy().into(),
            records: rows.len(),
            wrapped_offsets: rows.iter().filter(|r| r.wrapped).count(),
            bin_bytes: length,
            toc_bytes: toc.len() as u64,
        });
    }
    Ok(out)
}

pub fn digest_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = f.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn write_shard(input: &File, header: &[u8], rows: &[Record], target: &Path) -> Result<()> {
    let bin_path = target.with_extension("bin");
    let toc_path = target.with_extension("toc");
    let mut output = File::create_new(&bin_path)?;
    output.write_all(header)?;
    let mut hash = Sha256::new();
    hash.update(header);
    let mut toc = header.to_vec();
    let mut destination_offset = 32u64;
    let mut buffer = vec![0u8; 1024 * 1024];
    for row in rows {
        let mut offset = row.offset;
        let end = row.offset + 32 + u64::from(row.size);
        while offset < end {
            let n = usize::try_from((end - offset).min(buffer.len() as u64))?;
            input.read_exact_at(&mut buffer[..n], offset)?;
            output.write_all(&buffer[..n])?;
            hash.update(&buffer[..n]);
            offset += n as u64;
        }
        toc.extend_from_slice(&row.key);
        toc.extend_from_slice(&u32::try_from(destination_offset)?.to_le_bytes());
        toc.extend_from_slice(&row.size.to_le_bytes());
        destination_offset += 32 + u64::from(row.size);
    }
    output.sync_all()?;
    let mut index = File::create_new(&toc_path)?;
    index.write_all(&toc)?;
    index.sync_all()?;
    crate::progress(format_args!(
        "Checking the copied file and its shader locations: {}",
        toc_path.display()
    ));
    if digest_file(&bin_path)? != format!("{:x}", hash.finalize()) {
        return fail(
            "The copied shader file does not match the original. The incomplete copy was not saved.",
        );
    }
    let (_, checked, _) = read_records(&toc_path)?;
    if checked.len() != rows.len() || checked.iter().any(|r| r.wrapped) {
        return fail(
            "The copied file's shader locations did not pass the safety check. The incomplete copy was not saved.",
        );
    }
    Ok(())
}

pub fn recover(source: &Path, destination: &Path, limit: u64) -> Result<Vec<Report>> {
    let source = source.canonicalize()?;
    crate::progress(
        "Checking the copy's destination. Existing Shader Guard files will not be overwritten.",
    );
    if destination.exists() || destination.is_symlink() {
        return fail(
            "This game already has a Shader Guard folder. Existing files were kept, so copying stopped. Reuse them or move that folder to a backup before trying again.",
        );
    }
    let parent = destination
        .parent()
        .ok_or("The shader copy needs a folder to save into. The destination path is invalid.")?;
    // Check existing ancestors before mkdir can change the source tree.
    for ancestor in parent.ancestors() {
        match ancestor.canonicalize() {
            Ok(path) => {
                if path.starts_with(&source) {
                    return fail(
                        "The original shader folder and the copy must be in separate locations. Choose a destination outside the original folder.",
                    );
                }
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        }
    }
    fs::create_dir_all(parent)?;
    let destination = parent.canonicalize()?.join(
        destination
            .file_name()
            .ok_or("The shader copy's destination has no folder name.")?,
    );
    if destination.starts_with(&source) || source.starts_with(&destination) {
        return fail(
            "The original shader folder and the copy must be in separate locations. Choose a destination outside the original folder.",
        );
    }
    let files = pairs(&source)?;
    let before = files
        .iter()
        .flat_map(|p| [p.clone(), p.with_extension("bin")])
        .map(|p| Ok((p.clone(), stamp(&p)?)))
        .collect::<Result<Vec<_>>>()?;
    crate::progress("Checking that the saved shader files can be copied safely.");
    let reports = scan(&source)?;
    let required = reports
        .iter()
        .try_fold(64 * 1024 * 1024u64, |sum, r| {
            sum.checked_add(r.bin_bytes)?.checked_add(r.toc_bytes)
        })
        .ok_or("The saved shader files are too large for this version to copy safely.")?;
    let available = crate::free_space(parent)?;
    crate::progress(format_args!(
        "Checking free space: about {} MiB needed, {} MiB available.",
        required.div_ceil(1024 * 1024),
        available / (1024 * 1024)
    ));
    if available < required {
        return fail(format!(
            "Not enough free disk space to keep a separate copy. Make sure about {} MiB is free, then try again. The original shader files were kept.",
            required.div_ceil(1024 * 1024)
        ));
    }
    let stage = tempfile::Builder::new()
        .prefix(".shader-guard-")
        .tempdir_in(parent)?;
    let mut names = BTreeSet::new();
    for (i, path) in files.iter().enumerate() {
        crate::progress(format_args!(
            "Copying saved shader file {}/{}: {}",
            i + 1,
            files.len(),
            path.strip_prefix(&source)?.display()
        ));
        let relative = path.strip_prefix(&source)?;
        let directory = stage.path().join("nvidia").join(
            relative
                .parent()
                .ok_or("A saved shader file has no parent folder. Copying stopped.")?,
        );
        fs::create_dir_all(&directory)?;
        let (toc, rows, _) = read_records(path)?;
        let input = File::open(path.with_extension("bin"))?;
        let stem = format!(
            "sg_{:x}",
            Sha256::digest(relative.as_os_str().as_encoded_bytes())
        );
        let mut start = 0;
        let mut shard = 0;
        while start < rows.len() {
            let mut end = start;
            let mut length = 32u64;
            while end < rows.len() {
                let bytes = 32 + u64::from(rows[end].size);
                if bytes + 32 >= limit {
                    return fail(
                        "One saved shader entry is too large to copy safely with this version. The original file was kept.",
                    );
                }
                if length + bytes >= limit {
                    break;
                }
                length += bytes;
                end += 1;
            }
            let name = format!("{}_{shard}", &stem[..23]);
            crate::progress(format_args!("Copying part {} of this file.", shard + 1));
            write_shard(
                &input,
                &toc[..32],
                &rows[start..end],
                &directory.join(&name),
            )?;
            names.insert(name);
            shard += 1;
            start = end;
        }
    }
    crate::progress("Checking that the original shader files did not change while copying.");
    if pairs(&source)? != files
        || before
            .iter()
            .any(|(p, s)| stamp(p).ok().as_ref() != Some(s))
    {
        return fail(
            "Shader files changed while copying. The incomplete copy was not saved. Fully exit Steam and any running games, then try again.",
        );
    }
    if !names.is_empty() {
        fs::write(
            stage.path().join("readonly-names.txt"),
            names.into_iter().collect::<Vec<_>>().join(";") + "\n",
        )?;
    }
    atomic_json(&stage.path().join("recovery.json"), &reports)?;
    crate::progress(format_args!(
        "Saving the checked shader copy: {}",
        destination.display()
    ));
    crate::rename_new(stage.path(), &destination)?;
    Ok(reports)
}
