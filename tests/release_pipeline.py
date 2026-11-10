#!/usr/bin/env python3
"""Tests for the cross-platform release packaging helper."""

import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("queditor_release", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleasePipelineTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="queditor-release-")
        self.addCleanup(self.temporary.cleanup)
        self.original_root = release.ROOT
        release.ROOT = Path(self.temporary.name)
        self.addCleanup(setattr, release, "ROOT", self.original_root)
        (release.ROOT / "docs").mkdir()
        (release.ROOT / "Cargo.toml").write_text(
            '[package]\nname = "queditor"\nversion = "1.2.3"\n', encoding="utf-8"
        )
        for relative in release.PACKAGE_FILES:
            path = release.ROOT / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(str(relative), encoding="utf-8")

    def binary(self, target, executable):
        path = release.ROOT / "target" / target / "release" / executable
        path.parent.mkdir(parents=True)
        path.write_bytes(b"binary")

    def test_tag_must_match_manifest(self):
        self.assertEqual(release.validate_tag("v1.2.3"), "1.2.3")
        with self.assertRaises(SystemExit):
            release.validate_tag("v1.2.4")

    def test_packages_unix_archive_with_single_root(self):
        target = "x86_64-unknown-linux-musl"
        self.binary(target, "queditor")
        archive = release.package(target, "tar.gz", "1.2.3", release.ROOT / "dist")
        with tarfile.open(archive) as bundle:
            names = bundle.getnames()
        self.assertIn(f"queditor-v1.2.3-{target}/queditor", names)
        self.assertIn(f"queditor-v1.2.3-{target}/LICENSE", names)

    def test_packages_windows_and_generates_checksums(self):
        target = "x86_64-pc-windows-msvc"
        self.binary(target, "queditor.exe")
        archive = release.package(target, "zip", "1.2.3", release.ROOT / "dist")
        with zipfile.ZipFile(archive) as bundle:
            self.assertIn(f"queditor-v1.2.3-{target}/queditor.exe", bundle.namelist())
        sidecar = archive.with_name(f"{archive.name}.sha256")
        self.assertRegex(
            sidecar.read_text(encoding="utf-8"),
            rf"^[0-9a-f]{{64}}  {archive.name}\n$",
        )
        checksum = release.checksums(release.ROOT / "dist").read_text(encoding="utf-8")
        self.assertRegex(checksum, rf"^[0-9a-f]{{64}}  {archive.name}\n$")


if __name__ == "__main__":
    unittest.main()
