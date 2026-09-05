#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runner="$repo_root/scripts/run-action-changes.sh"
tmp="$(mktemp -d)"
# A working directory whose name is meaningful in Markdown, inside the checkout so the runner's base
# ref resolves from it. `target/` is ignored, so this leaves the tree clean.
weird_dir="$repo_root/target/gnr8-action-changes-test/a<b>&c"
mkdir -p "$weird_dir"
trap 'rm -rf "$tmp" "$repo_root/target/gnr8-action-changes-test"' EXIT

# The fake renders nothing: it returns what the real CLI returns for each format, so the runner is
# tested against the CLI's contract rather than against a second implementation of the report.
fake="$tmp/gnr8"
cat > "$fake" <<'SH'
#!/usr/bin/env bash
printf '%q ' "$@" >> "$FAKE_LOG"
printf '\n' >> "$FAKE_LOG"
if [[ -n "${FAIL_PROJECT:-}" && "$PWD" == "$FAIL_PROJECT" ]]; then exit 2; fi
for arg in "$@"; do
  if [[ "$arg" == "--markdown" ]]; then
    if [[ "${OVERSIZED:-false}" == true ]]; then
      python3 -c 'print("    " + "x" * (901 * 1024))'
      exit 1
    fi
    cat <<'MARKDOWN'
Base: <code>HEAD</code> → <code>0123456789012345678901234567890123456789</code>

Exempt tags: <code>internal</code>

Summary: 1 breaking, 0 additive, 0 doc-only, 1 gating.

Breaking — gating (1)

    BREAKING  DELETE /books/{id}  operation removed ## injected heading
        Code: operation.removed
        SDK operations: deleteBook (DELETE /books/{id}), listBooks (GET /books)
        Source: handlers/<books>.go:42
MARKDOWN
    exit 1
  fi
done
cat <<'JSON'
{
  "schema_version": 1,
  "base": {"ref": "HEAD", "resolved": "0123456789012345678901234567890123456789"},
  "policy": {"exempt_tags": ["internal"]},
  "summary": {"breaking": 1, "additive": 0, "doc_only": 0, "gating": 1},
  "changes": [{
    "kind": "breaking",
    "code": "operation.removed",
    "operation": "DELETE /books/{id}",
    "affected_operations": {
      "base": [
        {"operation": "DELETE /books/{id}", "operation_id": "deleteBook"},
        {"operation": "GET /books", "operation_id": "listBooks"}
      ],
      "current": null
    },
    "tags": {"base": ["books"], "current": null},
    "exempt": {"base": false, "current": null},
    "gating": true,
    "message": "operation removed\n## injected heading",
    "file": "handlers/<books>.go",
    "line": 42
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
EXEMPT_TAGS=$'internal\ninternal\n#partner\n partner APIs ' \
WORKING_DIRECTORIES="$repo_root/examples/bookstore" \
RUNNER_TEMP="$tmp" \
GITHUB_OUTPUT="$output" \
GITHUB_STEP_SUMMARY="$summary" \
GITHUB_JOB=test \
FAKE_LOG="$log" \
  "$runner"

grep -Fx 'gating=true' "$output" >/dev/null
grep -F 'artifact-name=gnr8-api-changes-test-' "$output" >/dev/null
grep -F 'BREAKING  DELETE /books/{id}  operation removed ## injected heading' "$summary" >/dev/null
grep -F 'SDK operations: deleteBook (DELETE /books/{id}), listBooks (GET /books)' "$summary" >/dev/null
grep -F 'Source: handlers/<books>.go:42' "$summary" >/dev/null
artifact_name="$(sed -n 's/^artifact-name=//p' "$output")"
grep -Ex 'artifact-name=gnr8-api-changes-test-[0-9a-f]{8}' "$output" >/dev/null
grep -Fx "<!-- gnr8-api-changes:$artifact_name -->" "$summary" >/dev/null
grep -Fx "marker=<!-- gnr8-api-changes:$artifact_name -->" "$output" >/dev/null
if grep -E '^## injected heading$|^```' "$summary" >/dev/null; then
  echo "report content escaped its indented code block" >&2
  exit 1
fi
# Both reports come from the CLI, each asked for exactly one format.
grep -F -- '--json changes --base HEAD' "$log" >/dev/null
grep -E -- '^changes --base HEAD .*--markdown $' "$log" >/dev/null
grep -F -- '--exempt-tag internal --exempt-tag internal' "$log" >/dev/null
grep -F -- '--exempt-tag \#partner' "$log" >/dev/null
grep -F -- '--exempt-tag \ partner\ APIs\ ' "$log" >/dev/null
report_root="$(sed -n 's/^report-root=//p' "$output")"
test -s "$report_root/001/report.json"
test -s "$report_root/001/report.md"

# The default configuration exempts nothing, which leaves the tag loop appending no arguments at
# all. Expanding an empty array under `set -u` is fatal on bash 3.2 (GitHub's macOS runners), so
# the empty case must run the binary, not merely parse.
empty_output="$tmp/output-empty"
empty_summary="$tmp/summary-empty"
empty_log="$tmp/args-empty"
: > "$empty_output"
: > "$empty_summary"
: > "$empty_log"

GNR8_BIN="$fake" \
BASE_REF=HEAD \
EXEMPT_TAGS="" \
WORKING_DIRECTORIES="$weird_dir" \
RUNNER_TEMP="$tmp" \
GITHUB_OUTPUT="$empty_output" \
GITHUB_STEP_SUMMARY="$empty_summary" \
GITHUB_JOB=test \
FAKE_LOG="$empty_log" \
  "$runner"

grep -Fx 'gating=true' "$empty_output" >/dev/null
grep -F -- '--base HEAD' "$empty_log" >/dev/null
if grep -F -- '--exempt-tag' "$empty_log" >/dev/null; then
  echo "empty exempt-tags must not pass --exempt-tag" >&2
  exit 1
fi
# The heading is the one value this script renders, so it escapes it.
grep -F 'a&lt;b&gt;&amp;c' "$empty_summary" >/dev/null
if grep -F 'a<b>&c' "$empty_summary" >/dev/null; then
  echo "working directory reached the heading unescaped" >&2
  exit 1
fi

# Separate matrix invocations own distinct markers, each derived from its artifact name.
second_name="$(sed -n 's/^artifact-name=//p' "$empty_output")"
test "$artifact_name" != "$second_name"
grep -Fx "<!-- gnr8-api-changes:$second_name -->" "$empty_summary" >/dev/null

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

# A complete first project survives a failed second project. Only completion outputs stay absent.
: > "$output"
: > "$summary"
if GNR8_BIN="$fake" BASE_REF=HEAD FAIL_PROJECT="$weird_dir" \
  WORKING_DIRECTORIES="$repo_root/examples/bookstore"$'\n'"$weird_dir" \
  RUNNER_TEMP="$tmp" GITHUB_OUTPUT="$output" GITHUB_STEP_SUMMARY="$summary" \
  GITHUB_JOB=test FAKE_LOG="$log" "$runner" 2> "$stderr"; then
  echo "expected project 2 to fail" >&2; exit 1
fi
grep -F 'report-root=' "$output" >/dev/null
grep -F 'artifact-name=' "$output" >/dev/null
grep -F 'BREAKING  DELETE /books/{id}' "$summary" >/dev/null
! grep -E '^(combined-report|gating)=' "$output"
report_root="$(sed -n 's/^report-root=//p' "$output")"
test -s "$report_root/001/report.md"
test -s "$report_root/001/report.json"

# Keep both full artifacts when the summary budget cannot accommodate a whole project block.
: > "$output"
: > "$summary"
GNR8_BIN="$fake" BASE_REF=HEAD OVERSIZED=true \
  WORKING_DIRECTORIES="$repo_root/examples/bookstore"$'\n'"$weird_dir" \
  RUNNER_TEMP="$tmp" GITHUB_OUTPUT="$output" GITHUB_STEP_SUMMARY="$summary" \
  GITHUB_JOB=test FAKE_LOG="$log" "$runner"
grep -F 'Report truncated at 900 KiB' "$summary" >/dev/null
test "$(grep -c 'Report truncated' "$summary")" -eq 1
test "$(wc -c < "$summary")" -lt $((1024 * 1024))
grep -Fx 'gating=true' "$output" >/dev/null
report_root="$(sed -n 's/^report-root=//p' "$output")"
test "$(wc -c < "$report_root/report.md")" -gt $((1800 * 1024))
test "$(grep -c '^<!-- gnr8-api-changes:' "$report_root/report.md")" -eq 2

echo "action changes tests: OK (6 cases)"
