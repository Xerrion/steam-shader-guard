use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
const BIN: &str = env!("CARGO_BIN_EXE_steam-shader-guard");
fn command(home: &Path) -> Command {
    let mut c = Command::new(BIN);
    c.env("SHADER_GUARD_HOME", home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"));
    c
}
fn run(home: &Path, args: &[&str]) -> Output {
    let o = command(home).args(args).output().unwrap();
    assert!(
        o.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&o.stderr)
    );
    o
}
fn setup(home: &Path) -> PathBuf {
    let root = home.join(".local/share/Steam");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    for (id, name) in [("42", "Example Game"), ("43", "Custom Game")] {
        fs::write(
            root.join(format!("steamapps/appmanifest_{id}.acf")),
            format!("\"AppState\" {{ \"appid\" \"{id}\" \"name\" \"{name}\" }}"),
        )
        .unwrap();
    }
    let file = root.join("userdata/100/config/localconfig.vdf");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file,"// Preserve this comment\n\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" { \"42\" { \"Unrelated\" \"keep me\" } \"43\" { \"LaunchOptions\" \"gamemoderun %command%\" } } } } } }").unwrap();
    file
}
#[test]
fn preview_has_no_side_effects_and_unknown_flags_fail() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    fs::create_dir(&home).unwrap();
    run(&home, &["install"]);
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    assert!(
        !command(&home)
            .args(["install", "--unknown"])
            .status()
            .unwrap()
            .success()
    );
}
#[test]
fn install_enable_disable_uninstall_preserves_custom_launch_options() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home with spaces");
    let config = setup(&home);
    let original = fs::read_to_string(&config).unwrap();
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "--all"]);
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    run(&home, &["enable", "--all", "--apply"]);
    let updated = fs::read_to_string(&config).unwrap();
    assert!(updated.contains("run -- %command%"));
    assert!(updated.contains("gamemoderun %command%"));
    assert!(updated.contains("\"Unrelated\" \"keep me\""));
    run(&home, &["disable", "42", "--apply"]);
    assert!(
        !fs::read_to_string(&config)
            .unwrap()
            .contains("steam-shader-guard")
    );
    let cache = home.join(".local/share/steam-shader-guard/games/42/nvidia");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("keep"), "shader").unwrap();
    run(&home, &["uninstall", "--apply"]);
    assert!(!home.join(".local/bin/steam-shader-guard").exists());
    assert_eq!(fs::read_to_string(cache.join("keep")).unwrap(), "shader");
}
#[test]
fn modified_launch_option_blocks_uninstall_and_keeps_binary() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "42", "--apply"]);
    let s = fs::read_to_string(&config)
        .unwrap()
        .replace("run -- %command%", "run -- %command% -custom");
    fs::write(&config, &s).unwrap();
    assert!(
        !command(&home)
            .args(["uninstall", "--apply"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(config).unwrap(), s);
    assert!(home.join(".local/bin/steam-shader-guard").exists());
}
#[test]
fn wrapper_preserves_game_arguments_help_and_exit_status() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    fs::create_dir(&home).unwrap();
    let o = command(&home)
        .env("SteamAppId", "42")
        .args([
            "run",
            "--",
            "/bin/sh",
            "-c",
            "printf '%s\\n' \"$@\"; exit 7",
            "game",
            "one two",
            "literal $;",
            "--help",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(7));
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        "one two\nliteral $;\n--help\n"
    );
    assert!(
        home.join(".local/share/steam-shader-guard/games/42/nvidia")
            .is_dir()
    );
}
#[test]
fn untracked_manual_launch_reference_blocks_binary_removal() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    let s = fs::read_to_string(&config).unwrap().replace(
        "gamemoderun %command%",
        "steam-shader-guard run -- %command%",
    );
    fs::write(config, s).unwrap();
    assert!(
        !command(&home)
            .args(["uninstall", "--apply"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(home.join(".local/bin/steam-shader-guard").exists());
}

#[test]
fn custom_steam_root_remains_known_after_disabling_a_game() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    setup(&home);
    let custom = home.join("custom-steam");
    fs::rename(home.join(".local/share/Steam"), &custom).unwrap();
    run(&home, &["install", "--apply"]);
    run(
        &home,
        &[
            "enable",
            "42",
            "--steam-root",
            custom.to_str().unwrap(),
            "--apply",
        ],
    );
    run(&home, &["disable", "42", "--apply"]);
    let config = custom.join("userdata/100/config/localconfig.vdf");
    let s = fs::read_to_string(&config).unwrap().replace(
        "gamemoderun %command%",
        "steam-shader-guard run -- %command%",
    );
    fs::write(config, s).unwrap();
    assert!(
        !command(&home)
            .args(["uninstall", "--apply"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(home.join(".local/bin/steam-shader-guard").exists());
}

#[test]
fn invalid_command_arguments_never_change_installation_or_launch_options() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "42", "--apply"]);
    let original = fs::read(&config).unwrap();
    let state_path = home.join(".local/state/steam-shader-guard/state.json");
    let state = fs::read(&state_path).unwrap();
    for args in [
        vec!["uninstall", "42", "--apply"],
        vec!["disable", "42", "--account", "999", "--apply"],
        vec!["disable", "42", "--all", "--apply"],
        vec!["uninstall", "--steam-root", "/missing", "--apply"],
        vec!["install", "--all", "--apply"],
        vec!["install", "42", "--apply"],
        vec!["install", "--apply", "--apply"],
        vec!["enable", "42", "--source", "/missing", "--apply"],
        vec!["enable", "42", "--account", "100", "--account", "999"],
        vec!["doctor", "--apply"],
    ] {
        let output = command(&home).args(&args).output().unwrap();
        assert!(
            home.join(".local/bin/steam-shader-guard").is_file(),
            "Invalid arguments removed the installed program: {args:?}"
        );
        assert!(!output.status.success(), "Unexpectedly accepted {args:?}");
        assert_eq!(fs::read(&config).unwrap(), original, "{args:?}");
        assert_eq!(fs::read(&state_path).unwrap(), state, "{args:?}");
    }
}

#[test]
fn enable_and_disable_support_case_variant_steam_keys() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    let text = fs::read_to_string(&config)
        .unwrap()
        .replace("UserLocalConfigStore", "userlocalconfigstore")
        .replace("Software", "software")
        .replace("Valve", "valve")
        .replace("Steam", "steam")
        .replace("LaunchOptions", "launchoptions");
    fs::write(&config, text).unwrap();
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "--all", "--apply"]);
    let changed = fs::read_to_string(&config).unwrap();
    assert!(changed.contains("run -- %command%"));
    assert!(changed.contains("\"launchoptions\" \"gamemoderun %command%\""));
    run(&home, &["disable", "42", "--apply"]);
    assert!(
        !fs::read_to_string(config)
            .unwrap()
            .contains("steam-shader-guard")
    );
}

#[test]
fn case_variant_manual_launch_reference_blocks_uninstall() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    let text = fs::read_to_string(&config)
        .unwrap()
        .replace("LaunchOptions", "LAUNCHOPTIONS")
        .replace(
            "gamemoderun %command%",
            "steam-shader-guard run -- %command%",
        );
    fs::write(&config, &text).unwrap();
    let output = command(&home)
        .args(["uninstall", "--apply"])
        .output()
        .unwrap();
    assert!(
        home.join(".local/bin/steam-shader-guard").is_file(),
        "Removed program despite a case-variant launch option referencing it"
    );
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(config).unwrap(), text);
}

#[test]
fn empty_xdg_variables_use_default_directories() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let output = command(&home)
        .env("XDG_DATA_HOME", "")
        .env("XDG_STATE_HOME", "")
        .args(["install", "--apply"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        home.join(".local/share/applications/steam-shader-guard.desktop")
            .is_file()
    );
    assert!(
        home.join(".local/state/steam-shader-guard/state.json")
            .is_file()
    );
}
