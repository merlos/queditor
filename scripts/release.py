#!/usr/bin/env python3
"""Validate, package, and checksum Queditor release binaries."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PACKAGE_FILES = (
    Path("README.md"),
    Path("LICENSE"),
    Path("docs/user-guide.md"),
    Path("docs/configuration.md"),
    Path("docs/live-ai.md"),
)


def cargo_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as source:
        manifest = tomllib.load(source)
    return manifest["package"]["version"]


def validate_tag(tag: str | None) -> str:
    version = cargo_version()
    if tag is not None and tag != f"v{version}":
        raise SystemExit(
            f"release tag {tag!r} does not match Cargo.toml version v{version}"
        )
    return version


def package(target: str, archive: str, version: str, output: Path) -> Path:
    expected = cargo_version()
    if version != expected:
        raise SystemExit(f"requested version {version!r} does not match {expected!r}")
    if any(part in target for part in ("/", "\\", "..")):
        raise SystemExit("invalid target triple")

    executable = "queditor.exe" if "windows" in target else "queditor"
    binary = ROOT / "target" / target / "release" / executable
    if not binary.is_file():
        raise SystemExit(f"release binary not found: {binary}")
    for relative in PACKAGE_FILES:
        if not (ROOT / relative).is_file():
            raise SystemExit(f"package file not found: {relative}")

    output.mkdir(parents=True, exist_ok=True)
    stem = f"queditor-v{version}-{target}"
    files = ((binary, Path(stem) / executable),) + tuple(
        (ROOT / relative, Path(stem) / relative) for relative in PACKAGE_FILES
    )
    if archive == "zip":
        destination = output / f"{stem}.zip"
        with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as bundle:
            for source, name in files:
                bundle.write(source, name.as_posix())
    elif archive == "tar.gz":
        destination = output / f"{stem}.tar.gz"
        with tarfile.open(destination, "w:gz") as bundle:
            for source, name in files:
                bundle.add(source, arcname=name.as_posix(), recursive=False)
    else:
        raise SystemExit(f"unsupported archive type: {archive}")
    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    destination.with_name(f"{destination.name}.sha256").write_text(
        f"{digest}  {destination.name}\n", encoding="utf-8", newline="\n"
    )
    return destination


def checksums(directory: Path) -> Path:
    archives = sorted(
        path
        for path in directory.iterdir()
        if path.is_file() and (path.name.endswith(".tar.gz") or path.suffix == ".zip")
    )
    if not archives:
        raise SystemExit(f"no release archives found in {directory}")
    destination = directory / "SHA256SUMS"
    with destination.open("w", encoding="utf-8", newline="\n") as output:
        for archive in archives:
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            output.write(f"{digest}  {archive.name}\n")
    return destination


def main() -> None:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    version_parser = commands.add_parser("version")
    version_parser.add_argument("--tag")
    package_parser = commands.add_parser("package")
    package_parser.add_argument("--target", required=True)
    package_parser.add_argument("--archive", choices=("tar.gz", "zip"), required=True)
    package_parser.add_argument("--version", required=True)
    package_parser.add_argument("--output", type=Path, required=True)
    checksum_parser = commands.add_parser("checksums")
    checksum_parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()

    if args.command == "version":
        version = validate_tag(args.tag)
        print(f"RELEASE_VERSION={version}")
    elif args.command == "package":
        print(package(args.target, args.archive, args.version, args.output))
    else:
        print(checksums(args.directory))


if __name__ == "__main__":
    main()
