#![cfg(target_os = "linux")]
mod cache;
mod steam;
mod vdf;

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
            let path = std::env::var_os(key).map(PathBuf::from).unwrap_or(default);
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

const HELP: &str = "Steam Shader Guard 0.1.0 (Linux, native Steam, NVIDIA)\n\
\nUsage:\n\
  steam-shader-guard doctor [--steam-root PATH]\n\
  steam-shader-guard scan APPID [--source PATH] [--steam-root PATH]\n\
  steam-shader-guard recover APPID [--source PATH] [--steam-root PATH] [--apply]\n\
  steam-shader-guard install [--apply]\n\
  steam-shader-guard enable APPID|--all [--account ID] [--steam-root PATH] [--apply]\n\
  steam-shader-guard disable APPID [--apply]\n\
  steam-shader-guard uninstall [--apply]\n\
  steam-shader-guard run -- GAME [ARGUMENTS...]\n\
  steam-shader-guard steam [STEAM ARGUMENTS...]\n\
Changes are previewed unless --apply is present. Originals and generated cache\n\
data are retained. No network access, telemetry or elevated privileges.\n";

#[derive(Default)]
struct Args {
    apply: bool,
    all: bool,
    id: Option<String>,
    account: Option<String>,
    root: Option<PathBuf>,
    source: Option<PathBuf>,
}
fn parse(command: &str, args: Vec<OsString>) -> Result<Args> {
    let allowed: &[&str] = match command {
        "doctor" => &["--steam-root"],
        "scan" => &["--source", "--steam-root"],
        "recover" => &["--source", "--steam-root", "--apply"],
        "install" | "disable" | "uninstall" => &["--apply"],
        "enable" => &["--all", "--account", "--steam-root", "--apply"],
        _ => return fail("Unknown command; use --help"),
    };
    let mut out = Args::default();
    let mut seen = std::collections::BTreeSet::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.to_str().ok_or("Option is not valid UTF-8")?;
        if arg.starts_with('-') && (!allowed.contains(&arg) || !seen.insert(arg.to_string())) {
            return fail(format!(
                "Unknown or duplicate argument for {command}: {arg}"
            ));
        }
        match arg {
            "--apply" => out.apply = true,
            "--all" => out.all = true,
            "--steam-root" => out.root = Some(args.next().ok_or("Missing root path")?.into()),
            "--source" => out.source = Some(args.next().ok_or("Missing cache source")?.into()),
            "--account" => {
                out.account = Some(
                    args.next()
                        .ok_or("Missing account ID")?
                        .into_string()
                        .map_err(|_| "Invalid account ID")?,
                )
            }
            value
                if !value.starts_with('-')
                    && out.id.is_none()
                    && matches!(command, "scan" | "recover" | "enable" | "disable") =>
            {
                out.id = Some(value.into())
            }
            value => return fail(format!("Unknown or duplicate argument: {value}")),
        }
    }
    if let Some(id) = &out.id {
        valid_id(id)?;
    }
    if let Some(id) = &out.account {
        valid_id(id)?;
    }
    Ok(out)
}

fn entry() -> Result<()> {
    let mut argv = std::env::args_os().skip(1);
    let command = argv.next().unwrap_or_else(|| "--help".into());
    let rest = argv.collect::<Vec<_>>();
    if command == "--help"
        || command == "-h"
        || (command != "run" && command != "steam" && rest.iter().any(|s| s == "--help"))
    {
        println!("{HELP}");
        return Ok(());
    }
    if command == "--version" {
        println!("steam-shader-guard {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let paths = Paths::new()?;
    if command == "run" {
        return steam::run_game(&paths, rest);
    }
    if command == "steam" {
        return steam::run_steam(rest);
    }
    let command = command.to_str().ok_or("Invalid command")?;
    let args = parse(command, rest)?;
    match command {
        "doctor" => steam::doctor(&paths, &args),
        "scan" | "recover" => {
            let id = args.id.as_deref().ok_or("Specify a Steam app ID")?;
            let source = steam::source(&paths, &args, id)?;
            if command == "scan" || !args.apply {
                print_json(&cache::scan(&source)?)?;
                if command == "recover" {
                    println!(
                        "Preview only. Re-run with --apply to create a separate verified copy."
                    );
                }
            } else {
                require_idle()?;
                let _lock = lock(&paths)?;
                print_json(&cache::recover(
                    &source,
                    &paths.app(id),
                    cache::SHARD_LIMIT,
                )?)?;
                println!("Recovered cache saved. Use enable to connect the game's launch option.");
            }
            Ok(())
        }
        "install" => steam::install(&paths, args.apply),
        "enable" => steam::enable(&paths, &args),
        "disable" => steam::disable(&paths, args.id.as_deref(), args.apply, false),
        "uninstall" => steam::disable(&paths, None, args.apply, true),
        _ => fail("Unknown command; use --help"),
    }
}

#[cfg(test)]
mod tests;
