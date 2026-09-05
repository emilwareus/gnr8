#!/usr/bin/env bash
set -euo pipefail

# `! grep` alone is exempt from errexit, so negative assertions must fail explicitly.
assert_absent() {
  local status=0
  grep "$@" || status=$?
  if [[ "$status" -ne 1 ]]; then
    echo "expected no matching output (grep status $status)" >&2
    exit 1
  fi
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
upsert="$repo_root/scripts/upsert-action-comment.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
# The boundary fake checks the actual query shape and models marker ownership over comment bodies.
cat > "$tmp/bin/gh" <<'PY'
#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

args = sys.argv[1:]
with open(os.environ['GH_LOG'], 'a') as log:
    log.write(json.dumps(args) + '\n')
state = Path(os.environ['GH_STATE'])
comments = json.loads(state.read_text())
if os.environ.get('GH_FAIL') == 'list' or (os.environ.get('GH_FAIL') == 'write' and '--paginate' not in args):
    print('simulated permission failure', file=sys.stderr)
    sys.exit(1)
if args[:2] == ['api', '--paginate']:
    marker = os.environ['MARKER']
    query = args[args.index('--jq') + 1]
    digest = '<!-- gnr8-api-changes-digest:'
    assert query.startswith('.[] | select(.body | split("\\n") | any(. == "' + marker + '" or . == "<!-- gnr8-api-changes -->")) | [.id, (.body | startswith("' + digest)
    assert query.endswith('\\n"))] | @tsv')
    prefix = query.split('startswith("', 1)[1].split('\\n"', 1)[0] + '\n'
    assert 'user' not in query
    for comment in comments:
        if any(line in (marker, '<!-- gnr8-api-changes -->') for line in comment['body'].split('\n')):
            print(str(comment['id']) + '\t' + str(comment['body'].startswith(prefix)).lower())
else:
    if args[:3] == ['api', '--method', 'PATCH']:
        comment_id = int(args[3].rsplit('/', 1)[1])
        body = Path(args[args.index('-F') + 1].removeprefix('body=@')).read_text()
        for comment in comments:
            if comment['id'] == comment_id:
                comment['body'] = body
    elif args[:3] == ['api', '--method', 'DELETE']:
        comment_id = int(args[3].rsplit('/', 1)[1])
        comments = [c for c in comments if c['id'] != comment_id]
    elif args[:2] == ['pr', 'comment']:
        body = Path(args[args.index('--body-file') + 1]).read_text()
        comments.append({'id': 999, 'user': {'login': 'some-app[bot]'}, 'body': body})
    else:
        raise AssertionError(args)
    state.write_text(json.dumps(comments))
PY
chmod +x "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH"
export GH_LOG="$tmp/log" GH_STATE="$tmp/state"
export REPOSITORY=oaiz-io/gnr8 PR_NUMBER=85 REPORT_PATH="$tmp/report.md"
export MARKER='<!-- gnr8-api-changes:gnr8-api-changes-test-12345678 -->'
printf '%s\nreport\n' "$MARKER" > "$REPORT_PATH"

seed() {
  : > "$GH_LOG"
  python3 - "$@" <<'PY'
import json, os, sys
from pathlib import Path
marker = os.environ['MARKER']
comments = []
for index, spelling in enumerate(sys.argv[1:], 1):
    text = {'own': marker, 'old': '<!-- gnr8-api-changes -->',
            'other': '<!-- gnr8-api-changes:gnr8-api-changes-test-87654321 -->',
            'quoted': '    ' + marker}[spelling]
    comments.append({'id': index, 'user': {'login': 'some-app[bot]'}, 'body': text + '\nold report\n'})
Path(os.environ['GH_STATE']).write_text(json.dumps(comments))
PY
}

# Non-bot author and environment marker select PATCH, never append.
seed own
"$upsert"
grep -F '"PATCH", "repos/oaiz-io/gnr8/issues/comments/1"' "$GH_LOG" >/dev/null
assert_absent -F '"pr", "comment"' "$GH_LOG"
assert_absent -F 'github-actions[bot]' "$GH_LOG"

# Identical second publication lists once and performs no writes.
: > "$GH_LOG"
"$upsert"
test "$(wc -l < "$GH_LOG")" -eq 1
assert_absent -F '"PATCH"' "$GH_LOG"

# Duplicates converge on the first (oldest), without touching another key or a quoted marker.
seed own own own other quoted
"$upsert"
grep -F '"PATCH", "repos/oaiz-io/gnr8/issues/comments/1"' "$GH_LOG" >/dev/null
grep -F '"DELETE", "repos/oaiz-io/gnr8/issues/comments/2"' "$GH_LOG" >/dev/null
grep -F '"DELETE", "repos/oaiz-io/gnr8/issues/comments/3"' "$GH_LOG" >/dev/null
assert_absent -E 'issues/comments/[45]' "$GH_LOG"

# A previous bare marker is adopted for one release.
seed old
"$upsert"
grep -F '"PATCH", "repos/oaiz-io/gnr8/issues/comments/1"' "$GH_LOG" >/dev/null
assert_absent -F '"pr", "comment"' "$GH_LOG"

# Another key and source quoting our marker cannot claim this invocation.
seed other quoted
"$upsert"
grep -F '"pr", "comment", "85", "--repo", "oaiz-io/gnr8"' "$GH_LOG" >/dev/null
assert_absent -F '"PATCH"' "$GH_LOG"

# Reject a marker that could change the jq program before making any request.
seed
if MARKER='")) | .[]' "$upsert" 2> "$tmp/error"; then exit 1; fi
grep -F 'invalid API change comment marker' "$tmp/error" >/dev/null
test ! -s "$GH_LOG"

# Exercise the composite step's actual shell, including its publication preconditions.
python3 - "$repo_root/action.yml" "$tmp/comment-step" <<'PYTHON'
from pathlib import Path
import sys, textwrap
step = Path(sys.argv[1]).read_text().split('    - name: Comment API changes on pull request\n')[1].split('\n    - name: ')[0]
Path(sys.argv[2]).write_text(textwrap.dedent(step.split('      run: |\n')[1]))
PYTHON
export GITHUB_ACTION_PATH="$repo_root" ARTIFACT_NAME=gnr8-api-changes-test-12345678
seed
IS_FORK=true bash "$tmp/comment-step" > "$tmp/notice"
grep -F '::notice::gnr8 action: pull-request comments are unavailable on fork' "$tmp/notice" >/dev/null
grep -F "$ARTIFACT_NAME" "$tmp/notice" >/dev/null
test ! -s "$GH_LOG"
python3 -c 'print("x" * (60 * 1024 + 1))' > "$REPORT_PATH"
IS_FORK=false bash "$tmp/comment-step" > "$tmp/notice"
grep -F "comment exceeds gnr8's 60 KiB budget" "$tmp/notice" >/dev/null
grep -F "$ARTIFACT_NAME" "$tmp/notice" >/dev/null
test ! -s "$GH_LOG"

# Failed list requests never append, and write failures use the permission message.
seed own
if GH_FAIL=list "$upsert" 2> "$tmp/error"; then exit 1; fi
test "$(wc -l < "$GH_LOG")" -eq 1
printf '%s\nreport\n' "$MARKER" > "$REPORT_PATH"
seed own
GH_FAIL=write IS_FORK=false bash "$tmp/comment-step" > "$tmp/notice" 2> "$tmp/error"
grep -F 'permissions: pull-requests: write' "$tmp/notice" >/dev/null
grep -F "$ARTIFACT_NAME" "$tmp/notice" >/dev/null
assert_absent -F '"pr", "comment"' "$GH_LOG"

# No findings still updates the existing comment, preserving the result on the PR.
seed own
printf '%s\n    No API changes.\n' "$MARKER" > "$REPORT_PATH"
"$upsert"
grep -F '"PATCH", "repos/oaiz-io/gnr8/issues/comments/1"' "$GH_LOG" >/dev/null
grep -F 'No API changes.' "$GH_STATE" >/dev/null
assert_absent -F '"DELETE"' "$GH_LOG"

echo "action comment tests: OK (11 cases)"
