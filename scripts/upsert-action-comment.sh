#!/usr/bin/env bash
# Create or update this Action's marker-owned pull-request comment.
set -euo pipefail

: "${REPOSITORY:?REPOSITORY is required}"
: "${PR_NUMBER:?PR_NUMBER is required}"
: "${REPORT_PATH:?REPORT_PATH is required}"
: "${MARKER:?MARKER is required}"

# Only our generated key can enter the jq expression. This is comment identity, never source text.
if [[ ! "$MARKER" =~ ^\<\!--\ gnr8-api-changes:gnr8-api-changes-[a-zA-Z0-9_-]+-[0-9a-f]{8}\ --\>$ ]]; then
  echo "gnr8 action: invalid API change comment marker" >&2
  exit 2
fi

digest="$(git hash-object --no-filters -- "$REPORT_PATH")"
digest_marker="<!-- gnr8-api-changes-digest:$digest -->"
body="$(mktemp)"
trap 'rm -f "$body"' EXIT
{ printf '%s\n' "$digest_marker"; cat "$REPORT_PATH"; } > "$body"

# TODO: Remove the old bare marker match after one release of keyed comments.
# Match whole marker lines, so a marker quoted by an indented finding cannot own a comment. Both the
# ownership match and the digest guard read the same CR-trimmed lines: GitHub stores a body edited
# through the web UI with CRLF endings, and a guard that only accepted LF would rewrite every run.
comments="$(gh api --paginate "repos/$REPOSITORY/issues/$PR_NUMBER/comments" \
  --jq ".[] | (.body | split(\"\\n\") | map(rtrimstr(\"\\r\"))) as \$lines | select(\$lines | any(. == \"$MARKER\" or . == \"<!-- gnr8-api-changes -->\")) | [.id, (\$lines[0] == \"$digest_marker\")] | @tsv")"

first=true
while IFS=$'\t' read -r comment_id unchanged; do
  [[ -z "$comment_id" ]] && continue
  if [[ "$first" == true ]]; then
    if [[ "$unchanged" != true ]]; then
      gh api --method PATCH "repos/$REPOSITORY/issues/comments/$comment_id" \
        -F "body=@$body" --silent
    fi
    first=false
  else
    gh api --method DELETE "repos/$REPOSITORY/issues/comments/$comment_id" --silent
  fi
done <<< "$comments"

if [[ "$first" == true ]]; then
  gh pr comment "$PR_NUMBER" --repo "$REPOSITORY" --body-file "$body"
fi
