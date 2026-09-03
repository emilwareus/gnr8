#!/usr/bin/env bash
# Run one API change report per configured project while preserving exit 1 for the final gate step.
set -euo pipefail

: "${GNR8_BIN:?GNR8_BIN is required}"
: "${BASE_REF:?BASE_REF is required}"
: "${WORKING_DIRECTORIES:?WORKING_DIRECTORIES is required}"
: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"
: "${GITHUB_STEP_SUMMARY:?GITHUB_STEP_SUMMARY is required}"

report_root="$(mktemp -d "$RUNNER_TEMP/gnr8-api-changes.XXXXXXXX")"
combined="$report_root/report.md"
printf '# gnr8 API changes\n\n' > "$combined"

dirs=()
while IFS= read -r dir || [[ -n "$dir" ]]; do
  dir="${dir#"${dir%%[![:space:]]*}"}"
  dir="${dir%"${dir##*[![:space:]]}"}"
  [[ -z "$dir" || "$dir" == \#* ]] && continue
  dirs+=("$dir")
done <<< "$WORKING_DIRECTORIES"

change_args=()
while IFS= read -r tag || [[ -n "$tag" ]]; do
  tag="${tag#"${tag%%[![:space:]]*}"}"
  tag="${tag%"${tag##*[![:space:]]}"}"
  [[ -z "$tag" || "$tag" == \#* ]] && continue
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
  stderr="$project_root/stderr.txt"

  echo "::group::gnr8 changes $dir"
  set +e
  (cd "$dir" && "$GNR8_BIN" --json changes --base "$BASE_REF" "${change_args[@]}") \
    > "$json" 2> "$stderr"
  status=$?
  set -e
  if [[ -s "$stderr" ]]; then
    cat "$stderr" >&2
  fi
  if [[ "$status" -ne 0 && "$status" -ne 1 ]]; then
    echo "gnr8 action: change analysis failed for '$dir' with status $status" >&2
    exit "$status"
  fi
  if [[ "$status" -eq 1 ]]; then
    gating=true
  fi

  python3 - "$json" "$markdown" "$dir" <<'PY'
import json
import html
import pathlib
import sys

source, destination, project = sys.argv[1:]
report = json.loads(pathlib.Path(source).read_text(encoding="utf-8"))
summary = report["summary"]
tags = report["policy"]["exempt_tags"]
def one_line(value):
    return " ".join(str(value).splitlines())

lines = ["<!-- gnr8-api-changes -->", f"## API changes for {html.escape(one_line(project))}", ""]
lines.append(
    "Base: <code>{}</code> → <code>{}</code>".format(
        html.escape(one_line(report["base"]["ref"])),
        html.escape(one_line(report["base"]["resolved"])),
    )
)
tag_list = ", ".join(f"<code>{html.escape(one_line(tag))}</code>" for tag in tags)
lines.extend(["", "Exempt tags: " + (tag_list or "none"), ""])
lines.append(
    "Summary: {} breaking, {} additive, {} doc-only, {} gating.".format(
        summary["breaking"], summary["additive"], summary["doc_only"], summary["gating"]
    )
)
lines.append("")
if not report["changes"]:
    lines.append("    No API changes.")
for change in report["changes"]:
    kind = change["kind"].upper().replace("_", "-")
    operation = change.get("operation", "-")
    suffix = ""
    if kind == "BREAKING" and not change["gating"]:
        base = change["exempt"]["base"]
        current = change["exempt"]["current"]
        if base is True and current is True:
            suffix = "  (exempt on both sides; not gating)"
        elif base is True and current is None:
            suffix = "  (exempt on base side; not gating)"
        elif base is None and current is True:
            suffix = "  (exempt on current side; not gating)"
    safe_operation = one_line(operation)
    safe_message = one_line(change["message"])
    lines.append(f"    {kind:<9} {safe_operation:<19} {safe_message}{suffix}")
    affected = {
        (item["operation_id"], item["operation"])
        for side in ("base", "current")
        for item in (change.get("affected_operations", {}).get(side) or [])
    }
    if affected:
        rendered = ", ".join(
            f"{one_line(operation_id)} ({one_line(operation)})"
            for operation_id, operation in sorted(affected)
        )
        lines.append(f"        SDK operations: {rendered}")
    if change.get("file"):
        location = one_line(change["file"])
        if change.get("line") is not None:
            location += f":{change['line']}"
        lines.append(f"        Source: {location}")
lines.append("")
pathlib.Path(destination).write_text("\n".join(lines), encoding="utf-8")
PY

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
