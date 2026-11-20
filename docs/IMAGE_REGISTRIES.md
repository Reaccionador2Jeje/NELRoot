# NELRoot Image Registries

NELRoot supports two distinct image-source paths.

## Existing NELRoot HTTPS archive index

The original registry format is a small versioned JSON index containing
prebuilt .tar.gz root filesystem archives and mandatory SHA-256 digests.
It remains useful for AtDev-maintained rootfs bundles and private mirrors.

## Docker Hub discovery and OCI manifest inspection

NELRoot can query the public Docker Hub web catalog to search repositories and
list their tags. The registry client obtains a short-lived anonymous bearer token
from Docker Hub's token service and resolves OCI/Docker manifests through the
Docker Registry HTTP API. Multi-platform image indexes are inspected by selecting
the requested Linux architecture (for example x86_64 -> amd64 or aarch64 -> arm64).

Examples:

    nelroot image hub-search alpine
    nelroot image hub-tags alpine
    nelroot image hub-inspect alpine latest --architecture x86_64

Repository names without a namespace resolve to Docker Hub's library/ namespace.
Public search and tag results are metadata only.

## Current boundary

This milestone provides remote repository discovery and platform-aware manifest
inspection only. It does not download image layers, apply OCI whiteouts,
reconstruct a root filesystem, or register the image locally. Those operations
require the dedicated layer-pull and safe extraction pipeline.

The Docker Hub path is intentionally separate from the legacy archive-index
path. Future registry adapters should share a common image-source interface,
while retaining registry-specific authentication and manifest handling.
