#!/usr/bin/env python3
"""Fetch official Alpine/Ubuntu Base rootfs archives and publish them to NELRoot Hub.

No third-party Python packages are required. Upstream SHA-256 sidecars are fetched
from the same official HTTPS release directory as each archive.
"""
from __future__ import annotations

import argparse
import hashlib
import html
import json
import os
import re
import sys
import tarfile
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path, PurePosixPath

MAX_ARCHIVE = 512 * 1024 * 1024
USER_AGENT = "NELRoot-Official-Image-Publisher/1.0"
ALPINE_BASE = "https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/"
UBUNTU_BASE = "https://cdimages.ubuntu.com/ubuntu-base/releases/26.04.1/release/"
ARCHES = ("x86_64", "aarch64")
UBUNTU_ARCH = {"x86_64": "amd64", "aarch64": "arm64"}


def request_bytes(url: str, limit: int = 4 * 1024 * 1024) -> bytes:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "https" or not parsed.hostname:
        raise ValueError(f"refusing non-HTTPS URL: {url}")
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=90) as response:
        final = urllib.parse.urlparse(response.geturl())
        if final.scheme != "https" or final.hostname != parsed.hostname:
            raise ValueError(f"unexpected redirect while fetching {url}")
        length = response.headers.get("Content-Length")
        if length and int(length) > limit:
            raise ValueError(f"response exceeds {limit} bytes: {url}")
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"response exceeds {limit} bytes: {url}")
    return data


def alpine_release(architecture: str) -> tuple[str, str, str]:
    directory = urllib.parse.urljoin(ALPINE_BASE, f"{architecture}/")
    page = request_bytes(directory).decode("utf-8", "replace")
    pattern = re.compile(
        r"""href=["'](alpine-minirootfs-(\d+\.\d+\.\d+)-"""
        + re.escape(architecture)
        + r""".tar\.gz)["']"""
    )
    matches = pattern.findall(page)
    if not matches:
        raise RuntimeError(f"no stable Alpine minirootfs found for {architecture}")
    filename, version = max(
        matches, key=lambda item: tuple(int(part) for part in item[1].split("."))
    )
    archive_url = urllib.parse.urljoin(directory, filename)
    checksum_url = archive_url + ".sha256"
    checksum_text = request_bytes(checksum_url, 4096).decode("ascii", "replace")
    digests = re.findall(r"\b[0-9a-fA-F]{64}\b", checksum_text)
    if len(digests) != 1:
        raise RuntimeError(f"could not unambiguously parse Alpine checksum for {filename}")
    return version, archive_url, digests[0].lower()


def ubuntu_release(architecture: str) -> tuple[str, str, str]:
    upstream_arch = UBUNTU_ARCH[architecture]
    filename = f"ubuntu-base-26.04.1-base-{upstream_arch}.tar.gz"
    sums = request_bytes(urllib.parse.urljoin(UBUNTU_BASE, "SHA256SUMS"), 64 * 1024)
    expected = None
    for line in sums.decode("ascii", "replace").splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[-1].lstrip("*") == filename:
            expected = parts[0].lower()
            break
    if not expected or not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise RuntimeError(f"{filename} is missing a valid entry in official SHA256SUMS")
    return "26.04.1", urllib.parse.urljoin(UBUNTU_BASE, filename), expected


def download_verified(url: str, expected: str, destination: Path) -> tuple[str, int]:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "https" or parsed.hostname not in {
        "dl-cdn.alpinelinux.org", "cdimages.ubuntu.com"
    }:
        raise ValueError(f"archive URL is not from an approved official host: {url}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256()
    total = 0
    fd, temporary_name = tempfile.mkstemp(prefix=".nelroot-download-", dir=destination.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=180) as response:
                final = urllib.parse.urlparse(response.geturl())
                if final.scheme != "https" or final.hostname != parsed.hostname:
                    raise ValueError("archive redirected outside its official HTTPS host")
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    total += len(chunk)
                    if total > MAX_ARCHIVE:
                        raise ValueError("rootfs archive exceeds NELRoot's 512 MiB limit")
                    digest.update(chunk)
                    output.write(chunk)
            output.flush()
            os.fsync(output.fileno())
        actual = digest.hexdigest()
        if actual != expected:
            raise RuntimeError(
                f"SHA-256 mismatch for {url}: expected {expected}, received {actual}"
            )
        os.replace(temporary_name, destination)
        return actual, total
    except Exception:
        try:
            os.unlink(temporary_name)
        except FileNotFoundError:
            pass
        raise


def validate_archive(path: Path) -> None:
    if path.stat().st_size < 2:
        raise RuntimeError("downloaded archive is empty")
    with path.open("rb") as stream:
        if stream.read(2) != b"\x1f\x8b":
            raise RuntimeError("upstream artifact is not a gzip archive")
    roots: set[str] = set()
    count = 0
    with tarfile.open(path, mode="r:gz") as archive:
        for member in archive:
            count += 1
            if count > 500_000:
                raise RuntimeError("rootfs archive contains too many entries")
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts:
                raise RuntimeError(f"unsafe path in official archive: {member.name}")
            parts = [part for part in name.parts if part not in ("", ".")]
            if parts:
                roots.add(parts[0])
    if not roots.intersection({"bin", "etc", "sbin", "usr"}):
        raise RuntimeError("archive does not look like a Linux root filesystem")


def post_json(url: str, token: str, payload: dict) -> dict:
    data = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()
    request = urllib.request.Request(
        url, data=data, method="POST",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "User-Agent": USER_AGENT,
        },
    )
    return read_json_response(request)


def read_json_response(request: urllib.request.Request) -> dict:
    try:
        with urllib.request.urlopen(request, timeout=180) as response:
            body = response.read(1024 * 1024 + 1)
    except urllib.error.HTTPError as error:
        detail = error.read(4096).decode("utf-8", "replace")
        raise RuntimeError(f"Hub returned HTTP {error.code}: {detail}") from error
    if len(body) > 1024 * 1024:
        raise RuntimeError("Hub response exceeded 1 MiB")
    return json.loads(body.decode("utf-8"))


def publish_one(
    distro: str, architecture: str, hub: str, token: str, workdir: Path, dry_run: bool
) -> None:
    if distro == "alpine":
        release, upstream_url, expected = alpine_release(architecture)
        image_id = f"alpine/{release}-{architecture}"
        name = f"Alpine Linux Mini Rootfs {release} ({architecture})"
        description = "Official Alpine Linux minirootfs, verified against the upstream SHA-256 sidecar."
        license_name = "MIT"
        tags = ["alpine", "official-upstream", "minirootfs", "minimal"]
    else:
        release, upstream_url, expected = ubuntu_release(architecture)
        image_id = f"ubuntu/{release}-{architecture}"
        name = f"Ubuntu Base {release} ({architecture})"
        description = "Official Canonical Ubuntu Base root filesystem, verified against upstream SHA256SUMS."
        license_name = "Various"
        tags = ["ubuntu", "official-upstream", "ubuntu-base", "minimal", "lts"]

    archive_path = workdir / f"{distro}-{release}-{architecture}.tar.gz"
    print(f"\n[{distro}] {architecture}: resolving official release")
    print(f"  Source: {upstream_url}")
    digest, size = download_verified(upstream_url, expected, archive_path)
    validate_archive(archive_path)
    print(f"  Verified SHA-256: {digest}")
    print(f"  Archive size: {size:,} bytes")
    if dry_run:
        print("  Dry-run: validated locally; nothing uploaded.")
        return

    upload = urllib.request.Request(
        f"{hub}/v1/artifacts/{digest}",
        data=archive_path.read_bytes(),
        method="POST",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/gzip",
            "User-Agent": USER_AGENT,
        },
    )
    upload_result = read_json_response(upload)
    manifest = {
        "schema_version": 1,
        "image": {
            "id": image_id,
            "name": name,
            "version": release,
            "distribution": "Alpine Linux" if distro == "alpine" else "Ubuntu",
            "release": release,
            "architecture": architecture,
            "description": description,
            "maintainer": "AtDev",
            "license": license_name,
            "tags": tags,
            "rootfs": {
                "format": "tar.gz",
                "size_bytes": size,
                "sha256": digest,
                "download_url": upstream_url,
            },
        },
    }
    published = post_json(f"{hub}/v1/manifests", token, manifest)
    print(f"  Artifact stored: {upload_result.get('stored')}")
    print(f"  Published ID: {published.get('id')}")
    print(f"  Manifest: {published.get('manifest_url')}")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Seed NELRoot Hub with real official Alpine and Ubuntu Base rootfs images."
    )
    parser.add_argument("--hub", default=os.environ.get("NELROOT_HUB_URL"))
    parser.add_argument("--token", default=os.environ.get("NELROOT_HUB_PUBLISH_TOKEN"))
    parser.add_argument("--distro", choices=("all", "alpine", "ubuntu"), default="all")
    parser.add_argument("--architectures", nargs="+", choices=(*ARCHES, "all"), default=["all"])
    parser.add_argument("--workdir", type=Path, default=Path("dist/official-rootfs"))
    parser.add_argument("--dry-run", action="store_true", help="download and verify, without publishing")
    args = parser.parse_args()

    if args.dry_run:
        hub = ""
        token = ""
    else:
        if not args.hub or not args.token:
            parser.error("set NELROOT_HUB_URL and NELROOT_HUB_PUBLISH_TOKEN, or pass --dry-run")
        parsed = urllib.parse.urlparse(args.hub)
        if parsed.scheme != "https" or not parsed.hostname:
            parser.error("--hub must be an HTTPS base URL")
        if len(args.token.encode()) < 32:
            parser.error("Hub publish token must contain at least 32 bytes")
        hub = args.hub.rstrip("/")
        token = args.token

    architectures = list(ARCHES) if "all" in args.architectures else args.architectures
    distros = ("alpine", "ubuntu") if args.distro == "all" else (args.distro,)
    try:
        for distro in distros:
            for architecture in architectures:
                publish_one(distro, architecture, hub, token, args.workdir, args.dry_run)
    except (OSError, ValueError, RuntimeError, tarfile.TarError, urllib.error.URLError) as error:
        print(f"\nERROR: {error}", file=sys.stderr)
        return 1
    print("\nNELRoot official image seed completed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
