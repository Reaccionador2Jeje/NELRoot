# Official rootfs publishing

scripts/seed_official_images.py builds the initial first-party catalog from real upstream root filesystem archives. It does not fabricate image files, checksums, or package contents.

## Initial image set

| Distribution | Release policy | Architectures |
|---|---|---|
| Alpine Linux minirootfs | Highest stable version listed by Alpine's official latest-stable directory at run time | x86_64, aarch64 |
| Ubuntu Base | Pinned release 26.04.1 | x86_64 (amd64), aarch64 (arm64) |

Official sources:

- Alpine: https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/
- Ubuntu Base: https://cdimages.ubuntu.com/ubuntu-base/releases/26.04.1/release/

The publisher retrieves the upstream checksum sidecar (.sha256 for Alpine and SHA256SUMS for Ubuntu), downloads the archive over HTTPS, computes SHA-256 locally, enforces the Hub's 512 MiB limit, and checks that the gzip/tar archive contains a recognizable root filesystem before upload. The Hub independently checks the artifact digest and exact size before accepting the manifest.

**Trust boundary:** this workflow verifies the checksum published alongside each archive over the same official HTTPS host. It does not yet verify upstream detached GPG signatures. Do not describe this as independent cryptographic provenance. The Hub's Ed25519 manifest-signing commands remain a separate maintainer operation.

## Dry run

No Hub credentials are needed for a local download and validation:

    python3 scripts/seed_official_images.py --dry-run --distro all

Limit the run to one distribution or architecture:

    python3 scripts/seed_official_images.py --dry-run --distro alpine
    python3 scripts/seed_official_images.py --dry-run --distro ubuntu --architectures aarch64

## Publish to the Hub

Wait until the Hub deployment is healthy and /healthz returns HTTP 200. Then set credentials in the shell; never commit the publish token:

    export NELROOT_HUB_URL="https://nelroot-image-hub-production.up.railway.app"
    read -rsp "NELRoot Hub publish token: " NELROOT_HUB_PUBLISH_TOKEN
    export NELROOT_HUB_PUBLISH_TOKEN
    echo
    python3 scripts/seed_official_images.py --distro all
    unset NELROOT_HUB_PUBLISH_TOKEN

Published IDs are immutable. Re-running the same release will safely reuse an already stored content-addressed artifact, but the Hub will reject a duplicate image ID. A newer Alpine release naturally gets a new image ID. Ubuntu updates should be added as a new pinned release rather than overwriting 26.04.1.

After publishing, inspect the catalog:

    curl --fail --silent --show-error "$NELROOT_HUB_URL/v1/catalog"
    nelroot image hub-remote-search alpine --hub "$NELROOT_HUB_URL"
    nelroot image hub-remote-search ubuntu --hub "$NELROOT_HUB_URL"

## Toolchain images

Python, Node.js, Rust, Go, Java, GCC and CMake environments are **not** mislabeled as upstream distribution rootfs images. They need separate reproducible build recipes layered on top of a verified base rootfs, with exact package versions, package-repository metadata, build logs and a maintainer signature. That is the next image-catalog milestone; this seed script intentionally publishes only the official base rootfs archives.
