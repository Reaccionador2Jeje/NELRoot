#!/usr/bin/env python3
import importlib.util
import io
import tempfile
import tarfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("seed_official_images.py")
SPEC = importlib.util.spec_from_file_location("seed_official_images", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(MODULE)


class OfficialImageValidationTests(unittest.TestCase):
    def make_archive(self, entries):
        temporary = tempfile.NamedTemporaryFile(suffix=".tar.gz", delete=False)
        temporary.close()
        path = Path(temporary.name)
        with tarfile.open(path, "w:gz") as archive:
            for name, content in entries:
                info = tarfile.TarInfo(name)
                info.size = len(content)
                archive.addfile(info, io.BytesIO(content))
        self.addCleanup(path.unlink, missing_ok=True)
        return path

    def test_accepts_linux_rootfs_archive(self):
        archive = self.make_archive([
            ("etc/os-release", b"ID=alpine\n"),
            ("usr/bin/busybox", b"binary"),
        ])
        MODULE.validate_archive(archive)

    def test_rejects_parent_traversal(self):
        archive = self.make_archive([
            ("etc/os-release", b"ID=alpine\n"),
            ("../../outside", b"no"),
        ])
        with self.assertRaises(RuntimeError):
            MODULE.validate_archive(archive)

    def test_rejects_archive_without_rootfs_markers(self):
        archive = self.make_archive([("docs/readme.txt", b"not a rootfs")])
        with self.assertRaises(RuntimeError):
            MODULE.validate_archive(archive)

    def test_rejects_non_https_source_before_network_access(self):
        with self.assertRaises(ValueError):
            MODULE.request_bytes("http://example.invalid/")

    def test_checksum_mismatch_never_publishes(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "image.tar.gz"
            with self.assertRaises(Exception):
                MODULE.download_verified(
                    "https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/"
                    "alpine-minirootfs-3.24.4-x86_64.tar.gz",
                    "0" * 64,
                    destination,
                )
            self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
