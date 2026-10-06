#![cfg(target_os = "linux")]
mod cache;
mod cli;
mod steam;
mod vdf;

use clap::{CommandFactory, Parser};
use cli::{CacheOptions, Cli, EnableOptions, SteamOptions};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::{CString, OsString},
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(message.into().into())
}

fn rename_new(from: &Path, to: &Path) -> Result<()> {
    let a = CString::new(from.as_os_str().as_bytes())?;
    let b = CString::new(to.as_os_str().as_bytes())?;
    // CString pointers stay valid for the syscall; RENAME_NOREPLACE prevents races.
    let status = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn free_space(path: &Path) -> Result<u64> {
    let p = CString::new(path.as_os_str().as_bytes())?;
    let mut result = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // statvfs initializes result on success; the path is a valid C string.
    if unsafe { libc::statvfs(p.as_ptr(), result.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = unsafe { result.assume_init() };
    result
        .f_bavail
        .checked_mul(result.f_frsize)
        .ok_or_else(|| "The available disk space could not be calculated safely.".into())
}
fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    if path.is_symlink() {
        return fail(
            "This file is a symbolic link. It was not replaced, to protect the file it points to.",
        );
    }
    let parent = path
        .parent()
        .ok_or("The file's destination has no parent folder.")?;
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?, 0o600)
}
fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn progress(message: impl std::fmt::Display) {
    eprintln!("Shader Guard: {message}");
}
fn report_summary(reports: &[cache::Report]) {
    let records: usize = reports.iter().map(|r| r.records).sum();
    let wrapped: usize = reports.iter().map(|r| r.wrapped_offsets).sum();
    progress(format_args!(
        "Cache file pairs inspected: {}. Records: {records}. Wrapped offsets: {wrapped}.",
        reports.len()
    ));
}
fn cache_report(reports: &[cache::Report], json: bool) -> Result<()> {
    if json {
        return print_json(&reports);
    }
    let records: usize = reports.iter().map(|r| r.records).sum();
    let wrapped: usize = reports.iter().map(|r| r.wrapped_offsets).sum();
    if records == 0 {
        println!("No saved shader entries were found. You can skip the copy step.");
    } else {
        println!("Sets of saved shader files checked: {}.", reports.len());
        if wrapped == 0 {
            println!("No broken file positions were found.");
        } else {
            println!("Found {wrapped} stored entries with broken file positions.");
        }
    }
    Ok(())
}

#[derive(Clone)]
struct Paths {
    home: PathBuf,
    data: PathBuf,
    state: PathBuf,
    bin: PathBuf,
}
impl Paths {
    fn new() -> Result<Self> {
        let home = PathBuf::from(
            std::env::var_os("SHADER_GUARD_HOME")
                .or_else(|| std::env::var_os("HOME"))
                .ok_or("Your home folder could not be found because HOME is not set. Run this command from your normal desktop terminal.")?,
        );
        if !home.is_absolute() {
            return fail(
                "The home folder path must start with '/'. Check HOME or SHADER_GUARD_HOME.",
            );
        }
        let xdg = |key: &str, default: PathBuf| -> Result<PathBuf> {
            let path = std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or(default);
            if !path.is_absolute() {
                return fail(format!(
                    "{key} must be a full folder path starting with '/'."
                ));
            }
            Ok(path)
        };
        Ok(Self {
            data: xdg("XDG_DATA_HOME", home.join(".local/share"))?.join("steam-shader-guard"),
            state: xdg("XDG_STATE_HOME", home.join(".local/state"))?.join("steam-shader-guard"),
            bin: home.join(".local/bin/steam-shader-guard"),
            home,
        })
    }
    fn app(&self, id: &str) -> PathBuf {
        self.data.join("games").join(id)
    }
    fn state_file(&self) -> PathBuf {
        self.state.join("state.json")
    }
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    files: BTreeMap<PathBuf, ManagedFile>,
    #[serde(default)]
    apps: Vec<ManagedApp>,
    #[serde(default)]
    steam_roots: std::collections::BTreeSet<PathBuf>,
}
#[derive(Serialize, Deserialize)]
struct ManagedFile {
    before: Option<Vec<u8>>,
    before_mode: u32,
    after_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_hash: Option<String>,
}
impl ManagedFile {
    fn matches_hash(&self, hash: &str) -> bool {
        self.after_hash == hash || self.pending_hash.as_deref() == Some(hash)
    }
}
#[derive(Serialize, Deserialize)]
struct ManagedApp {
    config: PathBuf,
    appid: String,
    before: Option<String>,
    after: String,
}
fn load_state(paths: &Paths) -> Result<State> {
    let file = paths.state_file();
    if file.is_symlink() {
        return fail(
            "Shader Guard's saved settings file is a symbolic link. It was not used, to protect another file.",
        );
    }
    if file.exists() {
        Ok(serde_json::from_slice(&fs::read(file)?)?)
    } else {
        Ok(State::default())
    }
}
fn lock(paths: &Paths) -> Result<File> {
    // Mutation only: never acquire/create state during doctor or a preview.
    if unsafe { libc::geteuid() } == 0 {
        return fail(
            "Do not use sudo or the root account. Run this command as the user who plays games on this desktop.",
        );
    }
    fs::create_dir_all(&paths.state)?;
    fs::set_permissions(&paths.state, fs::Permissions::from_mode(0o700))?;
    let path = paths.state.join("lock");
    if path.is_symlink() {
        return fail(
            "Shader Guard's operation lock is a symbolic link. No changes were started, to protect another file.",
        );
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    // The file descriptor is owned by file, which holds the lock until drop.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return fail(
            "Another Shader Guard command is still making changes. Wait for it to finish, then try again.",
        );
    }
    Ok(file)
}
fn require_idle() -> Result<()> {
    let uid = unsafe { libc::geteuid() };
    for e in fs::read_dir("/proc")? {
        let e = e?;
        if !e
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        if e.metadata().map(|m| m.uid() != uid).unwrap_or(true) {
            continue;
        }
        let name = fs::read_to_string(e.path().join("comm"))
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if name == "steam"
            || name.starts_with("fossilize")
            || name.starts_with("wineserver")
            || name.ends_with(".exe")
        {
            return fail(
                "Fully exit Steam and any running games before changing game settings or copying saved shaders. Closing only the Steam window may leave it running.",
            );
        }
    }
    Ok(())
}
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 20
        || !id.bytes().all(|c| c.is_ascii_digit())
        || id.parse::<u64>()? == 0
    {
        return fail(
            "The game or account ID must be a number greater than zero. Run doctor to find your game's ID.",
        );
    }
    Ok(())
}

fn main() {
    if let Err(e) = entry() {
        eprintln!("Shader Guard: {e}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let Some(command) = Cli::parse().command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    let paths = Paths::new()?;
    match command {
        cli::Command::Doctor(args) => steam::doctor(&paths, &args.steam, args.json),
        cli::Command::Scan(args) => {
            progress(format_args!(
                "Checking saved shader files for game {}. Nothing will be changed.",
                args.id
            ));
            let source = steam::source(&paths, &args)?;
            progress(format_args!("Reading from: {}", source.display()));
            let reports = cache::scan(&source)?;
            cache_report(&reports, args.json)?;
            if args.json {
                report_summary(&reports);
            }
            progress(format_args!(
                "Check finished. No files changed. To copy reusable shaders, run recover {} --apply before the first Shader Guard game launch.",
                args.id
            ));
            Ok(())
        }
        cli::Command::Recover { cache: args, apply } => {
            progress(if apply {
                "Preparing to copy saved shaders. Your original files will be kept."
            } else {
                "Showing the shader copy plan. Nothing will be changed."
            });
            let source = steam::source(&paths, &args)?;
            let destination = paths.app(&args.id);
            progress(format_args!("Reading from: {}", source.display()));
            progress(format_args!(
                "Shader Guard will save its copy in: {}",
                destination.join("nvidia").display()
            ));
            if !apply {
                let reports = cache::scan(&source)?;
                cache_report(&reports, args.json)?;
                if args.json {
                    report_summary(&reports);
                }
                progress(format_args!(
                    "No files changed. Add --apply to copy the reusable shaders: recover {} --apply.",
                    args.id
                ));
                return Ok(());
            }
            progress(
                "Checking for Steam and game processes that could still be using these files.",
            );
            require_idle()?;
            let _lock = lock(&paths)?;
            let reports = cache::recover(&source, &destination, cache::SHARD_LIMIT)?;
            if args.json {
                print_json(&reports)?;
                report_summary(&reports);
            }
            progress(format_args!(
                "Shader copy finished and checked. Saved in: {}.",
                destination.join("nvidia").display()
            ));
            progress(format_args!(
                "The game's settings have not changed. Next: enable {} --apply, then start 'Steam (Shader Guard)' from your application menu.",
                args.id
            ));
            Ok(())
        }
        cli::Command::Install { apply } => steam::install(&paths, apply),
        cli::Command::Enable(args) => steam::enable(&paths, &args),
        cli::Command::Disable { id, apply } => steam::disable(&paths, Some(&id), apply, false),
        cli::Command::Uninstall { apply } => steam::disable(&paths, None, apply, true),
        cli::Command::Run { command } => steam::run_game(&paths, command),
        cli::Command::Steam { arguments } => steam::run_steam(arguments),
    }
}

#[cfg(test)]
mod tests;
