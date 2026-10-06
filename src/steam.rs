use crate::vdf::{Value, Vdf};
use crate::*;
use std::collections::BTreeSet;

const PREFIX: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];
#[derive(Clone, Serialize)]
pub struct Game {
    pub appid: String,
    pub name: String,
    pub cache: PathBuf,
}

fn root(paths: &Paths, args: &Args) -> Result<PathBuf> {
    find_root(paths, args)?
        .ok_or_else(|| "Native Steam installation not found; use --steam-root PATH".into())
}
fn find_root(paths: &Paths, args: &Args) -> Result<Option<PathBuf>> {
    let candidates = args.root.clone().map(|p| vec![p]).unwrap_or_else(|| {
        vec![
            paths.home.join(".local/share/Steam"),
            paths.home.join(".steam/root"),
            paths.home.join(".steam/steam"),
        ]
    });
    for path in candidates {
        let metadata = match fs::metadata(path.join("steamapps")) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if metadata.is_dir() {
            let path = path.canonicalize()?;
            if path
                .components()
                .any(|p| p.as_os_str() == ".var" || p.as_os_str() == "snap")
            {
                return fail("Flatpak and Snap Steam are not supported in version 0.1");
            }
            return Ok(Some(path));
        }
    }
    Ok(None)
}
pub fn games(root: &Path) -> Result<Vec<Game>> {
    let mut libraries = BTreeSet::from([root.to_path_buf()]);
    let library_file = root.join("steamapps/libraryfolders.vdf");
    if library_file.exists() {
        let doc = Vdf::parse(fs::read_to_string(library_file)?)?;
        if let Some(Value::Map(entries)) = doc.get(&["libraryfolders"]) {
            for value in entries.values() {
                if let Value::Map(m) = value {
                    if let Some(Value::Text(path)) = m.get("path") {
                        libraries.insert(PathBuf::from(path));
                    }
                }
            }
        }
    }
    let mut result = BTreeMap::new();
    for library in libraries {
        let apps = library.join("steamapps");
        if !apps.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&apps)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                continue;
            }
            let doc = Vdf::parse(fs::read_to_string(entry.path())?)?;
            let id = doc
                .text(&["AppState", "appid"])
                .ok_or("Manifest has no app ID")?;
            valid_id(id)?;
            let name = doc
                .text(&["AppState", "name"])
                .unwrap_or("Unnamed application");
            let lower = name.to_lowercase();
            if lower.starts_with("proton")
                || lower.starts_with("steam linux runtime")
                || lower.starts_with("steamworks common")
            {
                continue;
            }
            result.insert(
                id.to_string(),
                Game {
                    appid: id.into(),
                    name: name.into(),
                    cache: apps.join("shadercache").join(id).join("nvidiav1"),
                },
            );
        }
    }
    Ok(result.into_values().collect())
}
pub fn source(paths: &Paths, args: &Args, id: &str) -> Result<PathBuf> {
    if let Some(path) = &args.source {
        return Ok(path.canonicalize()?);
    }
    games(&root(paths, args)?)?
        .into_iter()
        .find(|g| g.appid == id)
        .map(|g| g.cache)
        .ok_or_else(|| "Installed application not found".into())
}
fn account_config(root: &Path, account: Option<&str>) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    for e in fs::read_dir(root.join("userdata"))? {
        let e = e?;
        let id = e.file_name().to_string_lossy().into_owned();
        if valid_id(&id).is_err() || account.is_some_and(|a| a != id) {
            continue;
        }
        let p = e.path().join("config/localconfig.vdf");
        if p.is_file() {
            candidates.push(p);
        }
    }
    if candidates.len() != 1 {
        return fail("Select one Steam account with --account ID (see the userdata folder)");
    }
    let path = candidates.pop().unwrap();
    if path.is_symlink() {
        return fail("Steam localconfig is a symlink; edit launch options manually");
    }
    Ok(path)
}
pub fn doctor(paths: &Paths, args: &Args) -> Result<()> {
    let steam_root = root(paths, args)?;
    let mut output = Vec::new();
    for game in games(&steam_root)? {
        let private = paths.app(&game.appid);
        output.push(serde_json::json!({"appid":game.appid,"name":game.name,
            "steam_nvidia_cache_exists":game.cache.is_dir(),"private_cache_exists":private.join("nvidia").is_dir(),
            "recovered_seeds_present":private.join("readonly-names.txt").is_file()}));
    }
    print_json(
        &serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"steam_root":steam_root,
        "nvidia_driver_loaded":Path::new("/proc/driver/nvidia/version").exists(),
        "installed_launcher":paths.bin.is_file(),"applications":output,
        "scope":"native Steam on Linux; Flatpak/Snap unsupported; no GPU performance tuning"}),
    )
}

fn desktop_quote(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("Desktop path is not UTF-8")?;
    if text.contains(['\n', '\r']) {
        return fail("Newlines in executable path are unsupported");
    }
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\\\\\")
            .replace('"', "\\\\\\\"")
            .replace('`', "\\\\`")
            .replace('$', "\\\\$")
            .replace('%', "%%")
    ))
}
fn shell_quote(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("Executable path is not UTF-8")?;
    if text.contains(['\n', '\r', '%']) {
        return fail("Newlines and percent signs in Steam wrapper paths are unsupported");
    }
    Ok(format!("'{}'", text.replace('\'', "'\"'\"'")))
}
pub fn install(paths: &Paths, apply: bool) -> Result<()> {
    let icon = paths
        .data
        .parent()
        .ok_or("Missing XDG data directory")?
        .join("applications/steam-shader-guard.desktop");
    let desktop = format!(
        "[Desktop Entry]\nName=Steam (Shader Guard)\nComment=Start Steam with NVIDIA replay disabled and driver caching preserved\nExec={} steam %U\nIcon=steam\nTerminal=false\nType=Application\nCategories=Game;\n",
        desktop_quote(&paths.bin)?
    );
    println!(
        "Install program: {}\nAdd menu entry: Steam (Shader Guard)\nExisting Steam launchers remain unchanged.",
        paths.bin.display()
    );
    if !apply {
        println!("Preview only; add --apply to install.");
        return Ok(());
    }
    let _lock = lock(paths)?;
    let mut state = load_state(paths)?;
    let updates = [
        (
            paths.bin.clone(),
            fs::read(std::env::current_exe()?)?,
            0o755,
        ),
        (icon, desktop.into_bytes(), 0o644),
    ];
    let mut current_hashes = BTreeMap::new();
    for (path, _, _) in &updates {
        if path.is_symlink() {
            return fail("Installation target is a symlink");
        }
        if path.try_exists()? {
            let current_hash = cache::digest_file(path)?;
            if !state
                .files
                .get(path)
                .is_some_and(|e| e.matches_hash(&current_hash))
            {
                return fail(format!(
                    "Preserving existing or modified file: {}",
                    path.display()
                ));
            }
            current_hashes.insert(path.clone(), current_hash);
        }
    }
    for (path, bytes, mode) in updates {
        let entry = state.files.entry(path.clone()).or_insert(ManagedFile {
            before: None,
            before_mode: mode,
            after_hash: String::new(),
            pending_hash: None,
        });
        if let Some(current_hash) = current_hashes.remove(&path) {
            entry.after_hash = current_hash;
        }
        entry.pending_hash = Some(hash(&bytes));
        atomic_json(&paths.state_file(), &state)?; // Journal before each reversible write.
        atomic_write(&path, &bytes, mode)?;
        let entry = state
            .files
            .get_mut(&path)
            .ok_or("Missing managed file journal")?;
        entry.after_hash = entry
            .pending_hash
            .take()
            .ok_or("Missing pending file hash")?;
        atomic_json(&paths.state_file(), &state)?;
    }
    println!(
        "Installed. Start Steam through 'Steam (Shader Guard)'. Enable each game's cache profile separately."
    );
    Ok(())
}
pub fn enable(paths: &Paths, args: &Args) -> Result<()> {
    if args.all == args.id.is_some() {
        return fail("Choose one app ID or --all");
    }
    let steam_root = root(paths, args)?;
    let config = account_config(&steam_root, args.account.as_deref())?;
    let selected = games(&steam_root)?
        .into_iter()
        .filter(|g| args.all || args.id.as_deref() == Some(&g.appid))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return fail("No installed applications matched");
    }
    let original = fs::read_to_string(&config)?;
    let mut doc = Vdf::parse(original.clone())?;
    if !matches!(doc.get(&PREFIX), Some(Value::Map(_))) {
        return fail("Unrecognized Steam account configuration layout");
    }
    let value = format!("{} run -- %command%", shell_quote(&paths.bin)?);
    let mut updates = Vec::new();
    for game in selected {
        let mut key = PREFIX.to_vec();
        key.extend([&game.appid, "LaunchOptions"]);
        let before = doc.text(&key).map(String::from);
        if before.as_ref().is_some_and(|s| !s.trim().is_empty()) {
            println!(
                "Preserved {} ({}): existing launch options. Add the wrapper manually if appropriate.",
                game.name, game.appid
            );
            continue;
        }
        println!("Enable {} ({}): {value}", game.name, game.appid);
        let modified = doc.set(&key, Some(&value))?;
        doc = Vdf::parse(modified)?;
        updates.push(ManagedApp {
            config: config.clone(),
            appid: game.appid,
            before,
            after: value.clone(),
        });
    }
    if !args.apply {
        println!(
            "Preview only; add --apply to save. Existing custom launch options are never overwritten."
        );
        return Ok(());
    }
    require_idle()?;
    let _lock = lock(paths)?;
    let mut state = load_state(paths)?;
    if !state
        .files
        .get(&paths.bin)
        .is_some_and(|e| cache::digest_file(&paths.bin).is_ok_and(|hash| e.matches_hash(&hash)))
    {
        return fail("Run install --apply first; the managed program must be present");
    }
    if fs::read_to_string(&config)? != original {
        return fail("Steam configuration changed; retry after closing Steam");
    }
    let mut modified = original;
    state.steam_roots.insert(steam_root);
    for update in updates {
        let mut key = PREFIX.to_vec();
        key.extend([&update.appid, "LaunchOptions"]);
        modified = Vdf::parse(modified)?.set(&key, Some(&update.after))?;
        state
            .apps
            .retain(|a| !(a.config == update.config && a.appid == update.appid));
        state.apps.push(update);
    }
    atomic_json(&paths.state_file(), &state)?;
    atomic_write(
        &config,
        modified.as_bytes(),
        fs::metadata(&config)?.mode() & 0o777,
    )?;
    println!(
        "Launch options saved. Shader recovery is separate; newly encountered shaders may still compile."
    );
    Ok(())
}

fn read_config(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn disable(paths: &Paths, id: Option<&str>, apply: bool, uninstall: bool) -> Result<()> {
    if !uninstall && id.is_none() {
        return fail("Specify an app ID to disable");
    }
    if !apply {
        let state = load_state(paths)?;
        println!(
            "Preview: restore {} managed launch options{}; keep all cache data.",
            state
                .apps
                .iter()
                .filter(|a| id.is_none_or(|id| a.appid == id))
                .count(),
            if uninstall {
                " and remove unchanged managed program/menu files"
            } else {
                ""
            }
        );
        return Ok(());
    }
    require_idle()?;
    let _lock = lock(paths)?;
    let mut state = load_state(paths)?;
    let mut edits: BTreeMap<PathBuf, (String, String)> = BTreeMap::new();
    for entry in state
        .apps
        .iter()
        .filter(|a| id.is_none_or(|id| a.appid == id))
    {
        let pair = match edits.entry(entry.config.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let Some(original) = read_config(entry.key())? else {
                    continue;
                };
                entry.insert((original.clone(), original))
            }
        };
        let doc = Vdf::parse(pair.1.clone())?;
        let mut key = PREFIX.to_vec();
        key.extend([&entry.appid, "LaunchOptions"]);
        let current = doc.text(&key);
        if current == Some(&entry.after) {
            pair.1 = doc.set(&key, entry.before.as_deref())?;
        } else if current.is_some_and(|v| v.contains("steam-shader-guard")) {
            return fail(
                "A modified launch option still uses Shader Guard; remove that reference manually first",
            );
        } else {
            println!(
                "Preserved subsequently changed launch option for {}",
                entry.appid
            );
        }
    }
    for (path, (before, after)) in &edits {
        if fs::read_to_string(path)? != *before {
            return fail("Steam configuration changed; no changes saved");
        }
        if before != after {
            atomic_write(path, after.as_bytes(), fs::metadata(path)?.mode() & 0o777)?;
        }
    }
    let known_configs = state
        .apps
        .iter()
        .map(|a| a.config.clone())
        .collect::<BTreeSet<_>>();
    state.apps.retain(|a| id.is_some_and(|id| a.appid != id));
    atomic_json(&paths.state_file(), &state)?;
    if uninstall {
        // Includes manually edited/untracked app entries in every known account.
        // Also inspect accounts in the default Steam installation, if available.
        let mut configs = known_configs;
        let mut roots = state.steam_roots.clone();
        if let Some(steam_root) = find_root(paths, &Args::default())? {
            roots.insert(steam_root);
        }
        for steam_root in roots {
            let accounts = match fs::read_dir(steam_root.join("userdata")) {
                Ok(accounts) => accounts,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            for account in accounts {
                let account = account?;
                let metadata = match fs::metadata(account.path()) {
                    Ok(metadata) => metadata,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                if metadata.is_dir() {
                    configs.insert(account.path().join("config/localconfig.vdf"));
                }
            }
        }
        for config in configs {
            let Some(text) = read_config(&config)? else {
                continue;
            };
            let doc = Vdf::parse(text)?;
            if let Some(Value::Map(apps)) = doc.get(&PREFIX) {
                if apps.values().any(|value| matches!(value, Value::Map(fields) if matches!(fields.get("launchoptions"), Some(Value::Text(option)) if option.contains("steam-shader-guard")))) {
                    return fail("A Steam launch option still references Shader Guard; remove it manually before uninstalling");
                }
            }
        }
        // Do not remove a binary still referenced by a changed managed desktop entry.
        for (path, record) in &state.files {
            if path != &paths.bin
                && path.try_exists()?
                && !record.matches_hash(&cache::digest_file(path)?)
                && fs::read_to_string(path)?.contains("steam-shader-guard")
            {
                return fail(
                    "A modified menu entry still references Shader Guard; program retained",
                );
            }
        }
        let files = std::mem::take(&mut state.files);
        for (path, record) in files {
            if !path.try_exists()? {
                continue;
            }
            if path.is_symlink() || !record.matches_hash(&cache::digest_file(&path)?) {
                println!("Preserved modified file: {}", path.display());
                continue;
            }
            if let Some(before) = record.before {
                atomic_write(&path, &before, record.before_mode)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        atomic_json(&paths.state_file(), &state)?;
    }
    println!("Done. Source caches, recovered caches and new game shader data were retained.");
    Ok(())
}

pub fn game_environment(
    paths: &Paths,
    id: &str,
    env: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    valid_id(id)?;
    let mut out = env.clone();
    let existing = env
        .get("__GL_SHADER_DISK_CACHE_PATH")
        .map(String::as_str)
        .unwrap_or("");
    let steam_suffix = format!("/steamapps/shadercache/{id}/nvidiav1");
    if !existing.is_empty() && !existing.ends_with(&steam_suffix) {
        return Ok(out);
    }
    let directory = paths.app(id).join("nvidia");
    fs::create_dir_all(&directory)?;
    out.entry("__GL_SHADER_DISK_CACHE_SIZE".into())
        .or_insert("12000000000".into());
    out.insert(
        "__GL_SHADER_DISK_CACHE_PATH".into(),
        directory.to_str().ok_or("Cache path is not UTF-8")?.into(),
    );
    out.insert(
        "__GL_SHADER_DISK_CACHE_APP_NAME".into(),
        "shader_guard".into(),
    );
    let names = paths.app(id).join("readonly-names.txt");
    if names.exists() {
        let value = fs::read_to_string(names)?.trim().to_string();
        if value.split(';').any(|name| {
            name.is_empty()
                || name.len() > 128
                || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        }) {
            return fail("Invalid read-only cache names");
        }
        out.insert("__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME".into(), value);
    } else {
        out.remove("__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME");
    }
    Ok(out)
}
pub fn run_game(paths: &Paths, mut args: Vec<OsString>) -> Result<()> {
    if args.first().is_some_and(|a| a == "--") {
        args.remove(0);
    }
    if args.is_empty() {
        return fail("Use run -- GAME [ARGUMENTS...]");
    }
    let id = ["SteamAppId", "SteamGameId", "STEAM_COMPAT_APP_ID"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .ok_or("Launch this wrapper through Steam; app ID is missing")?;
    let selected = [
        "__GL_SHADER_DISK_CACHE_PATH",
        "__GL_SHADER_DISK_CACHE_SIZE",
        "__GL_SHADER_DISK_CACHE_APP_NAME",
        "__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME",
    ];
    let env = selected
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect();
    let env = game_environment(paths, &id, &env)?;
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..]);
    for key in selected {
        cmd.env_remove(key);
    }
    cmd.envs(env);
    Err(cmd.exec().into())
}
pub fn run_steam(args: Vec<OsString>) -> Result<()> {
    let path = ["/usr/bin/steam", "/usr/games/steam"]
        .iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("Native Steam launcher not found")?;
    Err(Command::new(path)
        .args(["+bShaderAllowReplayOnNVIDIA", "0"])
        .args(args)
        .exec()
        .into())
}
