#!/usr/bin/env bash
set -euo pipefail

BINARY="${1:-target/release/nelroot}"
[[ -x "$BINARY" ]] || { echo "NELRoot binary not found: $BINARY" >&2; exit 1; }

"$BINARY" --version
"$BINARY" --help >/dev/null
"$BINARY" engine status >/dev/null
"$BINARY" engine status --help >/dev/null
"$BINARY" elf inspect "$BINARY" >/dev/null
"$BINARY" elf plan "$BINARY" >/dev/null

PLAIN_OUTPUT="$("$BINARY" info)"
if printf '%s' "$PLAIN_OUTPUT" | grep -q $'\033'; then
  echo "Redirected output unexpectedly contains ANSI styling" >&2
  exit 1
fi

STATUS_JSON="$("$BINARY" engine status --json)"
printf '%s\n' "$STATUS_JSON" | python3 -c '
import json, sys
data = json.load(sys.stdin)
assert data["schema_version"] == 1
assert data["product"] == "NELRoot"
assert isinstance(data["version"], str) and data["version"]
assert isinstance(data["host"]["os"], str) and data["host"]["os"]
assert isinstance(data["host"]["architecture"], str) and data["host"]["architecture"]
assert isinstance(data["components"], list)
assert data["guest_execution"]["available"] is False
required = {
    "core",
    "host_detection",
    "configuration",
    "image_catalog",
    "registry_client",
    "event_logging",
    "elf64_inspector",
    "elf_load_planner",
}
available = {item["name"] for item in data["components"] if item["status"] == "available"}
assert required <= available
print("NELRoot CLI and NEXORA JSON contract: OK")
'
