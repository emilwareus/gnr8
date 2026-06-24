#!/usr/bin/env bash
#
# bench.sh — honest wall-clock benchmark for gnr8's three HARD-03 / WATCH-03 scenarios.
#
# Produces the cold / warm-no-op / single-file-edit latency numbers by driving the REAL release binary
# end-to-end on a SCRATCH copy of the goalservice fixture. Nothing committed is mutated: the fixture is
# copied into a `mktemp -d` working dir that is removed on exit (Pitfall 2 — `fixtures/goalservice` has
# an `expected/` golden dir and is a CI-gated Go module + the default `inspect` target, so it must NEVER
# be `init`/`generate`'d in place).
#
# The three scenarios:
#   1. COLD             — the first `generate` (empty manifest → every output written). The cold number
#                         is dominated by the `go run` (goextract helper compile) + `go build`/`gofmt`
#                         subprocess cost, NOT gnr8 itself.
#   2. WARM-NO-OP       — an immediate second `generate` (manifest hits → 0 files written; the WATCH-01
#                         no-op path).
#   3. SINGLE-FILE-EDIT — append one field to a DTO struct, then `generate` (the watch single-edit path).
#
# The numbers are REPRESENTATIVE and environment-dependent — never asserted as exact thresholds
# (Pitfall 3). Timing is external wall-clock (`date +%s%N`) so it includes process startup + the Go
# subprocess cost (the honest end-to-end number). No `jq` dependency.

set -euo pipefail

# Resolve the repo root from this script's own location (works regardless of the caller's cwd).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"

# A throwaway working dir, removed on ANY exit (success, error, or interrupt) so committed state is
# never touched and no scratch artifacts leak.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Copy the fixture into the scratch dir — NEVER operate in the committed fixture (Pitfall 2).
cp -R "$REPO/fixtures/goalservice" "$WORK/svc"

# Build the real release binary once (the thing the benchmark measures).
echo "building release binary (cargo build --release -p gnr8) ..." >&2
cargo build --release -p gnr8 --manifest-path "$REPO/Cargo.toml" >&2
GNR8="$REPO/target/release/gnr8"

cd "$WORK/svc"

# Scaffold the project-local .gnr8/ workspace in the scratch copy (idempotent).
"$GNR8" init >/dev/null

# Time one `gnr8 generate` end-to-end and echo the elapsed milliseconds. External wall-clock around the
# real subprocess so the number reflects the honest end-to-end cost (Go subprocesses included).
time_generate_ms() {
    local t0 t1
    t0=$(date +%s%N)
    "$GNR8" generate >/dev/null 2>&1
    t1=$(date +%s%N)
    echo $(( (t1 - t0) / 1000000 ))
}

# 1. COLD — first generate (empty manifest → everything written).
cold_ms=$(time_generate_ms)

# 2. WARM-NO-OP — immediate re-generate (manifest hits → 0 written; WATCH-01 no-op path).
warm_ms=$(time_generate_ms)

# 3. SINGLE-FILE-EDIT — append a benchmark field to the CreateGoalInput DTO struct, then generate.
#    A portable in-place edit via awk (no `sed -i` portability differences between BSD/GNU): insert a new
#    field line immediately after the struct opening brace. Operates ONLY on the scratch copy.
GOAL_GO="internal/common/dto/goal.go"
awk '
    /^type CreateGoalInput struct \{/ && !done {
        print
        print "\tBenchField       string             `json:\"benchField,omitempty\"`"
        done = 1
        next
    }
    { print }
' "$GOAL_GO" > "$GOAL_GO.tmp" && mv "$GOAL_GO.tmp" "$GOAL_GO"

edit_ms=$(time_generate_ms)

# The single labeled result line (the contract the verifier greps for) + an honesty note.
echo "cold=${cold_ms}ms warm-no-op=${warm_ms}ms single-file-edit=${edit_ms}ms"
echo "note: numbers are environment-dependent and reproducible via scripts/bench.sh; the cold number is dominated by the go run / go build / gofmt subprocess cost, not gnr8 itself." >&2
