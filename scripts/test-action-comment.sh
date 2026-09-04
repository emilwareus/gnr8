#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
upsert="$repo_root/scripts/upsert-action-comment.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fake_bin="$tmp/bin"
mkdir -p "$fake_bin"
cat > "$fake_bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%q ' "$@" >> "$GH_LOG"
printf '\n' >> "$GH_LOG"
if [[ "$1" == "api" && "$2" == "--paginate" && -n "${EXISTING_COMMENT_ID:-}" ]]; then
  printf '%s\n' "$EXISTING_COMMENT_ID"
fi
SH
chmod +x "$fake_bin/gh"

report="$tmp/report.md"
printf '<!-- gnr8-api-changes -->\nreport\n' > "$report"
log="$tmp/gh.log"
: > "$log"

PATH="$fake_bin:$PATH" \
GH_LOG="$log" \
EXISTING_COMMENT_ID=314 \
REPOSITORY=oaiz-io/gnr8 \
PR_NUMBER=82 \
REPORT_PATH="$report" \
  "$upsert"

grep -F 'repos/oaiz-io/gnr8/issues/comments/314' "$log" >/dev/null
grep -F "body=@$report" "$log" >/dev/null
if grep -F 'pr comment' "$log" >/dev/null; then
  echo "updated comment path unexpectedly created a comment" >&2
  exit 1
fi

: > "$log"
PATH="$fake_bin:$PATH" \
GH_LOG="$log" \
REPOSITORY=oaiz-io/gnr8 \
PR_NUMBER=82 \
REPORT_PATH="$report" \
  "$upsert"

grep -F 'pr comment 82 --repo oaiz-io/gnr8' "$log" >/dev/null
grep -F -- "--body-file $report" "$log" >/dev/null
grep -F 'github-actions\[bot\]' "$log" >/dev/null

echo "action comment tests: OK"
