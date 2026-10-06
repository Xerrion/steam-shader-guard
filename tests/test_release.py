import hashlib
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from scripts import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.manifest = self.root / "Cargo.toml"
        self.set_version("0.1.0")

    def set_version(self, version):
        self.manifest.write_text(f'[package]\nversion = "{version}"\n', encoding="utf-8")

    def test_tag_must_match_manifest_exactly(self):
        self.assertEqual(release.release_version(self.root, "v0.1.0"), "0.1.0")
        for tag in ("0.1.0", "v0.2.0", "v0.1.0-extra", "v0.1.0\n", "../v0.1.0"):
            with self.subTest(tag=tag), self.assertRaisesRegex(ValueError, "must match"):
                release.release_version(self.root, tag)

    def test_valid_prerelease_and_build_metadata(self):
        for version in ("1.2.3-rc.1", "1.2.3+build-1", "1.2.3-rc.1+build.2"):
            with self.subTest(version=version):
                self.set_version(version)
                self.assertEqual(release.release_version(self.root, f"v{version}"), version)

    def test_invalid_versions_are_rejected(self):
        for version in ("01.2.3", "1.2", "1.2.3-01", "1.2.3-", "../../private"):
            with self.subTest(version=version):
                self.set_version(version)
                with self.assertRaisesRegex(ValueError, "Invalid Cargo"):
                    release.release_version(self.root, f"v{version}")

    def test_dynamic_executable_is_rejected(self):
        for headers, dynamic in (("INTERP", ""), ("", "(NEEDED) libc.so")):
            with self.subTest(headers=headers, dynamic=dynamic):
                responses = [
                    subprocess.CompletedProcess([], 0, stdout=headers),
                    subprocess.CompletedProcess([], 0, stdout=dynamic),
                ]
                with patch("scripts.release.subprocess.run", side_effect=responses):
                    with self.assertRaisesRegex(ValueError, "statically linked"):
                        release.verify_binary(self.root / "binary", "0.1.0")

    def test_executable_version_is_verified(self):
        for actual, valid in (
            ("steam-shader-guard 0.1.0\n", True),
            ("steam-shader-guard 0.2.0\n", False),
        ):
            with self.subTest(actual=actual):
                responses = [
                    subprocess.CompletedProcess([], 0, stdout=""),
                    subprocess.CompletedProcess([], 0, stdout=""),
                    subprocess.CompletedProcess([], 0, stdout=actual),
                ]
                with patch("scripts.release.subprocess.run", side_effect=responses):
                    if valid:
                        release.verify_binary(self.root / "binary", "0.1.0")
                    else:
                        with self.assertRaisesRegex(ValueError, "unexpected version"):
                            release.verify_binary(self.root / "binary", "0.1.0")

    def test_archive_contents_checksums_and_cleanup(self):
        for filename in (*release.PACKAGE_FILES, "Cargo.lock", "VALIDATION.md", ".gitignore"):
            (self.root / filename).write_text(filename, encoding="utf-8")
        for directory in (".github", "scripts", "src", "tests", "third-party"):
            folder = self.root / directory
            folder.mkdir()
            (folder / "fixture").write_text(directory, encoding="utf-8")
        bytecode = self.root / "scripts" / "__pycache__"
        bytecode.mkdir()
        (bytecode / "release.pyc").write_bytes(b"bytecode")
        (self.root / "SHA256SUMS").write_text("stale checksums", encoding="utf-8")
        (self.root / "private-steam-config").write_text("private", encoding="utf-8")
        binary = self.root / "binary"
        binary.write_bytes(b"executable fixture")
        binary.chmod(0o755)

        archive_path = release.create_archive(self.root, binary, "0.1.0")
        expected_name = f"steam-shader-guard-0.1.0-{release.TARGET}"
        self.assertEqual(archive_path.name, f"{expected_name}.tar.gz")
        with tarfile.open(archive_path) as archive:
            files = {
                member.name.removeprefix(f"{expected_name}/"): member
                for member in archive.getmembers()
                if member.isfile()
            }
            expected = {
                "README.md",
                "README.da.md",
                "LICENSE",
                "CHANGELOG.md",
                "THIRD_PARTY_NOTICES.md",
                "third-party/fixture",
                "steam-shader-guard",
                "SHA256SUMS",
            }
            self.assertEqual(set(files), expected)
            self.assertEqual(files["steam-shader-guard"].mode, 0o755)
            sums = archive.extractfile(files["SHA256SUMS"]).read().decode("utf-8")
            lines = sums.splitlines()
            self.assertEqual(len(lines), len(files) - 1)
            for line in lines:
                digest, filename = line.split("  ", 1)
                payload = archive.extractfile(files[filename]).read()
                self.assertEqual(hashlib.sha256(payload).hexdigest(), digest)
        standalone = self.root / "dist" / expected_name
        self.assertEqual(standalone.read_bytes(), binary.read_bytes())
        self.assertEqual(standalone.stat().st_mode & 0o777, 0o755)
        self.assertEqual(
            (self.root / "dist" / "SHA256SUMS").read_text(encoding="utf-8"),
            f"{release.checksum(archive_path)}  {archive_path.name}\n"
            f"{release.checksum(standalone)}  {standalone.name}\n",
        )
        self.assertEqual(
            {path.name for path in (self.root / "dist").iterdir()},
            {archive_path.name, standalone.name, "SHA256SUMS"},
        )


if __name__ == "__main__":
    unittest.main()
