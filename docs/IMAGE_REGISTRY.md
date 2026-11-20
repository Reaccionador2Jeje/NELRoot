# NELRoot Image Registry Format

NELRoot can consume a public or private HTTPS registry index. The registry is a static JSON document; the NELRoot client does not require a registry server implementation.

## Index schema v1

Host an `index.json` file containing:

```json
{
  "schema_version": 1,
  "images": [
    {
      "name": "ubuntu-noble",
      "description": "Ubuntu 24.04 LTS ARM64 root filesystem",
      "architecture": "aarch64",
      "url": "https://downloads.example.org/ubuntu-noble-aarch64.tar.gz",
      "sha256": "REPLACE_WITH_64_HEXADECIMAL_SHA256_CHARACTERS"
    }
  ]
}
```

Each image entry must contain:

- `name`: 1–64 ASCII letters, digits, hyphens or underscores.
- `description`: optional text used by search.
- `architecture`: an architecture identifier such as `aarch64` or `x86_64`.
- `url`: HTTPS URL to a gzip-compressed tar rootfs archive.
- `sha256`: exactly 64 hexadecimal characters.

The index must be no larger than 4 MiB. Duplicate `name` + `architecture` pairs are rejected. Unsupported schema versions, insecure URLs and malformed entries are rejected. Registry and archive requests also reject redirects that downgrade from HTTPS to HTTP and stop redirect chains after ten hops.

## Client commands

```bash
nelroot image search ubuntu --registry https://example.org/nelroot/index.json
nelroot image pull ubuntu-noble --registry https://example.org/nelroot/index.json --architecture aarch64
nelroot image update ubuntu-noble --registry https://example.org/nelroot/index.json --architecture aarch64
nelroot image list
nelroot image inspect ubuntu-noble
nelroot image clean
```

The `pull` command downloads the archive, checks its SHA-256 digest, safely extracts the root filesystem into NELRoot's managed image store, and registers it in the local catalog. Temporary download files are removed after the operation.

`image clean` removes only abandoned extraction staging directories older than 24 hours and cached download files older than seven days. It does not remove registered images or user root filesystems.

## Hosting guidance

1. Build a rootfs archive from a distribution's official image or a reproducible build process.
2. Publish it over HTTPS.
3. Calculate SHA-256 after the archive is finalized.
4. Add one manifest entry per supported architecture.
5. Publish the index atomically alongside the immutable archive.
6. Keep source provenance, distribution version, update date and license information in the description or your registry's companion metadata.

NELRoot does not currently operate an official AtDev image registry. Do not treat arbitrary registry entries as trusted software merely because the transport uses HTTPS; verify the publisher and archive provenance before importing them.

## Current limitations

- Only gzip-compressed tar rootfs archives are accepted.
- Registry search is a simple case-insensitive match against name and description.
- The client does not yet provide registry authentication, signature verification, delta updates or automatic background refresh. Manual `image update` is available for images stored in NELRoot's managed image directory; external registered directories are never replaced.
- A downloaded rootfs is stored and catalogued; it is not yet booted or executed by NELRoot.
