use crate::valid_id;
use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about,
    disable_help_subcommand = true,
    after_help = "Changes are previewed unless --apply is present. Originals and generated cache\n\
                  data are retained. No network access, telemetry or elevated privileges."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Inspect the native Steam installation and per-game cache status
    Doctor(SteamOptions),
    /// Inspect a game's NVIDIA cache
    Scan(CacheOptions),
    /// Recover intact cache records into a separate verified copy
    Recover {
        #[command(flatten)]
        cache: CacheOptions,
        /// Write changes instead of showing a preview
        #[arg(long)]
        apply: bool,
    },
    /// Install the program and the Steam (Shader Guard) menu entry
    Install {
        /// Write changes instead of showing a preview
        #[arg(long)]
        apply: bool,
    },
    /// Connect one game or all installed games to Shader Guard
    Enable(EnableOptions),
    /// Restore a game's tracked launch options across accounts
    Disable {
        #[arg(value_name = "APPID", value_parser = parse_id)]
        id: String,
        /// Write changes instead of showing a preview
        #[arg(long)]
        apply: bool,
    },
    /// Restore tracked settings and remove the installed program
    Uninstall {
        /// Write changes instead of showing a preview
        #[arg(long)]
        apply: bool,
    },
    /// Launch a game with its private NVIDIA cache. Use run -- GAME [ARGUMENTS...]
    #[command(disable_help_flag = true)]
    Run {
        #[arg(
            value_name = "GAME",
            num_args = 1..,
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        command: Vec<OsString>,
    },
    /// Launch native Steam without NVIDIA replay. All arguments go to Steam
    #[command(disable_help_flag = true)]
    Steam {
        #[arg(
            value_name = "STEAM_ARGUMENTS",
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        arguments: Vec<OsString>,
    },
}

#[derive(Args, Default)]
pub struct SteamOptions {
    /// Path to a nonstandard native Steam installation
    #[arg(long = "steam-root", value_name = "PATH")]
    pub root: Option<PathBuf>,
}

#[derive(Args)]
pub struct CacheOptions {
    #[arg(value_name = "APPID", value_parser = parse_id)]
    pub id: String,
    /// NVIDIA cache tree to inspect instead of the game's live cache
    #[arg(long, value_name = "PATH")]
    pub source: Option<PathBuf>,
    #[command(flatten)]
    pub steam: SteamOptions,
}

#[derive(Args)]
#[group(required = true, multiple = false, args = ["id", "all"])]
pub struct EnableOptions {
    #[arg(value_name = "APPID", value_parser = parse_id)]
    pub id: Option<String>,
    /// Select all installed games except Steam runtime and Proton tools
    #[arg(long)]
    pub all: bool,
    /// Numeric account directory under Steam's userdata folder
    #[arg(long, value_name = "ID", value_parser = parse_id)]
    pub account: Option<String>,
    #[command(flatten)]
    pub steam: SteamOptions,
    /// Write changes instead of showing a preview
    #[arg(long)]
    pub apply: bool,
}

fn parse_id(id: &str) -> std::result::Result<String, String> {
    valid_id(id).map_err(|error| error.to_string())?;
    Ok(id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, error::ErrorKind};
    use std::os::unix::ffi::OsStringExt;

    fn parse(args: &[&str]) -> std::result::Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("steam-shader-guard").chain(args.iter().copied()))
    }

    #[test]
    fn command_schema_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn command_metadata_comes_from_cargo() {
        let command = Cli::command();
        assert_eq!(command.get_name(), env!("CARGO_PKG_NAME"));
        assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(
            command.get_about().map(|about| about.to_string()),
            Some(env!("CARGO_PKG_DESCRIPTION").to_owned())
        );
    }

    #[test]
    fn accepts_existing_commands_and_command_specific_options() {
        for args in [
            vec!["doctor", "--steam-root", "/Steam"],
            vec!["scan", "42", "--source", "/cache", "--steam-root", "/Steam"],
            vec![
                "recover",
                "42",
                "--source",
                "/cache",
                "--steam-root",
                "/Steam",
                "--apply",
            ],
            vec!["install", "--apply"],
            vec![
                "enable",
                "42",
                "--account",
                "100",
                "--steam-root",
                "/Steam",
                "--apply",
            ],
            vec!["enable", "--all", "--account", "100", "--apply"],
            vec!["disable", "42", "--apply"],
            vec!["uninstall", "--apply"],
            vec!["run", "--", "/game", "--help"],
            vec!["run", "/game", "--version"],
            vec!["steam"],
            vec!["steam", "-silent", "--help"],
        ] {
            assert!(parse(&args).is_ok(), "Rejected {args:?}");
        }
    }

    #[test]
    fn paths_ids_and_apply_values_reach_the_command() {
        let Some(Command::Recover { cache, apply }) = parse(&[
            "recover",
            "--source=/cache with spaces",
            "--steam-root=/Steam",
            "00042",
            "--apply",
        ])
        .unwrap()
        .command
        else {
            panic!("Expected recovery command");
        };
        assert_eq!(cache.id, "00042");
        assert_eq!(cache.source, Some(PathBuf::from("/cache with spaces")));
        assert_eq!(cache.steam.root, Some(PathBuf::from("/Steam")));
        assert!(apply);

        let Some(Command::Enable(options)) =
            parse(&["enable", "--all", "--account=100", "--apply"])
                .unwrap()
                .command
        else {
            panic!("Expected enable command");
        };
        assert!(options.all);
        assert!(options.id.is_none());
        assert_eq!(options.account.as_deref(), Some("100"));
        assert!(options.apply);
    }

    #[test]
    fn mutations_default_to_preview() {
        for args in [
            vec!["recover", "42"],
            vec!["install"],
            vec!["enable", "42"],
            vec!["enable", "--all"],
            vec!["disable", "42"],
            vec!["uninstall"],
        ] {
            let apply = match parse(&args).unwrap().command.unwrap() {
                Command::Recover { apply, .. }
                | Command::Install { apply }
                | Command::Disable { apply, .. }
                | Command::Uninstall { apply } => apply,
                Command::Enable(options) => options.apply,
                _ => panic!("Expected mutation command"),
            };
            assert!(!apply, "{args:?}");
        }
    }

    #[test]
    fn rejects_missing_conflicting_duplicate_and_unsupported_arguments() {
        for args in [
            vec!["unknown"],
            vec!["doctor", "--apply"],
            vec!["doctor", "--steam-root"],
            vec!["doctor", "--steam-root", "/Steam", "--steam-root", "/Steam"],
            vec!["scan"],
            vec!["scan", "42", "--apply"],
            vec!["scan", "42", "--source"],
            vec!["scan", "42", "--source", "--apply"],
            vec!["recover"],
            vec!["recover", "42", "--source", "/cache", "--source", "/cache"],
            vec!["recover", "42", "--apply", "--apply"],
            vec!["install", "42"],
            vec!["install", "--all"],
            vec!["install", "--unknown"],
            vec!["install", "--apply", "--apply"],
            vec!["enable"],
            vec!["enable", "--account", "100"],
            vec!["enable", "42", "--all"],
            vec!["enable", "42", "43"],
            vec!["enable", "--all", "--all"],
            vec!["enable", "42", "--account"],
            vec!["enable", "42", "--account", "100", "--account", "100"],
            vec!["enable", "42", "--source", "/cache"],
            vec!["disable"],
            vec!["disable", "42", "--account", "100"],
            vec!["uninstall", "42"],
            vec!["uninstall", "--steam-root", "/Steam"],
            vec!["run"],
            vec!["run", "--"],
        ] {
            assert!(parse(&args).is_err(), "Accepted {args:?}");
        }
    }

    #[test]
    fn app_and_account_ids_use_existing_decimal_validation() {
        for id in [
            "",
            "0",
            "000",
            "-1",
            "abc",
            "../42",
            "18446744073709551616",
            "000000000000000000042",
        ] {
            for command in ["scan", "recover", "enable", "disable"] {
                assert!(parse(&[command, id]).is_err(), "{command} {id:?}");
            }
            assert!(parse(&["enable", "--all", "--account", id]).is_err());
        }
        assert!(parse(&["scan", "18446744073709551615"]).is_ok());
    }

    #[test]
    fn help_and_version_are_clap_display_results() {
        assert!(parse(&[]).unwrap().command.is_none());
        for args in [
            vec!["--help"],
            vec!["-h"],
            vec!["doctor", "-h"],
            vec!["scan", "--help"],
            vec!["recover", "--help"],
            vec!["install", "--help"],
            vec!["enable", "--help"],
            vec!["disable", "--help"],
            vec!["uninstall", "--help"],
        ] {
            assert_eq!(parse(&args).err().unwrap().kind(), ErrorKind::DisplayHelp);
        }
        assert_eq!(
            parse(&["--version"]).err().unwrap().kind(),
            ErrorKind::DisplayVersion
        );
    }

    #[test]
    fn forwards_os_arguments_without_interpreting_game_or_steam_flags() {
        let arguments = vec![
            OsString::from("-silent"),
            OsString::from("one two"),
            OsString::from(""),
            OsString::from("--help"),
            OsString::from("--version"),
            OsString::from("--"),
            OsString::from_vec(b"non-utf8-\xff".to_vec()),
        ];
        for name in ["run", "steam"] {
            let mut argv = vec!["steam-shader-guard".into(), name.into()];
            if name == "run" {
                argv.extend([OsString::from("--"), OsString::from("/game")]);
            }
            argv.extend(arguments.clone());
            match Cli::try_parse_from(argv).unwrap().command.unwrap() {
                Command::Run { command } => {
                    assert_eq!(command[0], "/game");
                    assert_eq!(command[1..], arguments);
                }
                Command::Steam { arguments: parsed } => assert_eq!(parsed, arguments),
                _ => panic!("Expected forwarding command"),
            }
        }
    }

    #[test]
    fn preserves_non_utf8_paths_and_consumes_only_the_cli_separator() {
        let path = OsString::from_vec(b"/cache-\xff".to_vec());
        let Some(Command::Scan(options)) = Cli::try_parse_from([
            OsString::from("steam-shader-guard"),
            OsString::from("scan"),
            OsString::from("42"),
            OsString::from("--source"),
            path.clone(),
            OsString::from("--steam-root"),
            path.clone(),
        ])
        .unwrap()
        .command
        else {
            panic!("Expected scan command");
        };
        assert_eq!(options.source, Some(PathBuf::from(path.clone())));
        assert_eq!(options.steam.root, Some(PathBuf::from(path)));

        let Some(Command::Run { command }) =
            parse(&["run", "--", "--", "argument"]).unwrap().command
        else {
            panic!("Expected forwarding command");
        };
        assert_eq!(
            command,
            vec![OsString::from("--"), OsString::from("argument")]
        );

        let Some(Command::Steam { arguments }) = parse(&["steam", "--", "--help", "--", "-silent"])
            .unwrap()
            .command
        else {
            panic!("Expected Steam command");
        };
        assert_eq!(
            arguments,
            vec![
                OsString::from("--help"),
                OsString::from("--"),
                OsString::from("-silent"),
            ]
        );
    }
}
