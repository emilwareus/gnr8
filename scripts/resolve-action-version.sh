#!/usr/bin/env bash
set -euo pipefail

requested_version="${REQUESTED_VERSION:-lock}"
working_directories="${WORKING_DIRECTORIES:-.}"

if [[ -z "$requested_version" || "$requested_version" == "latest" ]]; then
  echo 'gnr8 action: version must be an exact release or "lock"; "latest" is not allowed for checks' >&2
  exit 2
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo 'gnr8 action: cargo is required to resolve the direct gnr8 dependency from .gnr8/Cargo.lock' >&2
  exit 2
fi

locked_version() {
  local dir="$1"
  local manifest="$dir/.gnr8/Cargo.toml"
  local lock="$dir/.gnr8/Cargo.lock"
  local tree

  if [[ ! -f "$manifest" ]]; then
    echo "gnr8 action: missing $manifest" >&2
    return 2
  fi
  if [[ ! -f "$lock" ]]; then
    echo "gnr8 action: missing $lock; commit a locked generation crate" >&2
    return 2
  fi
  if ! tree="$(cargo tree \
    --locked \
    --manifest-path "$manifest" \
    --depth 1 \
    --edges normal \
    --prefix depth 2>&1)"; then
    echo "gnr8 action: failed to read the locked dependency graph for $manifest" >&2
    printf '%s\n' "$tree" >&2
    return 2
  fi

  local versions=()
  while IFS= read -r version; do
    [[ -n "$version" ]] && versions+=("$version")
  done < <(
    awk '$1 == "1gnr8" && $2 ~ /^v/ { sub(/^v/, "", $2); print $2 }' <<< "$tree" |
      LC_ALL=C sort -u
  )

  if [[ "${#versions[@]}" -eq 0 ]]; then
    echo "gnr8 action: $manifest has no direct normal dependency on gnr8" >&2
    return 2
  fi
  if [[ "${#versions[@]}" -ne 1 ]]; then
    echo "gnr8 action: $manifest resolves multiple direct gnr8 versions: ${versions[*]}" >&2
    return 2
  fi
  printf '%s\n' "${versions[0]}"
}

requested="${requested_version#v}"
resolved=""
directory_count=0
while IFS= read -r dir || [[ -n "$dir" ]]; do
  dir="${dir#"${dir%%[![:space:]]*}"}"
  dir="${dir%"${dir##*[![:space:]]}"}"
  [[ -z "$dir" || "$dir" == \#* ]] && continue
  directory_count=$((directory_count + 1))
  locked="$(locked_version "$dir")"
  if [[ -z "$resolved" ]]; then
    resolved="$locked"
  elif [[ "$resolved" != "$locked" ]]; then
    echo "gnr8 action: generation crates pin different gnr8 versions ($resolved and $locked)" >&2
    exit 2
  fi
done <<< "$working_directories"

if [[ "$directory_count" -eq 0 ]]; then
  echo "gnr8 action: no working directories configured" >&2
  exit 2
fi
if [[ "$requested_version" != "lock" && "$requested" != "$resolved" ]]; then
  echo "gnr8 action: requested gnr8 $requested but .gnr8/Cargo.lock pins $resolved" >&2
  exit 2
fi
if [[ ! "$resolved" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?(\+[0-9A-Za-z][0-9A-Za-z.-]*)?$ ]]; then
  echo "gnr8 action: lockfile gnr8 version is not an exact release: $resolved" >&2
  exit 2
fi
if [[ -z "${GITHUB_OUTPUT:-}" ]]; then
  echo "gnr8 action: GITHUB_OUTPUT is required" >&2
  exit 2
fi

printf 'version=%s\ntag=v%s\n' "$resolved" "$resolved" >> "$GITHUB_OUTPUT"
echo "gnr8 action: resolved exact gnr8 version $resolved"
