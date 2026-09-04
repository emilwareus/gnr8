#!/usr/bin/env bash
# Create or update this Action's marker-owned pull-request comment.
set -euo pipefail

: "${REPOSITORY:?REPOSITORY is required}"
: "${PR_NUMBER:?PR_NUMBER is required}"
: "${REPORT_PATH:?REPORT_PATH is required}"

comment_id="$(
  gh api --paginate "repos/$REPOSITORY/issues/$PR_NUMBER/comments" \
    --jq '.[] | select(.user.login == "github-actions[bot]" and (.body | contains("<!-- gnr8-api-changes -->"))) | .id' \
    | tail -n 1
)"

if [[ -n "$comment_id" ]]; then
  gh api --method PATCH "repos/$REPOSITORY/issues/comments/$comment_id" \
    -F "body=@$REPORT_PATH" --silent
else
  gh pr comment "$PR_NUMBER" --repo "$REPOSITORY" --body-file "$REPORT_PATH"
fi
