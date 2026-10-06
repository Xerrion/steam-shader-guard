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

fn root(paths: &Paths, args: &SteamOptions) -> Result<PathBuf> {
    find_root(paths, args)?
        .ok_or_else(|| "Steam was not found. This tool needs the regular Linux Steam app, not Flatpak or Snap. If Steam is installed elsewhere, use --steam-root with its folder.".into())
}
fn find_root(paths: &Paths, args: &SteamOptions) -> Result<Option<PathBuf>> {
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
                return fail(
                    "This version cannot set up Flatpak or Snap Steam. Use the regular Linux Steam app instead.",
                );
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
                .ok_or("A Steam game information file has no game ID. Try verifying the game's files in Steam.")?;
            valid_id(id)?;
            let name = doc.text(&["AppState", "name"]).unwrap_or("Unnamed game");
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
pub fn source(paths: &Paths, args: &CacheOptions) -> Result<PathBuf> {
    if let Some(path) = &args.source {
        return Ok(path.canonicalize()?);
    }
    games(&root(paths, &args.steam)?)?
        .into_iter()
        .find(|g| g.appid == args.id)
        .map(|g| g.cache)
        .ok_or_else(|| {
            "That game was not found. Run doctor to see the IDs of your installed games.".into()
        })
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
        return fail(
            "A single Steam account could not be selected. Make sure you signed in to Steam. If several accounts use this computer, add --account with the number of your folder under Steam's userdata folder.",
        );
    }
    let path = candidates.pop().unwrap();
    if path.is_symlink() {
        return fail(
            "Steam's settings file is a symbolic link. To keep that setup safe, change the game's launch options in Steam instead.",
        );
    }
    Ok(path)
}
pub fn doctor(paths: &Paths, args: &SteamOptions, json: bool) -> Result<()> {
    progress("Checking your Steam installation and saved shaders. Nothing will be changed.");
    let steam_root = root(paths, args)?;
    progress(format_args!("Steam folder: {}", steam_root.display()));
    let installed = paths.bin.is_file();
    let nvidia = Path::new("/proc/driver/nvidia/version").exists();
    if !json {
        println!("Steam: found.");
        println!(
            "Shader Guard: {}.",
            if installed {
                "installed"
            } else {
                "not installed yet"
            }
        );
        println!(
            "NVIDIA driver: {}.",
            if nvidia {
                "detected"
            } else {
                "not detected. This tool requires NVIDIA graphics"
            }
        );
        println!("\nYour installed games:");
        println!("{:<12} {:<21} Game", "Game ID", "Steam shader folder");
    }
    let mut output = Vec::new();
    for game in games(&steam_root)? {
        let private = paths.app(&game.appid);
        if !json {
            println!(
                "{:<12} {:<21} {}",
                game.appid,
                if game.cache.is_dir() {
                    "found"
                } else {
                    "not found"
                },
                game.name
            );
        }
        output.push(serde_json::json!({"appid":game.appid,"name":game.name,
            "steam_nvidia_cache_exists":game.cache.is_dir(),"private_cache_exists":private.join("nvidia").is_dir(),
            "recovered_seeds_present":private.join("readonly-names.txt").is_file()}));
    }
    let count = output.len();
    if json {
        print_json(
            &serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"steam_root":steam_root,
        "nvidia_driver_loaded":nvidia,
        "installed_launcher":installed,"applications":output,
        "scope":"native Steam on Linux; Flatpak/Snap unsupported; no GPU performance tuning"}),
        )?;
    } else if count == 0 {
        println!("No games found. Install a game in Steam, then run doctor again.");
    } else {
        println!("\nUse the number in 'Game ID' when a command asks which game to set up.");
        println!("A shader folder does not mean the game is already using Shader Guard.");
        if !installed {
            println!("First, install the tool with install --apply.");
        }
    }
    progress(format_args!(
        "Check finished. Games found: {count}. No files changed."
    ));
    Ok(())
}

fn desktop_quote(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or(
        "The menu shortcut cannot use this folder name because it is not valid UTF-8 text.",
    )?;
    if text.contains(['\n', '\r']) {
        return fail(
            "The installation path contains a line break. Choose a home folder without line breaks in its name.",
        );
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
    let text = path
        .to_str()
        .ok_or("Steam cannot use this tool's path because it is not valid UTF-8 text.")?;
    if text.contains(['\n', '\r', '%']) {
        return fail(
            "Steam cannot use the tool's path because it contains a line break or percent sign. Use a home folder without those characters.",
        );
    }
    Ok(format!("'{}'", text.replace('\'', "'\"'\"'")))
}
pub fn install(paths: &Paths, apply: bool) -> Result<()> {
    progress(if apply {
        "Installing Shader Guard and adding its Steam shortcut."
    } else {
        "Showing the installation plan. Nothing will be changed."
    });
    let icon = paths
        .data
        .parent()
        .ok_or("The application data folder could not be determined. Check XDG_DATA_HOME.")?
        .join("applications/steam-shader-guard.desktop");
    let desktop = format!(
        "[Desktop Entry]\nName=Steam (Shader Guard)\nComment=Start Steam without NVIDIA shader pre-processing. Set up games with Shader Guard first.\nExec={} steam %U\nIcon=steam\nTerminal=false\nType=Application\nCategories=Game;\n",
        desktop_quote(&paths.bin)?
    );
    println!(
        "Tool location: {}\nNew application-menu shortcut: Steam (Shader Guard)\nShortcut file: {}\nYour usual Steam shortcuts will stay unchanged.\nThis only installs the tool. It does not set up games, copy shaders or start Steam.",
        paths.bin.display(),
        icon.display()
    );
    if !apply {
        println!("Nothing changed. Add --apply when you are ready to install.");
        return Ok(());
    }
    progress(
        "Checking existing installation files. Files changed outside Shader Guard will not be overwritten.",
    );
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
            return fail(
                "An installation file is a symbolic link. Installation stopped to avoid replacing a file that belongs to another setup.",
            );
        }
        if path.try_exists()? {
            let current_hash = cache::digest_file(path)?;
            if !state
                .files
                .get(path)
                .is_some_and(|e| e.matches_hash(&current_hash))
            {
                return fail(format!(
                    "Installation stopped because this file already exists or was changed outside Shader Guard: {}. It was not overwritten.",
                    path.display()
                ));
            }
            current_hashes.insert(path.clone(), current_hash);
        }
    }
    for (path, bytes, mode) in updates {
        progress(format_args!("Installing file: {}", path.display()));
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
            .ok_or("The saved installation record is missing. Installation stopped to avoid losing undo information.")?;
        entry.after_hash = entry
            .pending_hash
            .take()
            .ok_or("The saved installation check is missing. Installation stopped to avoid losing undo information.")?;
        atomic_json(&paths.state_file(), &state)?;
    }
    println!(
        "Shader Guard is installed. Your game settings have not changed.\n\
         Next: run doctor to find your game and its ID.\n\
         To reuse its saved shaders, run recover GAME_ID --apply before its first Shader Guard launch.\n\
         Then run enable GAME_ID --apply to set up that game. Replace GAME_ID with the number from doctor.\n\
         Finally, start 'Steam (Shader Guard)' from your application menu and play normally."
    );
    Ok(())
}
pub fn enable(paths: &Paths, args: &EnableOptions) -> Result<()> {
    if args.all == args.id.is_some() {
        return fail("Choose a game ID from doctor, or use --all to set up all eligible games.");
    }
    progress(if args.apply {
        "Preparing to set up games in Steam. This does not copy shaders or start Steam."
    } else {
        "Showing the game setup plan. Nothing will be changed."
    });
    let steam_root = root(paths, &args.steam)?;
    let config = account_config(&steam_root, args.account.as_deref())?;
    progress(format_args!(
        "Reading Steam settings from: {}",
        config.display()
    ));
    let selected = games(&steam_root)?
        .into_iter()
        .filter(|g| args.all || args.id.as_deref() == Some(&g.appid))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return fail("No installed games matched. Run doctor to find your game's ID.");
    }
    let original = fs::read_to_string(&config)?;
    let mut doc = Vdf::parse(original.clone())?;
    if !matches!(doc.get(&PREFIX), Some(Value::Map(_))) {
        return fail(
            "This version cannot safely read your Steam settings. No game settings were changed.",
        );
    }
    let value = format!("{} run -- %command%", shell_quote(&paths.bin)?);
    let mut updates = Vec::new();
    let mut skipped = 0;
    let mut already = 0;
    for game in selected {
        let mut key = PREFIX.to_vec();
        key.extend([&game.appid, "LaunchOptions"]);
        let before = doc.text(&key).map(String::from);
        if before.as_ref().is_some_and(|s| !s.trim().is_empty()) {
            if before.as_deref() == Some(&value) {
                already += 1;
                println!(
                    "Already has Shader Guard launch options: {} (game ID {}). Left unchanged.",
                    game.name, game.appid
                );
                continue;
            }
            skipped += 1;
            println!(
                "Skipped {} (game ID {}): it already has launch options, so they were left unchanged.\nTo add Shader Guard yourself, keep your existing options and place this around the game's command:\n{value}",
                game.name, game.appid
            );
            continue;
        }
        println!("Game to set up: {} (game ID {}).", game.name, game.appid);
        println!(
            "Shader Guard will save this game's shaders in: {}",
            paths.app(&game.appid).join("nvidia").display()
        );
        let modified = doc.set(&key, Some(&value))?;
        doc = Vdf::parse(modified)?;
        updates.push(ManagedApp {
            config: config.clone(),
            appid: game.appid,
            before,
            after: value.clone(),
        });
    }
    let connected = updates.len();
    println!(
        "Setup plan: {connected} to set up, {already} already set up, {skipped} with other launch options left unchanged."
    );
    if !args.apply {
        println!("Nothing changed. Add --apply when you are ready to save the game settings.");
        return Ok(());
    }
    progress(
        "Checking for running Steam or game processes, and checking the Shader Guard installation.",
    );
    require_idle()?;
    let _lock = lock(paths)?;
    let mut state = load_state(paths)?;
    if !state
        .files
        .get(&paths.bin)
        .is_some_and(|e| cache::digest_file(&paths.bin).is_ok_and(|hash| e.matches_hash(&hash)))
    {
        return fail(
            "Shader Guard is not installed or its installed file has changed. Run install --apply before setting up games.",
        );
    }
    if fs::read_to_string(&config)? != original {
        return fail(
            "Steam's settings changed while this command was running. Fully exit Steam, then try again.",
        );
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
    progress(format_args!("Saving game settings: {}", config.display()));
    atomic_json(&paths.state_file(), &state)?;
    atomic_write(
        &config,
        modified.as_bytes(),
        fs::metadata(&config)?.mode() & 0o777,
    )?;
    if connected == 0 {
        println!("No new games were set up. Existing launch options were kept.");
    } else {
        println!("Games set up: {connected}.");
    }
    println!(
        "Already set up: {already}. Games with other launch options left unchanged: {skipped}."
    );
    println!(
        "This command did not copy existing shaders. Use recover before the first Shader Guard launch if you want to reuse them."
    );
    if skipped > 0 {
        println!(
            "To add Shader Guard to a skipped game, follow the launch-option instructions above."
        );
    }
    if connected > 0 || already > 0 {
        println!(
            "Next: start 'Steam (Shader Guard)' from your application menu, then launch the game normally."
        );
    }
    println!("The game may still need to compile shaders it has not seen before.");
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
        return fail("Choose the game ID to stop using Shader Guard for. Run doctor to find it.");
    }
    progress(match (uninstall, apply) {
        (true, true) => "Preparing to remove Shader Guard. All saved shaders will be kept.",
        (true, false) => "Showing what uninstall would remove. Nothing will be changed.",
        (false, true) => {
            "Preparing to stop using Shader Guard for this game. The tool and saved shaders will be kept."
        }
        (false, false) => {
            "Showing how this game's settings would be restored. Nothing will be changed."
        }
    });
    if !apply {
        let state = load_state(paths)?;
        println!(
            "Plan: undo Shader Guard's launch-option changes for {} saved game settings{}. All saved shaders will be kept.",
            state
                .apps
                .iter()
                .filter(|a| id.is_none_or(|id| a.appid == id))
                .count(),
            if uninstall {
                " and remove the tool and its menu shortcut if they have not been changed"
            } else {
                ""
            }
        );
        println!("Nothing changed. Add --apply when you are ready.");
        return Ok(());
    }
    progress("Checking for Steam and game processes before changing settings.");
    require_idle()?;
    let _lock = lock(paths)?;
    let mut state = load_state(paths)?;
    let mut edits: BTreeMap<PathBuf, (String, String)> = BTreeMap::new();
    let mut restored = 0;
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
            restored += 1;
        } else if current.is_some_and(|v| v.contains("steam-shader-guard")) {
            return fail(
                "This game's launch options were changed after setup and still use Shader Guard. Remove 'steam-shader-guard run' from its Launch Options box in Steam, then try again.",
            );
        } else {
            println!("Kept your changed launch options for game {}.", entry.appid);
        }
    }
    for (path, (before, after)) in &edits {
        if fs::read_to_string(path)? != *before {
            return fail(
                "Steam's settings changed while this command was running. Fully exit Steam, then try again.",
            );
        }
        if before != after {
            progress(format_args!("Restoring game settings: {}", path.display()));
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
    println!("Game settings restored: {restored}.");
    if uninstall {
        progress("Checking that no known game or changed menu shortcut still needs Shader Guard.");
        // Includes manually edited/untracked app entries in every known account.
        // Also inspect accounts in the default Steam installation, if available.
        let mut configs = known_configs;
        let mut roots = state.steam_roots.clone();
        if let Some(steam_root) = find_root(paths, &SteamOptions::default())? {
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
                    return fail("A Steam launch option still uses Shader Guard. Remove 'steam-shader-guard run' from that game's Launch Options box before uninstalling.");
                }
            }
        }
        // Do not remove a binary still referenced by a changed managed desktop entry.
        for (path, record) in &state.files {
            if path != &paths.bin
                && path.try_exists()?
                && (path.is_symlink() || !record.matches_hash(&cache::digest_file(path)?))
                && fs::read_to_string(path)?.contains("steam-shader-guard")
            {
                return fail(
                    "A changed menu shortcut still uses Shader Guard. The tool was kept. Remove that shortcut before uninstalling.",
                );
            }
        }
        let files = std::mem::take(&mut state.files);
        let mut removed_files = 0;
        let mut restored_files = 0;
        let mut preserved_files = 0;
        for (path, record) in files {
            if !path.try_exists()? {
                continue;
            }
            if path.is_symlink() || !record.matches_hash(&cache::digest_file(&path)?) {
                println!(
                    "Kept a file changed outside Shader Guard: {}",
                    path.display()
                );
                preserved_files += 1;
                continue;
            }
            if let Some(before) = record.before {
                progress(format_args!("Restoring original file: {}", path.display()));
                atomic_write(&path, &before, record.before_mode)?;
                restored_files += 1;
            } else {
                progress(format_args!("Removing installed file: {}", path.display()));
                fs::remove_file(&path)?;
                removed_files += 1;
            }
        }
        atomic_json(&paths.state_file(), &state)?;
        println!(
            "Uninstall finished. Files removed: {removed_files}. Original files restored: {restored_files}. Changed files kept: {preserved_files}."
        );
    } else {
        println!(
            "Finished checking this game's settings. The tool and its Steam shortcut were kept."
        );
    }
    println!(
        "All original and copied shader files were kept, including shaders created while playing."
    );
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
    let steam_suffix = format!("steamapps/shadercache/{id}/nvidiav1");
    if !existing.is_empty() && !Path::new(existing).ends_with(&steam_suffix) {
        return Ok(out);
    }
    let directory = paths.app(id).join("nvidia");
    fs::create_dir_all(&directory)?;
    out.entry("__GL_SHADER_DISK_CACHE_SIZE".into())
        .or_insert("12000000000".into());
    out.insert(
        "__GL_SHADER_DISK_CACHE_PATH".into(),
        directory
            .to_str()
            .ok_or(
                "NVIDIA cannot use this shader folder because its path is not valid UTF-8 text.",
            )?
            .into(),
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
            return fail(
                "The list of copied shader files is invalid. The game was not started, so those files would not be used incorrectly.",
            );
        }
        out.insert("__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME".into(), value);
    } else {
        out.remove("__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME");
    }
    Ok(out)
}
pub fn run_game(paths: &Paths, args: Vec<OsString>) -> Result<()> {
    if args.is_empty() {
        return fail(
            "No game command was provided. Normally you should set up the game with enable, then launch it in Steam.",
        );
    }
    let id = ["SteamAppId", "SteamGameId", "STEAM_COMPAT_APP_ID"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .ok_or(
            "The game ID is missing. Launch this game from Steam after setting it up with enable.",
        )?;
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
    if let Some(path) = env.get("__GL_SHADER_DISK_CACHE_PATH") {
        progress(format_args!(
            "Starting game {id}. Saved shader folder: {path}"
        ));
    }
    progress(
        if env.contains_key("__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME") {
            "Copied shader files are available for the game to reuse. New shaders may still need to compile."
        } else {
            "No copied shader files are set up. The game may need to compile shaders as you play."
        },
    );
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
        .ok_or("The regular Linux Steam app was not found. Flatpak and Snap Steam cannot be used with this version.")?;
    progress("Starting Steam without NVIDIA shader pre-processing for this session.");
    progress(
        "Only games set up with enable will use Shader Guard's shader folders. Your usual Steam shortcuts are unchanged.",
    );
    Err(Command::new(path)
        .args(["+bShaderAllowReplayOnNVIDIA", "0"])
        .args(args)
        .exec()
        .into())
}
