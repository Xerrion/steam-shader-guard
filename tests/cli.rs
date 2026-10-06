use std::{
    fs,
    os::unix::fs::PermissionsExt,
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
fn cache_fixture(home: &Path) -> PathBuf {
    let source = home.join(".local/share/Steam/steamapps/shadercache/42/nvidiav1");
    let directory = source.join("GLCache/example");
    fs::create_dir_all(&directory).unwrap();
    let mut header = [0u8; 32];
    header[..4].copy_from_slice(b"CDVN");
    let mut bin = header.to_vec();
    let mut record = [0u8; 32];
    record[..4].copy_from_slice(&[0x9d, 0xa1, 0x46, 0x98]);
    record[4..20].fill(1);
    record[28..32].copy_from_slice(&8u32.to_le_bytes());
    bin.extend(record);
    bin.extend([7u8; 8]);
    let mut toc = header.to_vec();
    toc.extend([1u8; 16]);
    toc.extend(32u32.to_le_bytes());
    toc.extend(8u32.to_le_bytes());
    fs::write(directory.join("cache.bin"), bin).unwrap();
    fs::write(directory.join("cache.toc"), toc).unwrap();
    source
}

#[test]
fn explicit_json_reports_have_no_progress_on_stdout_or_side_effects() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    setup(&home);
    cache_fixture(&home);

    let doctor = run(&home, &["doctor", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(report["applications"].as_array().unwrap().len(), 2);
    assert_eq!(report["installed_launcher"], false);
    let messages = String::from_utf8(doctor.stderr).unwrap();
    assert!(messages.contains("Nothing will be changed."));
    assert!(messages.contains("Games found: 2. No files changed."));

    let scan = run(&home, &["scan", "42", "--json"]);
    let reports: Vec<serde_json::Value> = serde_json::from_slice(&scan.stdout).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["records"], 1);
    assert_eq!(reports[0]["wrapped_offsets"], 0);
    let messages = String::from_utf8(scan.stderr).unwrap();
    assert!(messages.contains("Checking saved shader file 1/1: GLCache/example/cache.toc"));
    assert!(messages.contains("Cache file pairs inspected: 1. Records: 1. Wrapped offsets: 0."));
    assert!(messages.contains("Check finished. No files changed."));
    assert!(!home.join(".local/state/steam-shader-guard").exists());
    assert!(!home.join(".local/share/steam-shader-guard").exists());
}

#[test]
fn inspection_defaults_to_readable_output_for_players() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    setup(&home);
    cache_fixture(&home);
    let doctor = run(&home, &["doctor"]);
    let text = String::from_utf8(doctor.stdout).unwrap();
    assert!(text.contains("Steam: found."));
    assert!(text.contains("Shader Guard: not installed yet."));
    assert!(text.contains("Game ID"));
    assert!(text.contains("Steam shader folder"));
    assert!(text.contains("42"));
    assert!(text.contains("Example Game"));
    assert!(text.contains("Custom Game"));
    assert!(text.contains("A shader folder does not mean the game is already using Shader Guard."));
    assert!(serde_json::from_str::<serde_json::Value>(&text).is_err());

    let scan = run(&home, &["scan", "42"]);
    let text = String::from_utf8(scan.stdout).unwrap();
    assert!(text.contains("Sets of saved shader files checked: 1."));
    assert!(text.contains("No broken file positions were found."));
    assert!(serde_json::from_str::<serde_json::Value>(&text).is_err());

    let recover = run(&home, &["recover", "42"]);
    let text = String::from_utf8(recover.stdout).unwrap();
    assert!(text.contains("No broken file positions were found."));
    let messages = String::from_utf8(recover.stderr).unwrap();
    assert!(messages.contains("Nothing will be changed."));
    assert!(messages.contains("Add --apply to copy"));
    assert!(!home.join(".local/share/steam-shader-guard").exists());
}

#[test]
fn normal_shader_copy_reports_completion_without_a_json_dump() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    let original = fs::read(&config).unwrap();
    cache_fixture(&home);
    let copied = run(&home, &["recover", "42", "--apply"]);
    assert!(copied.stdout.is_empty());
    let messages = String::from_utf8(copied.stderr).unwrap();
    assert!(messages.contains("Preparing to copy saved shaders."));
    assert!(messages.contains("Shader copy finished and checked."));
    assert!(messages.contains("The game's settings have not changed."));
    assert!(messages.contains("Next: enable 42 --apply"));
    assert!(!messages.contains("wrapped_offsets"));
    assert!(!messages.contains("seed caches"));
    assert_eq!(fs::read(config).unwrap(), original);
    assert!(
        home.join(".local/share/steam-shader-guard/games/42/nvidia")
            .is_dir()
    );
}

#[test]
fn recovery_messages_distinguish_preview_copy_verification_and_completion() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    let original_config = fs::read(&config).unwrap();
    let source = cache_fixture(&home);
    let original_bin = fs::read(source.join("GLCache/example/cache.bin")).unwrap();
    let original_toc = fs::read(source.join("GLCache/example/cache.toc")).unwrap();
    let destination = home.join(".local/share/steam-shader-guard/games/42/nvidia");

    let preview = run(&home, &["recover", "42", "--json"]);
    let reports: Vec<serde_json::Value> = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(reports.len(), 1);
    let messages = String::from_utf8(preview.stderr).unwrap();
    assert!(messages.contains("Showing the shader copy plan. Nothing will be changed."));
    assert!(messages.contains("Reading from:"));
    assert!(messages.contains("Shader Guard will save its copy in:"));
    assert!(messages.contains("recover 42 --apply."));
    assert!(!messages.contains("Shader copy finished"));
    assert!(!destination.exists());
    assert!(!home.join(".local/state/steam-shader-guard").exists());

    let applied = run(&home, &["recover", "42", "--apply", "--json"]);
    let copied: Vec<serde_json::Value> = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(copied, reports);
    let messages = String::from_utf8(applied.stderr).unwrap();
    let stages = [
        "Checking that the saved shader files can be copied safely.",
        "Checking free space:",
        "Copying saved shader file 1/1:",
        "Copying part 1 of this file.",
        "Checking the copied file and its shader locations:",
        "Checking that the original shader files did not change while copying.",
        "Saving the checked shader copy:",
        "Shader copy finished",
        "Next: enable 42 --apply",
    ];
    let positions: Vec<_> = stages
        .iter()
        .map(|stage| messages.find(stage).expect(stage))
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(destination.is_dir());
    assert_eq!(fs::read(&config).unwrap(), original_config);
    assert_eq!(
        fs::read(source.join("GLCache/example/cache.bin")).unwrap(),
        original_bin
    );
    assert_eq!(
        fs::read(source.join("GLCache/example/cache.toc")).unwrap(),
        original_toc
    );

    let failed = command(&home)
        .args(["recover", "42", "--apply"])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    let messages = String::from_utf8(failed.stderr).unwrap();
    assert!(messages.contains("already has a Shader Guard folder"));
    assert!(messages.contains("Reuse them or move that folder to a backup"));
    assert!(!messages.contains("Shader copy finished"));
}

#[test]
fn failed_inspection_never_reports_completion() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    setup(&home);
    let source = cache_fixture(&home);
    fs::write(source.join("GLCache/example/cache.toc"), "invalid").unwrap();
    let output = command(&home).args(["scan", "42"]).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let messages = String::from_utf8(output.stderr).unwrap();
    assert!(messages.contains("Checking saved shader file 1/1:"));
    assert!(messages.contains("A shader index file is incomplete"));
    assert!(messages.contains("The original file was kept."));
    assert!(!messages.contains("Check finished."));
}

#[test]
fn command_help_explains_separate_setup_steps() {
    let t = tempfile::tempdir().unwrap();
    let install = run(t.path(), &["install", "--help"]);
    let text = String::from_utf8(install.stdout).unwrap();
    assert!(text.contains("This installs the tool only."));
    assert!(text.contains("It does not set up games, copy shaders or start Steam."));
    assert!(text.contains("Next: run doctor to find your game's ID"));
    let enable = run(t.path(), &["enable", "--help"]);
    let text = String::from_utf8(enable.stdout).unwrap();
    assert!(text.contains("Run install --apply first."));
    assert!(text.contains("Games with existing launch options are left unchanged."));
    assert!(text.contains("This does not copy existing shaders."));
}
#[test]
fn help_and_version_succeed_without_valid_home_or_side_effects() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    for args in [
        vec![],
        vec!["--help"],
        vec!["-h"],
        vec!["--version"],
        vec!["doctor", "--help"],
        vec!["scan", "--help"],
        vec!["recover", "--help"],
        vec!["install", "-h"],
        vec!["enable", "--help"],
        vec!["disable", "--help"],
        vec!["uninstall", "--help"],
    ] {
        let output = command(&home)
            .env("SHADER_GUARD_HOME", "relative-home")
            .env("XDG_DATA_HOME", "relative-data")
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        if args == ["--version"] {
            assert_eq!(
                text,
                format!("steam-shader-guard {}\n", env!("CARGO_PKG_VERSION"))
            );
        } else {
            assert!(text.contains("Usage:"), "{args:?}: {text}");
        }
        assert!(!home.exists(), "{args:?}");
    }
}

#[test]
fn argument_errors_report_diagnostics_before_reading_environment() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    for args in [
        vec!["unknown"],
        vec!["scan"],
        vec!["scan", "0"],
        vec!["enable", "42", "--all"],
        vec!["install", "--apply", "--apply"],
        vec!["doctor", "--unknown"],
        vec!["run", "--"],
    ] {
        let output = command(&home)
            .env("SHADER_GUARD_HOME", "relative-home")
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("error:"), "{args:?}: {error}");
        assert!(
            error.contains("Usage:") || error.contains("For more information, try '--help'."),
            "{args:?}: {error}"
        );
        assert!(!error.contains("Home directory"), "{args:?}: {error}");
        assert!(!home.exists(), "{args:?}");
    }
}

#[test]
fn preview_has_no_side_effects_and_unknown_flags_fail() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    fs::create_dir(&home).unwrap();
    let preview = run(&home, &["install"]);
    let output = String::from_utf8(preview.stdout).unwrap();
    assert!(output.contains("It does not set up games, copy shaders or start Steam."));
    assert!(output.contains("Nothing changed. Add --apply"));
    assert!(
        String::from_utf8(preview.stderr)
            .unwrap()
            .contains("Nothing will be changed.")
    );
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
    let install = run(&home, &["install", "--apply"]);
    let output = String::from_utf8(install.stdout).unwrap();
    assert!(output.contains("Shader Guard is installed. Your game settings have not changed."));
    assert!(output.contains("Then run enable GAME_ID --apply"));
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    run(&home, &["enable", "--all"]);
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    let enable = run(&home, &["enable", "--all", "--apply"]);
    let output = String::from_utf8(enable.stdout).unwrap();
    assert!(output.contains("Games set up: 1."));
    assert!(
        output.contains("Already set up: 0. Games with other launch options left unchanged: 1.")
    );
    assert!(output.contains("This command did not copy existing shaders."));
    assert!(
        String::from_utf8(enable.stderr)
            .unwrap()
            .contains("Saving game settings:")
    );
    let updated = fs::read_to_string(&config).unwrap();
    assert!(updated.contains("run -- %command%"));
    assert!(updated.contains("gamemoderun %command%"));
    assert!(updated.contains("\"Unrelated\" \"keep me\""));
    let disable = run(&home, &["disable", "42", "--apply"]);
    let output = String::from_utf8(disable.stdout).unwrap();
    assert!(output.contains("Game settings restored: 1."));
    assert!(output.contains("The tool and its Steam shortcut were kept."));
    assert!(
        !fs::read_to_string(&config)
            .unwrap()
            .contains("steam-shader-guard")
    );
    let cache = home.join(".local/share/steam-shader-guard/games/42/nvidia");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("keep"), "shader").unwrap();
    let uninstall = run(&home, &["uninstall", "--apply"]);
    let output = String::from_utf8(uninstall.stdout).unwrap();
    assert!(
        output.contains("Files removed: 2. Original files restored: 0. Changed files kept: 0.")
    );
    assert!(output.contains("All original and copied shader files were kept"));
    assert!(!home.join(".local/bin/steam-shader-guard").exists());
    assert_eq!(fs::read_to_string(cache.join("keep")).unwrap(), "shader");
}
#[test]
fn skipped_games_are_not_reported_as_connected() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "--all", "--apply"]);
    let original = fs::read(&config).unwrap();
    let enable = run(&home, &["enable", "--all", "--apply"]);
    let output = String::from_utf8(enable.stdout).unwrap();
    assert!(output.contains(
        "Setup plan: 0 to set up, 1 already set up, 1 with other launch options left unchanged."
    ));
    assert!(output.contains("No new games were set up."));
    assert!(output.contains("Already has Shader Guard launch options: Example Game (game ID 42)."));
    assert!(!output.contains("Skipped Example Game"));
    assert!(output.contains("follow the launch-option instructions above"));
    assert!(!output.contains("Games set up: 2."));
    assert_eq!(fs::read(&config).unwrap(), original);
}
#[test]
fn already_set_up_game_does_not_prompt_for_duplicate_launch_options() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "42", "--apply"]);
    let original = fs::read(&config).unwrap();
    for arguments in [&["enable", "42"][..], &["enable", "42", "--apply"][..]] {
        let result = run(&home, arguments);
        let output = String::from_utf8(result.stdout).unwrap();
        assert!(
            output.contains("Already has Shader Guard launch options: Example Game (game ID 42).")
        );
        assert!(output.contains(
            "Setup plan: 0 to set up, 1 already set up, 0 with other launch options left unchanged."
        ));
        assert!(!output.contains("To add Shader Guard yourself"));
        assert!(!output.contains("follow the launch-option instructions above"));
        assert_eq!(fs::read(&config).unwrap(), original);
    }
}
#[test]
fn setup_and_undo_previews_explain_that_nothing_will_change() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "42", "--apply"]);
    let original = fs::read(&config).unwrap();
    let state_path = home.join(".local/state/steam-shader-guard/state.json");
    let state = fs::read(&state_path).unwrap();
    for args in [
        vec!["enable", "--all"],
        vec!["disable", "42"],
        vec!["uninstall"],
    ] {
        let preview = run(&home, &args);
        assert!(
            String::from_utf8(preview.stdout)
                .unwrap()
                .contains("Nothing changed.")
        );
        assert!(
            String::from_utf8(preview.stderr)
                .unwrap()
                .contains("Nothing will be changed.")
        );
        assert_eq!(fs::read(&config).unwrap(), original);
        assert_eq!(fs::read(&state_path).unwrap(), state);
        assert!(home.join(".local/bin/steam-shader-guard").is_file());
    }
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
            "--version",
            "--",
            "",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(7));
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        "one two\nliteral $;\n--help\n--version\n--\n\n"
    );
    let messages = String::from_utf8(o.stderr).unwrap();
    assert!(messages.contains("Starting game 42. Saved shader folder:"));
    assert!(messages.contains("No copied shader files are set up."));
    assert!(!messages.contains("literal $;"));
    assert!(
        home.join(".local/share/steam-shader-guard/games/42/nvidia")
            .is_dir()
    );
}
#[test]
fn wrapper_reports_seed_use_and_preserves_custom_cache_paths() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let cache = home.join(".local/share/steam-shader-guard/games/42/nvidia");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.parent().unwrap().join("readonly-names.txt"),
        "sg_seed\n",
    )
    .unwrap();
    let args = [
        "run",
        "--",
        "/bin/sh",
        "-c",
        "printf '%s\\n' \"$__GL_SHADER_DISK_CACHE_PATH\" \"$__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME\"",
    ];
    let seeded = command(&home)
        .env("SteamAppId", "42")
        .args(args)
        .output()
        .unwrap();
    assert!(seeded.status.success());
    assert_eq!(
        String::from_utf8(seeded.stdout).unwrap(),
        format!("{}\nsg_seed\n", cache.display())
    );
    assert!(
        String::from_utf8(seeded.stderr)
            .unwrap()
            .contains("Copied shader files are available for the game to reuse.")
    );

    let custom = home.join("custom NVIDIA cache");
    let overridden = command(&home)
        .env("SteamAppId", "42")
        .env("__GL_SHADER_DISK_CACHE_PATH", &custom)
        .env("__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME", "custom_seed")
        .args(args)
        .output()
        .unwrap();
    assert!(overridden.status.success());
    assert_eq!(
        String::from_utf8(overridden.stdout).unwrap(),
        format!("{}\ncustom_seed\n", custom.display())
    );
    let messages = String::from_utf8(overridden.stderr).unwrap();
    assert!(messages.contains(&format!("Saved shader folder: {}", custom.display())));
    assert!(!messages.contains(&format!("Saved shader folder: {}", cache.display())));
}
#[test]
fn untracked_manual_launch_reference_blocks_binary_removal() {
    for account in ["100", "0"] {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        setup(&home);
        let accounts = home.join(".local/share/Steam/userdata");
        if account != "100" {
            fs::rename(accounts.join("100"), accounts.join(account)).unwrap();
        }
        fs::write(accounts.join("unrelated-file"), "keep").unwrap();
        let config = accounts.join(account).join("config/localconfig.vdf");
        run(&home, &["install", "--apply"]);
        let s = fs::read_to_string(&config).unwrap().replace(
            "gamemoderun %command%",
            "steam-shader-guard run -- %command%",
        );
        fs::write(config, s).unwrap();
        let output = command(&home)
            .args(["uninstall", "--apply"])
            .output()
            .unwrap();
        assert!(
            home.join(".local/bin/steam-shader-guard").exists(),
            "Account {account}"
        );
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("still uses Shader Guard"));
    }
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
fn uninstall_retains_program_when_known_accounts_cannot_be_inspected() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = setup(&home);
    run(&home, &["install", "--apply"]);
    run(&home, &["enable", "42", "--apply"]);
    let original = fs::read(&config).unwrap();
    let accounts = home.join(".local/share/Steam/userdata");
    let permissions = fs::metadata(&accounts).unwrap().permissions();
    fs::set_permissions(&accounts, fs::Permissions::from_mode(0o000)).unwrap();
    let output = command(&home).args(["uninstall", "--apply"]).output();
    fs::set_permissions(&accounts, permissions).unwrap();
    let output = output.unwrap();
    assert!(
        home.join(".local/bin/steam-shader-guard").is_file(),
        "Removed program without inspecting the inaccessible account"
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .to_lowercase()
            .contains("permission denied")
    );
    assert_eq!(fs::read(&config).unwrap(), original);
    run(&home, &["uninstall", "--apply"]);
}

#[test]
fn failed_install_update_can_be_retried() {
    use sha2::{Digest, Sha256};

    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    run(&home, &["install", "--apply"]);
    let binary = home.join(".local/bin/steam-shader-guard");
    let old = b"previous managed executable";
    fs::write(&binary, old).unwrap();
    let state_path = home.join(".local/state/steam-shader-guard/state.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["files"][binary.to_str().unwrap()]["after_hash"] =
        format!("{:x}", Sha256::digest(old)).into();
    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    let directory = binary.parent().unwrap();
    let permissions = fs::metadata(directory).unwrap().permissions();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o555)).unwrap();
    let output = command(&home).args(["install", "--apply"]).output();
    fs::set_permissions(directory, permissions).unwrap();
    assert!(!output.unwrap().status.success());
    assert_eq!(fs::read(&binary).unwrap(), old);
    run(&home, &["install", "--apply"]);
    assert_eq!(fs::read(binary).unwrap(), fs::read(BIN).unwrap());
    run(&home, &["uninstall", "--apply"]);
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

#[test]
fn uninstall_refuses_inaccessible_untracked_accounts_and_menu_entries() {
    for relative in [
        ".local/share/Steam",
        ".local/share/Steam/userdata",
        ".local/share/Steam/userdata/100/config",
        ".local/share/applications",
    ] {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let config = setup(&home);
        run(&home, &["install", "--apply"]);
        let text = fs::read_to_string(&config).unwrap().replace(
            "gamemoderun %command%",
            "steam-shader-guard run -- %command%",
        );
        if relative.contains("Steam") {
            fs::write(&config, text).unwrap();
        }
        let directory = home.join(relative);
        let permissions = fs::metadata(&directory).unwrap().permissions();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o000)).unwrap();
        let output = command(&home).args(["uninstall", "--apply"]).output();
        fs::set_permissions(&directory, permissions).unwrap();
        assert!(
            home.join(".local/bin/steam-shader-guard").is_file(),
            "Removed program without inspecting inaccessible {relative}"
        );
        assert!(!output.unwrap().status.success(), "{relative}");
    }
}

#[test]
fn published_pending_install_can_be_retried_or_uninstalled() {
    for action in ["install", "uninstall"] {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        run(&home, &["install", "--apply"]);
        let binary = home.join(".local/bin/steam-shader-guard");
        let state_path = home.join(".local/state/steam-shader-guard/state.json");
        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        let entry = &mut state["files"][binary.to_str().unwrap()];
        entry["pending_hash"] = entry["after_hash"].clone();
        entry["after_hash"] = "previous version hash".into();
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        run(&home, &[action, "--apply"]);
        if action == "install" {
            let state: serde_json::Value =
                serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
            assert!(state["files"][binary.to_str().unwrap()]["pending_hash"].is_null());
            assert_eq!(fs::read(binary).unwrap(), fs::read(BIN).unwrap());
        } else {
            assert!(!binary.exists());
        }
    }
}

#[test]
fn symlinked_menu_entry_blocks_removal_of_program_it_still_references() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    run(&home, &["install", "--apply"]);
    let desktop = home.join(".local/share/applications/steam-shader-guard.desktop");
    let moved = home.join("custom.desktop");
    fs::rename(&desktop, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &desktop).unwrap();
    let output = command(&home)
        .args(["uninstall", "--apply"])
        .output()
        .unwrap();
    assert!(
        home.join(".local/bin/steam-shader-guard").is_file(),
        "Removed program but preserved a symlinked menu entry that still launches it"
    );
    assert!(!output.status.success());
    assert!(desktop.is_symlink());
}
