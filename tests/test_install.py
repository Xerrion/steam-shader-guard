import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


INSTALLER = Path(__file__).resolve().parents[1] / "scripts" / "install.sh"
REPOSITORY = "https://github.com/Xerrion/steam-shader-guard"
TOOLS = ("id", "uname", "curl", "mktemp", "awk", "sha256sum", "chmod", "rm", "rmdir")
SHELL = shutil.which("sh")

# Every curl request uses local fixtures. Other commands are isolated wrappers
# around the host utilities, with controllable identity and failure responses.
FAKE_TOOL = textwrap.dedent(
    """\
    import json
    import os
    from pathlib import Path
    import sys

    root = Path(os.environ["FIXTURE_ROOT"])
    tool = Path(sys.argv[0]).name
    args = sys.argv[1:]
    with (root / "commands.jsonl").open("a", encoding="utf-8") as log:
        log.write(json.dumps([tool, *args]) + "\\n")

    if os.environ.get("FAIL_TOOL") == tool:
        print("fixture: " + tool + " failed", file=sys.stderr)
        sys.exit(7)
    if tool == "id":
        print(os.environ.get("TEST_UID", "1000"))
    elif tool == "uname":
        print(os.environ.get("TEST_SYSTEM", "Linux") if args == ["-s"]
              else os.environ.get("TEST_MACHINE", "x86_64"))
    elif tool == "curl":
        repository = "https://github.com/Xerrion/steam-shader-guard"
        version = os.environ["TEST_VERSION"]
        release = repository + "/releases/download/v" + version + "/"
        url = args[-1]
        output = args[args.index("-o") + 1]
        if url == repository + "/releases/latest":
            stage = "latest"
            payload = b""
        elif url == release + "SHA256SUMS":
            stage = "checksums"
            payload = (root / "SHA256SUMS").read_bytes()
        elif url == release + os.environ["TEST_ASSET"]:
            stage = "executable"
            payload = (root / "executable").read_bytes()
        else:
            print("fixture: unexpected URL " + url, file=sys.stderr)
            sys.exit(22)
        if os.environ.get("FAIL_DOWNLOAD") == stage:
            if output != "/dev/null":
                Path(output).write_bytes(b"partial download")
            print("fixture: curl download failed at " + stage, file=sys.stderr)
            sys.exit(22)
        if stage == "latest":
            output_format = args[args.index("-w") + 1]
            print(output_format.replace("%{url_effective}", os.environ["RESOLVED_URL"]), end="")
        else:
            Path(output).write_bytes(payload)
    else:
        real_tools = json.loads(os.environ["REAL_TOOLS"])
        os.execv(real_tools[tool], [real_tools[tool], *args])
    """
)

FAKE_EXECUTABLE = textwrap.dedent(
    """\
    import json
    import os
    from pathlib import Path
    import stat
    import sys

    root = Path(os.environ["FIXTURE_ROOT"])
    executable = Path(sys.argv[0])
    invocation = {
        "argv": sys.argv,
        "home": os.environ["HOME"],
        "file_mode": stat.S_IMODE(executable.stat().st_mode),
        "directory_mode": stat.S_IMODE(executable.parent.stat().st_mode),
    }
    (root / "invocation.json").write_text(json.dumps(invocation), encoding="utf-8")
    if os.environ.get("LEAVE_UNKNOWN_FILE"):
        (executable.parent / "unowned-file").write_text("keep", encoding="utf-8")
    print("fixture: managed install invoked")
    sys.exit(int(os.environ.get("INSTALL_EXIT", "0")))
    """
)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="installer-tests-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bin = self.root / "tools"
        self.home = self.root / "home"
        self.tmp = self.root / "temporary files"
        for directory in (self.bin, self.home, self.tmp):
            directory.mkdir()
        self.sentinel = self.tmp / "unrelated"
        self.sentinel.write_text("preserve", encoding="utf-8")
        real_tools = {name: shutil.which(name) for name in TOOLS}
        for name, path in real_tools.items():
            self.assertIsNotNone(path, f"test host requires {name}")
            fake = self.bin / name
            fake.write_text(f"#!{sys.executable}\n{FAKE_TOOL}", encoding="utf-8")
            fake.chmod(0o755)
        self.env = {
            "PATH": str(self.bin),
            "HOME": str(self.home),
            "TMPDIR": str(self.tmp),
            "LC_ALL": "C",
            "FIXTURE_ROOT": str(self.root),
            "REAL_TOOLS": json.dumps(real_tools),
        }
        self.configure_release()

    def configure_release(self, version="0.2.0", legacy=False, binary_marker=False):
        asset = (
            "steam-shader-guard"
            if legacy
            else f"steam-shader-guard-{version}-x86_64-unknown-linux-musl"
        )
        executable = f"#!{sys.executable}\n{FAKE_EXECUTABLE}".encode()
        (self.root / "executable").write_bytes(executable)
        digest = hashlib.sha256(executable).hexdigest()
        marker = " *" if binary_marker else "  "
        self.sums = (
            f"{'0' * 64}  steam-shader-guard-{version}-x86_64-unknown-linux-musl.tar.gz\n"
            f"{digest}{marker}{asset}\n"
        )
        (self.root / "SHA256SUMS").write_text(self.sums, encoding="utf-8")
        self.env.update(
            TEST_VERSION=version,
            TEST_ASSET=asset,
            RESOLVED_URL=f"{REPOSITORY}/releases/tag/v{version}",
        )

    def commands(self):
        path = self.root / "commands.jsonl"
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]

    def run_installer(self, cleanup_expected=True, **environment):
        # Feed the script to sh, as curl | sh does, without any network access.
        result = subprocess.run(
            [SHELL],
            input=INSTALLER.read_text(encoding="utf-8"),
            text=True,
            capture_output=True,
            cwd=self.root,
            env={**self.env, **environment},
            timeout=10,
        )
        self.assertEqual(list(self.home.iterdir()), [], "fixture must not install into HOME")
        self.assertEqual(self.sentinel.read_text(encoding="utf-8"), "preserve")
        if cleanup_expected:
            self.assertEqual(list(self.tmp.iterdir()), [self.sentinel])
        return result

    def assert_rejected(self, result, message, invoked=False, chmod=False):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(message, result.stderr)
        self.assertEqual((self.root / "invocation.json").exists(), invoked)
        self.assertEqual(any(c[0] == "chmod" for c in self.commands()), chmod)
        self.assertNotIn("Installation complete.", result.stdout)
        self.assertNotIn("Next steps:", result.stdout)

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        invocation = json.loads((self.root / "invocation.json").read_text(encoding="utf-8"))
        self.assertEqual(invocation["argv"][1:], ["install", "--apply"])
        self.assertEqual(invocation["home"], str(self.home))
        self.assertEqual(invocation["file_mode"], 0o700)
        self.assertEqual(invocation["directory_mode"], 0o700)
        self.assertIn("Downloading the information needed to check the program.", result.stdout)
        self.assertIn(
            "Checking that the downloaded program matches the published copy before running it.",
            result.stdout,
        )
        self.assertIn(
            "Installing Steam Shader Guard and adding 'Steam (Shader Guard)' to your application menu.",
            result.stdout,
        )
        self.assertIn("Installation complete.", result.stdout)
        self.assertIn("The installer did not change game settings, copy existing shaders, or start Steam.", result.stdout)
        next_steps = result.stdout.split("Next steps:\n", 1)[1]
        self.assertIn("1. Check Steam and find your game's ID.", next_steps)
        doctor = next_steps.index("   ~/.local/bin/steam-shader-guard doctor\n")
        recovery = next_steps.index("   ~/.local/bin/steam-shader-guard recover GAME_ID --apply\n")
        enable = next_steps.index("   ~/.local/bin/steam-shader-guard enable GAME_ID --apply\n")
        start_steam = next_steps.index("Start 'Steam (Shader Guard)' from your application menu")
        self.assertLess(doctor, recovery)
        self.assertLess(recovery, enable)
        self.assertLess(enable, start_steam)
        self.assertIn("Optional: copy your game's existing shaders before connecting it.", next_steps)
        self.assertIn("When you are ready, connect that game to Shader Guard.", next_steps)
        self.assertIn("Replace GAME_ID with the game's number from doctor. Do not type GAME_ID literally.", next_steps)
        self.assertIn("Fully exit Steam and any running games before steps 2 and 3.", next_steps)
        self.assertNotIn("status", result.stdout)
        self.assertNotIn("$HOME", result.stdout)
        self.assertNotIn("managed installer", result.stdout)
        self.assertNotIn("activate", result.stdout)
        self.assertNotIn("x86_64-unknown-linux-musl", result.stdout)
        commands = self.commands()
        curl_commands = [command for command in commands if command[0] == "curl"]
        self.assertEqual(
            [command[-1] for command in curl_commands],
            [
                f"{REPOSITORY}/releases/latest",
                f"{REPOSITORY}/releases/download/v{self.env['TEST_VERSION']}/SHA256SUMS",
                f"{REPOSITORY}/releases/download/v{self.env['TEST_VERSION']}/{self.env['TEST_ASSET']}",
            ],
        )
        for command in curl_commands:
            self.assertEqual(command[1:7], ["-q", "-fsSL", "--proto", "=https", "--proto-redir", "=https"])
        verify = next(i for i, command in enumerate(commands) if command[0] == "sha256sum")
        chmod = next(i for i, command in enumerate(commands) if command[0] == "chmod")
        self.assertLess(verify, chmod)
        self.assertEqual(commands[verify][1:], ["--check", "-"])
        self.assertEqual(commands[chmod][1], "700")
        cleanup = [command for command in commands if command[0] in ("rm", "rmdir")]
        executable = Path(invocation["argv"][0])
        self.assertEqual(
            cleanup,
            [
                ["rm", "-f", "--", str(executable.parent / "SHA256SUMS"), str(executable)],
                ["rmdir", "--", str(executable.parent)],
            ],
        )

    def test_versioned_asset_success(self):
        self.assert_success(self.run_installer())

    def test_next_step_command_is_doctor_not_status(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        commands = [
            line.strip()
            for line in result.stdout.splitlines()
            if line.strip().startswith("~/.local/bin/steam-shader-guard ")
        ]
        self.assertEqual(
            commands,
            [
                "~/.local/bin/steam-shader-guard doctor",
                "~/.local/bin/steam-shader-guard recover GAME_ID --apply",
                "~/.local/bin/steam-shader-guard enable GAME_ID --apply",
            ],
        )
        self.assertNotIn("status", result.stdout)

    def test_legacy_asset_success(self):
        self.configure_release(version="0.1.0", legacy=True)
        self.assert_success(self.run_installer())

    def test_binary_checksum_marker_success(self):
        self.configure_release(binary_marker=True)
        self.assert_success(self.run_installer())

    def test_stable_build_metadata_success(self):
        self.configure_release(version="1.2.3+build.4")
        self.assert_success(self.run_installer())

    def test_root_is_rejected_before_download(self):
        result = self.run_installer(TEST_UID="0")
        self.assert_rejected(result, "normal login account")
        self.assertFalse(any(c[0] in ("curl", "mktemp") for c in self.commands()))

    def test_unsupported_platforms_are_rejected(self):
        for system, machine in (("Darwin", "x86_64"), ("Linux", "aarch64"), ("Linux", "i686")):
            with self.subTest(system=system, machine=machine):
                result = self.run_installer(TEST_SYSTEM=system, TEST_MACHINE=machine)
                self.assert_rejected(result, "only supports Linux")
        self.assertFalse(any(c[0] in ("curl", "mktemp") for c in self.commands()))

    def test_missing_dependencies_are_rejected(self):
        for tool in TOOLS:
            with self.subTest(tool=tool):
                path = self.bin / tool
                disabled = self.root / f"disabled-{tool}"
                path.rename(disabled)
                try:
                    result = self.run_installer()
                    self.assert_rejected(result, f"A required command is missing: {tool}")
                finally:
                    disabled.rename(path)
        self.assertEqual(self.commands(), [])

    def test_identity_and_platform_command_failures_are_rejected(self):
        for tool, message in (("id", "user account"), ("uname", "operating system")):
            with self.subTest(tool=tool):
                self.assert_rejected(self.run_installer(FAIL_TOOL=tool), message)
        self.assertFalse(any(c[0] in ("curl", "mktemp") for c in self.commands()))

    def test_temporary_directory_failure_is_rejected(self):
        result = self.run_installer(FAIL_TOOL="mktemp")
        self.assert_rejected(result, "Could not create a folder for the download")
        self.assertFalse(any(c[0] == "curl" for c in self.commands()))

    def test_download_failures_never_execute_and_clean_partial_files(self):
        for stage, message in (
            ("latest", "Could not find the latest version"),
            ("checksums", "Could not download the information needed to check the program"),
            ("executable", "Could not download Steam Shader Guard"),
        ):
            with self.subTest(stage=stage):
                result = self.run_installer(FAIL_DOWNLOAD=stage)
                self.assert_rejected(result, message)
                self.assertIn(f"fixture: curl download failed at {stage}", result.stderr)

    def test_unexpected_release_urls_and_tags_are_rejected(self):
        for url in (
            "http://github.com/Xerrion/steam-shader-guard/releases/tag/v0.2.0",
            "https://example.invalid/releases/tag/v0.2.0",
            f"{REPOSITORY}/releases/latest",
            f"{REPOSITORY}/releases/tag/v0.2.0-rc.1",
            f"{REPOSITORY}/releases/tag/v01.2.3",
            f"{REPOSITORY}/releases/tag/v0.2.0/other",
            f"{REPOSITORY}/releases/tag/v0.2.0?query",
            f"{REPOSITORY}/releases/tag/v0.2.0\n",
            f"{REPOSITORY}/releases/tag/v0.2.\\060",
        ):
            with self.subTest(url=url):
                result = self.run_installer(RESOLVED_URL=url)
                self.assert_rejected(result, "supported version")
        self.assertTrue(all(c[-1].endswith("/releases/latest") for c in self.commands() if c[0] == "curl"))

    def test_missing_and_wrong_executable_entries_are_rejected(self):
        archive = self.sums.splitlines()[0] + "\n"
        digest = hashlib.sha256((self.root / "executable").read_bytes()).hexdigest()
        for sums in (
            "",
            archive,
            archive + f"{digest}  ./steam-shader-guard\n",
            archive + f"{digest}  steam-shader-guard-0.3.0-x86_64-unknown-linux-musl\n",
            archive + f"{digest}  steam-shader-guard-0.2.0-x86_64-unknown-linux-gnu\n",
            archive + f"{digest}  steam-shader-guard \n",
        ):
            with self.subTest(sums=sums):
                (self.root / "SHA256SUMS").write_text(sums, encoding="utf-8")
                self.assert_rejected(self.run_installer(), "download information is missing or unclear")
        self.assertFalse(any(c[0] == "sha256sum" for c in self.commands()))

    def test_malformed_checksum_entries_are_rejected(self):
        asset = self.env["TEST_ASSET"]
        digest = hashlib.sha256((self.root / "executable").read_bytes()).hexdigest()
        for sums in (
            f"{'g' * 64}  {asset}\n",
            f"{digest[:-1]}  {asset}\n",
            f"{digest}0  {asset}\n",
            f"{digest} {asset}\n",
            f"{digest}\t{asset}\n",
            f"{digest}  \n",
            f"{digest}  {asset}\r\n",
            self.sums + "\n",
            self.sums + "malformed archive checksum\n",
        ):
            with self.subTest(sums=sums):
                (self.root / "SHA256SUMS").write_text(sums, encoding="utf-8")
                self.assert_rejected(self.run_installer(), "download information is missing or unclear")
        self.assertFalse(any(c[0] == "sha256sum" for c in self.commands()))

    def test_duplicate_or_ambiguous_checksum_entries_are_rejected(self):
        executable_line = self.sums.splitlines()[1] + "\n"
        digest = hashlib.sha256((self.root / "executable").read_bytes()).hexdigest()
        for sums in (
            self.sums + executable_line,
            self.sums + f"{digest}  steam-shader-guard\n",
            f"{digest}  steam-shader-guard\n" * 2,
            self.sums + self.sums.splitlines()[0] + "\n",
        ):
            with self.subTest(sums=sums):
                (self.root / "SHA256SUMS").write_text(sums, encoding="utf-8")
                self.assert_rejected(self.run_installer(), "download information is missing or unclear")

    def test_checksum_mismatch_never_executes(self):
        with (self.root / "executable").open("ab") as binary:
            binary.write(b"\n# corrupted download\n")
        result = self.run_installer()
        self.assert_rejected(result, "The download did not pass the check")
        self.assertNotIn("FAILED", result.stdout)
        self.assertIn("checksum did NOT match", result.stderr)

    def test_checksum_tool_failure_never_executes(self):
        result = self.run_installer(FAIL_TOOL="sha256sum")
        self.assert_rejected(result, "The download did not pass the check")

    def test_chmod_failure_never_executes(self):
        result = self.run_installer(FAIL_TOOL="chmod")
        self.assert_rejected(result, "Could not prepare the downloaded program to run", chmod=True)

    def test_managed_installer_failure_is_visible_and_cleans_up(self):
        result = self.run_installer(INSTALL_EXIT="23")
        self.assert_rejected(result, "Installation failed (error code 23)", invoked=True, chmod=True)
        invocation = json.loads((self.root / "invocation.json").read_text(encoding="utf-8"))
        self.assertEqual(invocation["argv"][1:], ["install", "--apply"])
        self.assertIn("fixture: managed install invoked", result.stdout)

    def test_cleanup_does_not_remove_unknown_files(self):
        result = self.run_installer(cleanup_expected=False, LEAVE_UNKNOWN_FILE="1")
        self.assertNotEqual(result.returncode, 0)
        invocation = json.loads((self.root / "invocation.json").read_text(encoding="utf-8"))
        directory = Path(invocation["argv"][0]).parent
        self.assertEqual([path.name for path in directory.iterdir()], ["unowned-file"])
        self.assertEqual((directory / "unowned-file").read_text(encoding="utf-8"), "keep")
        self.assertIn("rmdir", result.stderr)


if __name__ == "__main__":
    unittest.main()
