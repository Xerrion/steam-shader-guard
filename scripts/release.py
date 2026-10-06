#!/usr/bin/env python3
import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

TARGET = "x86_64-unknown-linux-musl"
PACKAGE_FILES = (
    ".gitignore",
    "Cargo.toml",
    "Cargo.lock",
    "README.md",
    "README.da.md",
    "LICENSE",
    "CHANGELOG.md",
    "THIRD_PARTY_NOTICES.md",
    "VALIDATION.md",
)
PACKAGE_DIRECTORIES = (".github", "scripts", "src", "tests", "third-party")


def release_version(root: Path, tag: str) -> str:
    with (root / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    number = r"(?:0|[1-9][0-9]*)"
    identifier = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    semver = (
        rf"{number}\.{number}\.{number}"
        rf"(?:-{identifier}(?:\.{identifier})*)?"
        r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    )
    if not isinstance(version, str) or re.fullmatch(semver, version) is None:
        raise ValueError(f"Invalid Cargo package version: {version!r}")
    if tag != f"v{version}":
        raise ValueError(f"Release tag {tag!r} must match Cargo.toml: v{version}")
    return version


def checksum(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def verify_binary(binary: Path, version: str) -> None:
    headers = subprocess.run(
        ["readelf", "--program-headers", str(binary)],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    ).stdout
    dynamic = subprocess.run(
        ["readelf", "--dynamic", str(binary)],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    ).stdout
    if "INTERP" in headers or "(NEEDED)" in dynamic:
        raise ValueError("Release executable must be statically linked")
    actual_version = subprocess.run(
        [str(binary), "--version"], check=True, stdout=subprocess.PIPE, text=True
    ).stdout.strip()
    if actual_version != f"steam-shader-guard {version}":
        raise ValueError(f"Release executable has unexpected version: {actual_version!r}")


def create_archive(root: Path, binary: Path, version: str) -> Path:
    dist = root / "dist"
    dist.mkdir(exist_ok=True)
    name = f"steam-shader-guard-{version}-{TARGET}"
    archive_path = dist / f"{name}.tar.gz"
    with tempfile.TemporaryDirectory(prefix=".package-", dir=dist) as staging:
        package = Path(staging) / name
        package.mkdir()
        for filename in PACKAGE_FILES:
            shutil.copy2(root / filename, package / filename)
        for directory in PACKAGE_DIRECTORIES:
            shutil.copytree(
                root / directory,
                package / directory,
                ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
            )
        shutil.copy2(binary, package / "steam-shader-guard")
        entries = sorted(path for path in package.rglob("*") if path.is_file())
        sums = "".join(
            f"{checksum(path)}  {path.relative_to(package).as_posix()}\n"
            for path in entries
        )
        (package / "SHA256SUMS").write_text(sums, encoding="utf-8")
        with tarfile.open(archive_path, "w:gz") as archive:
            archive.add(package, arcname=name)
    (dist / "SHA256SUMS").write_text(
        f"{checksum(archive_path)}  {archive_path.name}\n", encoding="utf-8"
    )
    return archive_path


def main() -> None:
    parser = argparse.ArgumentParser(description="Validate and package version-tag releases")
    parser.add_argument("command", choices=("version", "package"))
    parser.add_argument("tag", help="Version tag matching Cargo.toml, for example v0.1.0")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        version = release_version(root, args.tag)
        if args.command == "version":
            print(version)
            return
        subprocess.run(
            ["cargo", "build", "--release", "--locked", "--target", TARGET],
            cwd=root,
            check=True,
        )
        binary = root / "target" / TARGET / "release" / "steam-shader-guard"
        verify_binary(binary, version)
        print(create_archive(root, binary, version))
    except ValueError as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
