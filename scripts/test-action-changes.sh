#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runner="$repo_root/scripts/run-action-changes.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fake="$tmp/gnr8"
cat > "$fake" <<'SH'
#!/usr/bin/env bash
printf '%q ' "$@" >> "$FAKE_LOG"
printf '\n' >> "$FAKE_LOG"
cat <<'JSON'
{
  "base": {"ref": "HEAD", "resolved": "0123456789012345678901234567890123456789"},
  "policy": {"exempt_tags": ["internal"]},
  "summary": {"breaking": 1, "additive": 0, "doc_only": 0, "gating": 1},
  "changes": [{
    "kind": "breaking",
    "code": "operation.removed",
    "operation": "DELETE /books/{id}",
    "tags": {"base": ["books"], "current": null},
    "exempt": {"base": false, "current": null},
    "gating": true,
    "message": "operation removed"
  }]
}
JSON
exit 1
SH
chmod +x "$fake"

output="$tmp/output"
summary="$tmp/summary"
log="$tmp/args"
: > "$output"
: > "$summary"
: > "$log"

GNR8_BIN="$fake" \
BASE_REF=HEAD \
EXEMPT_TAGS=$'internal\ninternal' \
WORKING_DIRECTORIES="$repo_root/examples/bookstore" \
RUNNER_TEMP="$tmp" \
GITHUB_OUTPUT="$output" \
GITHUB_STEP_SUMMARY="$summary" \
GITHUB_JOB=test \
FAKE_LOG="$log" \
  "$runner"

grep -Fx 'gating=true' "$output" >/dev/null
grep -F 'artifact-name=gnr8-api-changes-test-' "$output" >/dev/null
grep -F 'BREAKING  DELETE /books/{id}  operation removed' "$summary" >/dev/null
grep -F -- '--exempt-tag internal --exempt-tag internal' "$log" >/dev/null
report_root="$(sed -n 's/^report-root=//p' "$output")"
test -s "$report_root/001/report.json"
test -s "$report_root/001/report.md"

stderr="$tmp/missing-stderr"
if GNR8_BIN="$fake" \
  BASE_REF=refs/heads/not-present \
  WORKING_DIRECTORIES="$repo_root/examples/bookstore" \
  RUNNER_TEMP="$tmp" \
  GITHUB_OUTPUT="$output" \
  GITHUB_STEP_SUMMARY="$summary" \
  FAKE_LOG="$log" \
  "$runner" 2> "$stderr"; then
  echo "expected missing base history to fail" >&2
  exit 1
fi
grep -F 'checkout with fetch-depth: 0' "$stderr" >/dev/null

echo "action changes tests: OK"
