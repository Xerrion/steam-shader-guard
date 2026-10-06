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
        .ok_or_else(|| "Free-space overflow".into())
}
fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    if path.is_symlink() {
        return fail("Refusing to replace a symlink");
    }
    let parent = path.parent().ok_or("Missing parent directory")?;
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
                .ok_or("HOME is missing")?,
        );
        if !home.is_absolute() {
            return fail("Home directory must be absolute");
        }
        let xdg = |key: &str, default: PathBuf| -> Result<PathBuf> {
            let path = std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or(default);
            if !path.is_absolute() {
                return fail(format!("{key} must be absolute"));
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
        return fail("State file is a symlink");
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
        return fail("Run as your desktop user, without sudo");
    }
    fs::create_dir_all(&paths.state)?;
    fs::set_permissions(&paths.state, fs::Permissions::from_mode(0o700))?;
    let path = paths.state.join("lock");
    if path.is_symlink() {
        return fail("Lock path is a symlink");
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    // The file descriptor is owned by file, which holds the lock until drop.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return fail("Another Shader Guard operation is running");
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
                "Close Steam and Wine games before changing configuration or recovering live caches",
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
        return fail("App ID must be a positive decimal number");
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
        cli::Command::Doctor(args) => steam::doctor(&paths, &args),
        cli::Command::Scan(args) => print_json(&cache::scan(&steam::source(&paths, &args)?)?),
        cli::Command::Recover { cache: args, apply } => {
            let source = steam::source(&paths, &args)?;
            if !apply {
                print_json(&cache::scan(&source)?)?;
                println!("Preview only. Re-run with --apply to create a separate verified copy.");
                return Ok(());
            }
            require_idle()?;
            let _lock = lock(&paths)?;
            print_json(&cache::recover(
                &source,
                &paths.app(&args.id),
                cache::SHARD_LIMIT,
            )?)?;
            println!("Recovered cache saved. Use enable to connect the game's launch option.");
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
