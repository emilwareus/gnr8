#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
resolver="$repo_root/scripts/resolve-action-version.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
workspace_version="$(awk '
  /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version = "/ {
    value = $0
    sub(/^version = "/, "", value)
    sub(/"$/, "", value)
    print value
    exit
  }
' "$repo_root/Cargo.toml")"
if [[ -z "$workspace_version" ]]; then
  echo "failed to read workspace package version" >&2
  exit 1
fi

assert_resolves() {
  local requested="$1"
  local directories="$2"
  local expected="$3"
  local output="$tmp/github-output"
  : > "$output"
  GITHUB_OUTPUT="$output" \
    REQUESTED_VERSION="$requested" \
    WORKING_DIRECTORIES="$directories" \
    "$resolver" >/dev/null
  grep -Fx "version=$expected" "$output" >/dev/null
  grep -Fx "tag=v$expected" "$output" >/dev/null
}

assert_fails() {
  local requested="$1"
  local directories="$2"
  local expected_message="$3"
  local output="$tmp/github-output"
  local stderr="$tmp/stderr"
  : > "$output"
  if GITHUB_OUTPUT="$output" \
    REQUESTED_VERSION="$requested" \
    WORKING_DIRECTORIES="$directories" \
    "$resolver" >/dev/null 2>"$stderr"; then
    echo "expected action version resolution to fail" >&2
    exit 1
  fi
  grep -F "$expected_message" "$stderr" >/dev/null
}

write_package() {
  local dir="$1"
  local name="$2"
  local version="$3"
  mkdir -p "$dir/src"
  printf '[package]\nname = "%s"\nversion = "%s"\nedition = "2021"\n' \
    "$name" "$version" > "$dir/Cargo.toml"
  printf 'pub fn marker() {}\n' > "$dir/src/lib.rs"
}

assert_resolves "lock" "$repo_root/examples/bookstore" "$workspace_version"
assert_fails "v999.999.999" "$repo_root/examples/bookstore" "requested gnr8 999.999.999"

write_package "$tmp/direct" "gnr8" "1.2.3+build.7"
write_package "$tmp/transitive" "gnr8" "9.9.9"
write_package "$tmp/helper" "action-helper" "0.1.0"
printf '\n[dependencies]\ntransitive-gnr8 = { package = "gnr8", path = "../transitive" }\n' \
  >> "$tmp/helper/Cargo.toml"

mkdir -p "$tmp/project/.gnr8/src"
printf '%s\n' \
  '[package]' \
  'name = "action-version-fixture"' \
  'version = "0.1.0"' \
  'edition = "2021"' \
  '' \
  '[dependencies]' \
  'gnr8 = { path = "../../direct" }' \
  'action-helper = { path = "../../helper" }' \
  > "$tmp/project/.gnr8/Cargo.toml"
printf 'fn main() {}\n' > "$tmp/project/.gnr8/src/main.rs"
cargo generate-lockfile --manifest-path "$tmp/project/.gnr8/Cargo.toml" --quiet

assert_resolves "lock" "$tmp/project" "1.2.3+build.7"
assert_resolves "v1.2.3+build.7" "$tmp/project" "1.2.3+build.7"

mkdir -p "$tmp/other-project/.gnr8/src"
printf '%s\n' \
  '[package]' \
  'name = "action-other-version-fixture"' \
  'version = "0.1.0"' \
  'edition = "2021"' \
  '' \
  '[dependencies]' \
  'gnr8 = { path = "../../transitive" }' \
  > "$tmp/other-project/.gnr8/Cargo.toml"
printf 'fn main() {}\n' > "$tmp/other-project/.gnr8/src/main.rs"
cargo generate-lockfile --manifest-path "$tmp/other-project/.gnr8/Cargo.toml" --quiet
assert_fails \
  "lock" \
  "$tmp/project
$tmp/other-project" \
  "generation crates pin different gnr8 versions"

mkdir -p "$tmp/transitive-only/.gnr8/src"
printf '%s\n' \
  '[package]' \
  'name = "action-transitive-fixture"' \
  'version = "0.1.0"' \
  'edition = "2021"' \
  '' \
  '[dependencies]' \
  'action-helper = { path = "../../helper" }' \
  > "$tmp/transitive-only/.gnr8/Cargo.toml"
printf 'fn main() {}\n' > "$tmp/transitive-only/.gnr8/src/main.rs"
cargo generate-lockfile --manifest-path "$tmp/transitive-only/.gnr8/Cargo.toml" --quiet
assert_fails "lock" "$tmp/transitive-only" "has no direct normal dependency on gnr8"

echo "action version tests: OK"
