# NELRoot Image Hub and Image Sources

NELRoot is moving toward a first-party image service: **NELRoot Image Hub**.
Its catalog format, image manifests, validation rules and client contract are
owned by AtDev. Docker Hub is not the canonical source and is not required by
the first-party Hub design.

## First-party Hub metadata, schema version 1

The Hub uses two JSON document types:

- **Catalog**: a versioned list of image releases, architectures, manifest URLs
  and SHA-256 digests of those manifests.
- **Image manifest**: describes one immutable image build and points to its
  compressed root filesystem archive, including its format, byte size and
  SHA-256 digest.

A catalog entry has this shape:

```json
{
  "schema_version": 1,
  "generated_at": "2026-11-20T12:00:00Z",
  "images": [
    {
      "id": "atdev/alpine-3.22",
      "name": "Alpine Linux",
      "version": "3.22.0",
      "distribution": "Alpine",
      "architecture": "x86_64",
      "description": "Minimal Linux root filesystem",
      "manifest_url": "https://hub.example.invalid/manifests/alpine-3.22-x86_64.json",
      "manifest_sha256": "<64 hexadecimal characters>",
      "tags": ["alpine", "minimal"]
    }
  ]
}
```

The corresponding manifest contains `schema_version` and an `image` object
with `id`, `name`, `version`, `distribution`, `release`,
`architecture`, `description`, `maintainer`, `license`, `tags`, and
`rootfs`. The rootfs object contains `format` (`tar.gz` in schema v1),
`size_bytes`, `sha256`, and an HTTPS `download_url`.

Schema v1 restricts identifiers and architectures, rejects duplicate catalog
IDs, requires HTTPS URLs without embedded credentials, enforces SHA-256
syntax, and applies metadata size and collection limits. The archive digest is
verified before import by the existing image download pipeline.

## Current first-party CLI

Validate a manifest:

    nelroot image hub-manifest-validate ./manifest.json

Validate a catalog:

    nelroot image hub-catalog-validate ./catalog.json

Search a local catalog copy:

    nelroot image hub-catalog-search ./catalog.json alpine
    nelroot image hub-catalog-search ./catalog.json alpine --architecture x86_64

These commands validate and search local JSON files. They do not yet contact a
hosted Hub, publish images, download a manifest, or pull an image from a Hub
catalog. The remote service, publishing workflow, authentication policy,
storage backend and manifest-fetch client are subsequent milestones.

## Existing archive-index registry

The original generic HTTPS archive index remains supported for compatibility
and private mirrors. It describes prebuilt `.tar.gz` root filesystem archives
with mandatory SHA-256 checksums and can search, pull and update those entries.

## Optional external Docker Hub adapter

The current `hub-search`, `hub-tags` and `hub-inspect` commands are an
optional external discovery adapter. They query Docker Hub metadata and inspect
OCI/Docker manifests only. They do not define the NELRoot Hub format and do not
download image layers or reconstruct root filesystems.

## Security and compatibility boundary

Catalog and manifest metadata are bounded to 8 MiB per local file. Only HTTPS
URLs without embedded credentials are accepted. Schema v1 supports
architecture identifiers `x86_64`, `aarch64`, `arm`, `armv7`, `i686`,
`riscv64`, `ppc64le` and `s390x`. The first archive format is `tar.gz`.

Future schema versions may add archive formats, signatures, provenance,
multiple rootfs variants and delta updates. They must remain explicit and
versioned; clients must reject unknown schema versions rather than guessing.


## First-party Hub HTTP server (initial implementation)

The repository now includes a separate `nelroot-hub` server binary. It provides
a first-party HTTP API for catalog reads, content-addressed manifest reads,
artifact downloads, authenticated artifact uploads and authenticated manifest
publication.

Build and start it on a Linux server:

    cargo build --release --bin nelroot-hub

Configure the service using environment variables:

- `NELROOT_HUB_BIND`: listen address; defaults to `127.0.0.1:8787`.
- `NELROOT_HUB_DATA_DIR`: persistent storage directory; defaults to
  `./nelroot-hub-data`.
- `NELROOT_HUB_PUBLIC_URL`: externally reachable HTTPS base URL, without a
  trailing slash.
- `NELROOT_HUB_PUBLISH_TOKEN`: publishing bearer token, at least 32 bytes.

Example configuration (replace the URL and token with deployment-specific
values; do not commit secrets):

    export NELROOT_HUB_BIND=127.0.0.1:8787
    export NELROOT_HUB_DATA_DIR=/var/lib/nelroot-hub
    export NELROOT_HUB_PUBLIC_URL=https://hub.example.org
    export NELROOT_HUB_PUBLISH_TOKEN='replace-with-a-long-random-secret'
    ./target/release/nelroot-hub

API endpoints:

| Method | Endpoint | Access |
|---|---|---|
| GET | `/healthz` | Public |
| GET | `/v1/catalog` | Public |
| GET | `/v1/manifests/{manifest_sha256}` | Public |
| GET | `/v1/artifacts/{artifact_sha256}` | Public |
| POST | `/v1/artifacts/{artifact_sha256}` | Bearer token |
| POST | `/v1/manifests` | Bearer token |

Publishing flow:

1. Build a gzip-compressed rootfs archive and calculate its SHA-256 digest.
2. Upload the archive bytes to `POST /v1/artifacts/{sha256}` with
   `Authorization: Bearer <token>`.
3. Prepare a schema-v1 manifest whose rootfs format is `tar.gz`, whose
   `size_bytes` and `sha256` match the uploaded archive, and whose initial
   `download_url` is a valid HTTPS URL (the server replaces it with its own
   canonical artifact URL).
4. Publish the JSON manifest to `POST /v1/manifests` with the same bearer
   token. The server validates the manifest, verifies the stored artifact,
   writes the immutable content-addressed manifest, and atomically updates the
   catalog.

Example upload:

    sha256sum alpine-rootfs.tar.gz
    curl --fail-with-body -X POST \
      -H "Authorization: Bearer $NELROOT_HUB_PUBLISH_TOKEN" \
      --data-binary @alpine-rootfs.tar.gz \
      "https://hub.example.org/v1/artifacts/<sha256>"

Example manifest publication:

    curl --fail-with-body -X POST \
      -H "Authorization: Bearer $NELROOT_HUB_PUBLISH_TOKEN" \
      -H "Content-Type: application/json" \
      --data-binary @manifest.json \
      https://hub.example.org/v1/manifests

Storage uses immutable SHA-256 filenames for artifacts and manifests. Existing
image IDs cannot be overwritten: publish a new version/architecture using a
new ID. Catalog updates are serialized within one server process and written
through a temporary file followed by rename.

Current limits and deployment boundaries:

- Uploaded rootfs archives are limited to 512 MiB; manifests and catalogs are
  limited to 8 MiB by the metadata contract.
- This first server uses a local filesystem backend and one process. It is not
  yet a horizontally scaled object-storage service.
- TLS termination, DNS, reverse proxying, rate limiting, backups, monitoring
  and secret rotation must be configured by the deployment operator.
- Publishing uses one bearer token; per-user accounts, scoped credentials,
  moderation, signed provenance and key rotation are not implemented yet.
- The server exposes a publishing API, but the NELRoot CLI does not yet have
  remote login/publish/pull commands.
