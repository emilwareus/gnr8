#!/usr/bin/env bash
# Run one API change report per configured project while preserving exit 1 for the final gate step.
set -euo pipefail

: "${GNR8_BIN:?GNR8_BIN is required}"
: "${BASE_REF:?BASE_REF is required}"
: "${WORKING_DIRECTORIES:?WORKING_DIRECTORIES is required}"
: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"
: "${GITHUB_STEP_SUMMARY:?GITHUB_STEP_SUMMARY is required}"

# The per-project heading is the only report text this script writes; everything under it comes from
# `gnr8 changes --markdown`, which is the one renderer for that format. The directory lands in a
# Markdown document outside a code block, so it is escaped the way the CLI escapes what it puts there.
escape_html() {
  printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' \
    -e 's/"/\&quot;/g' -e "s/'/\&#x27;/g"
}

# Run one report for $dir into $1 and hand back the gate status. Only 0 and 1 are gate answers; any
# other status is a failed analysis and stops the whole step.
report() {
  local destination="$1"
  shift
  local status=0
  (cd "$dir" && "$GNR8_BIN" "$@") > "$destination" 2> "$stderr" || status=$?
  if [[ -s "$stderr" ]]; then
    cat "$stderr" >&2
  fi
  if [[ "$status" -ne 0 && "$status" -ne 1 ]]; then
    echo "gnr8 action: change analysis failed for '$dir' with status $status" >&2
    exit "$status"
  fi
  return "$status"
}

report_root="$(mktemp -d "$RUNNER_TEMP/gnr8-api-changes.XXXXXXXX")"
# Intermediates live outside report_root, which is uploaded verbatim as the run's artifact.
work_root="$(mktemp -d "$RUNNER_TEMP/gnr8-api-changes-work.XXXXXXXX")"
combined="$report_root/report.md"
printf '# gnr8 API changes\n\n' > "$combined"

dirs=()
while IFS= read -r dir || [[ -n "$dir" ]]; do
  dir="${dir#"${dir%%[![:space:]]*}"}"
  dir="${dir%"${dir##*[![:space:]]}"}"
  [[ -z "$dir" || "$dir" == \#* ]] && continue
  dirs+=("$dir")
done <<< "$WORKING_DIRECTORIES"

if [[ "${#dirs[@]}" -eq 0 ]]; then
  echo "gnr8 action: no working directories configured" >&2
  exit 2
fi

# Seeded with the argv both reports share so the array is never empty: expanding "${empty[@]}" under
# `set -u` is an unbound-variable error in bash 3.2, the bash GitHub's macOS runners provide, and the
# default configuration (no exempt-tags) appends nothing to it.
change_args=(changes --base "$BASE_REF")
while IFS= read -r tag || [[ -n "$tag" ]]; do
  # Tag matching is exact. Empty lines separate values; every byte on a non-empty line belongs to
  # the OpenAPI tag, including leading/trailing spaces and a leading '#'. The CLI performs the one
  # canonical validity check (blank-only and multiline values are invalid).
  [[ -z "$tag" ]] && continue
  change_args+=(--exempt-tag "$tag")
done <<< "${EXEMPT_TAGS:-}"

gating=false
index=0
for dir in "${dirs[@]}"; do
  if ! (cd "$dir" && git rev-parse --verify --quiet --end-of-options "${BASE_REF}^{commit}" >/dev/null); then
    echo "gnr8 action: base ref '$BASE_REF' is unavailable from '$dir'; checkout with fetch-depth: 0 and ensure the ref exists" >&2
    exit 2
  fi

  index=$((index + 1))
  project_root="$report_root/$(printf '%03d' "$index")"
  mkdir -p "$project_root"
  json="$project_root/report.json"
  markdown="$project_root/report.md"
  body="$work_root/$(printf '%03d' "$index").md"
  stderr="$project_root/stderr.txt"

  echo "::group::gnr8 changes $dir"
  # Each format is rendered by the CLI. The Markdown report is a second invocation rather than a
  # second implementation of the format here: `changes` is deterministic and this run reads the
  # cache the first one just filled, so it costs a fraction of it and no formatting lives here.
  status=0
  report "$json" --json "${change_args[@]}" || status=$?
  body_status=0
  report "$body" "${change_args[@]}" --markdown || body_status=$?
  if [[ "$status" -ne "$body_status" ]]; then
    echo "gnr8 action: '$dir' reported gate status $status as JSON and $body_status as Markdown" >&2
    exit 2
  fi
  if [[ "$status" -eq 1 ]]; then
    gating=true
  fi

  {
    printf '<!-- gnr8-api-changes -->\n'
    printf '## API changes for %s\n\n' "$(escape_html "$dir")"
    cat "$body"
  } > "$markdown"

  cat "$markdown" >> "$combined"
  echo "::endgroup::"
done

cat "$combined" >> "$GITHUB_STEP_SUMMARY"
artifact_suffix="$(printf '%s\n%s\n' "$WORKING_DIRECTORIES" "$BASE_REF" | git hash-object --stdin | cut -c1-8)"
{
  echo "gating=$gating"
  echo "report-root=$report_root"
  echo "combined-report=$combined"
  echo "artifact-name=gnr8-api-changes-${GITHUB_JOB:-job}-$artifact_suffix"
} >> "$GITHUB_OUTPUT"
