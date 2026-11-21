#!/usr/bin/env bash
set -Eeuo pipefail

HUB_BINARY="${1:-target/release/nelroot-hub}"
[[ -x "$HUB_BINARY" ]] || {
    echo "Hub server binary not found or not executable: $HUB_BINARY" >&2
    exit 1
}

WORK_DIR="$(mktemp -d)"
HUB_PID=""
PORT=18787
BASE_URL="http://127.0.0.1:$PORT"
TOKEN="nelroot-ci-test-token-$(python3 -c 'import secrets; print(secrets.token_hex(32))')"

cleanup() {
    if [[ -n "$HUB_PID" ]]; then
        kill "$HUB_PID" 2>/dev/null || true
        wait "$HUB_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

export NELROOT_HUB_BIND="127.0.0.1:$PORT"
export NELROOT_HUB_DATA_DIR="$WORK_DIR/data"
export NELROOT_HUB_PUBLIC_URL="https://hub.test.invalid"
export NELROOT_HUB_PUBLISH_TOKEN="$TOKEN"

"$HUB_BINARY" >"$WORK_DIR/server.log" 2>&1 &
HUB_PID=$!

echo "==> Waiting for NELRoot Image Hub..."
READY=0
for _ in $(seq 1 40); do
    if curl --silent --fail "$BASE_URL/healthz" >"$WORK_DIR/health.json"; then
        READY=1
        break
    fi
    sleep 0.25
done
if [[ "$READY" != "1" ]]; then
    cat "$WORK_DIR/server.log" >&2
    echo "NELRoot Image Hub did not become healthy." >&2
    exit 1
fi
python3 - "$WORK_DIR/health.json" <<'PY'
import json
import sys
health = json.load(open(sys.argv[1], encoding="utf-8"))
assert health["service"] == "NELRoot Image Hub"
assert health["status"] == "ok"
assert health["api_version"] == 1
PY

echo "==> Creating a tiny test rootfs archive..."
mkdir -p "$WORK_DIR/rootfs/etc"
printf 'NELRoot Hub integration test\n' > "$WORK_DIR/rootfs/etc/nelroot-test"
tar -czf "$WORK_DIR/rootfs.tar.gz" -C "$WORK_DIR/rootfs" .
ARCHIVE_SHA="$(sha256sum "$WORK_DIR/rootfs.tar.gz" | awk '{print $1}')"
ARCHIVE_SIZE="$(stat -c '%s' "$WORK_DIR/rootfs.tar.gz")"

echo "==> Checking that unauthenticated uploads are rejected..."
UNAUTH_STATUS="$(curl --silent --output "$WORK_DIR/unauthorized.json" --write-out '%{http_code}' \
    -X POST --data-binary @"$WORK_DIR/rootfs.tar.gz" \
    "$BASE_URL/v1/artifacts/$ARCHIVE_SHA")"
[[ "$UNAUTH_STATUS" == "401" ]] || {
    echo "Expected HTTP 401 for unauthenticated upload, received $UNAUTH_STATUS." >&2
    exit 1
}

echo "==> Uploading the test artifact..."
curl --silent --show-error --fail-with-body -X POST \
    -H "Authorization: Bearer $TOKEN" \
    --data-binary @"$WORK_DIR/rootfs.tar.gz" \
    "$BASE_URL/v1/artifacts/$ARCHIVE_SHA" > "$WORK_DIR/upload.json"

python3 - "$WORK_DIR/upload.json" "$ARCHIVE_SHA" "$ARCHIVE_SIZE" <<'PY'
import json
import sys
response = json.load(open(sys.argv[1], encoding="utf-8"))
assert response["sha256"] == sys.argv[2]
assert response["size_bytes"] == int(sys.argv[3])
assert response["stored"] is True
PY

echo "==> Publishing a schema-v1 image manifest..."
python3 - "$WORK_DIR/manifest.json" "$ARCHIVE_SHA" "$ARCHIVE_SIZE" <<'PY'
import json
import sys
manifest = {
    "schema_version": 1,
    "image": {
        "id": "atdev/ci-smoke-test",
        "name": "NELRoot Hub CI Test Image",
        "version": "1.0.0",
        "distribution": "NELRoot Test",
        "release": "1.0.0",
        "architecture": "x86_64",
        "description": "Temporary image used by the Hub integration test.",
        "maintainer": "AtDev CI",
        "license": "MIT",
        "tags": ["ci", "test"],
        "rootfs": {
            "format": "tar.gz",
            "size_bytes": int(sys.argv[3]),
            "sha256": sys.argv[2],
            "download_url": "https://placeholder.example.invalid/rootfs.tar.gz",
        },
    },
}
with open(sys.argv[1], "w", encoding="utf-8") as output:
    json.dump(manifest, output)
PY

curl --silent --show-error --fail-with-body -X POST \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    --data-binary @"$WORK_DIR/manifest.json" \
    "$BASE_URL/v1/manifests" > "$WORK_DIR/publish.json"

MANIFEST_SHA="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["manifest_sha256"])' "$WORK_DIR/publish.json")"
MANIFEST_URL="$BASE_URL/v1/manifests/$MANIFEST_SHA"

echo "==> Verifying catalog, manifest and downloaded artifact..."
curl --silent --show-error --fail-with-body "$BASE_URL/v1/catalog" > "$WORK_DIR/catalog.json"
curl --silent --show-error --fail-with-body "$MANIFEST_URL" > "$WORK_DIR/remote-manifest.json"
curl --silent --show-error --fail-with-body "$BASE_URL/v1/artifacts/$ARCHIVE_SHA" > "$WORK_DIR/downloaded.tar.gz"

python3 - "$WORK_DIR/catalog.json" "$WORK_DIR/remote-manifest.json" "$ARCHIVE_SHA" "$MANIFEST_SHA" <<'PY'
import hashlib
import json
import sys
catalog = json.load(open(sys.argv[1], encoding="utf-8"))
manifest_bytes = open(sys.argv[2], "rb").read()
manifest = json.loads(manifest_bytes)
assert catalog["schema_version"] == 1
assert len(catalog["images"]) == 1
entry = catalog["images"][0]
assert entry["id"] == "atdev/ci-smoke-test"
assert entry["architecture"] == "x86_64"
assert entry["manifest_sha256"] == sys.argv[4]
assert hashlib.sha256(manifest_bytes).hexdigest() == sys.argv[4]
assert manifest["image"]["rootfs"]["sha256"] == sys.argv[3]
assert manifest["image"]["rootfs"]["download_url"].endswith("/v1/artifacts/" + sys.argv[3])
PY

DOWNLOADED_SHA="$(sha256sum "$WORK_DIR/downloaded.tar.gz" | awk '{print $1}')"
[[ "$DOWNLOADED_SHA" == "$ARCHIVE_SHA" ]] || {
    echo "Downloaded artifact checksum mismatch." >&2
    exit 1
}

echo "==> Checking immutable image IDs..."
DUPLICATE_STATUS="$(curl --silent --output "$WORK_DIR/duplicate.json" --write-out '%{http_code}' \
    -X POST -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    --data-binary @"$WORK_DIR/manifest.json" \
    "$BASE_URL/v1/manifests")"
[[ "$DUPLICATE_STATUS" == "409" ]] || {
    echo "Expected HTTP 409 for duplicate image ID, received $DUPLICATE_STATUS." >&2
    exit 1
}

echo "NELRoot Image Hub integration test passed."
