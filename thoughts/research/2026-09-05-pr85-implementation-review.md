# Adversarial review: PR #85 (implementation of #76)

Date: 2026-09-05 · Branch `research/pr-change-reports` → `origin/main` @ `78c89a8` · 10 commits,
~2400 insertions · Reviewer: independent (Claude Opus 5), implementer: Codex.

Contract reviewed against: `thoughts/research/2026-09-05-pr-change-reports.md` §5 (workstreams A–F),
§5.7 (tests), §5.9 (must not change), §5.10 (out of scope), §6 (rejected alternatives). Nothing below
is taken from the implementer's own report; every claim was re-derived from the diff, from the files
read in full, or from an experiment run on this machine.

---

## 1. Verdict summary

The PR delivers all six workstreams. The containment story is genuinely sound — I attacked the
workflow-command encoder with bare CR, CRLF, `%`, `::`, `,`, `=`, NUL, U+2028 and control-character
payloads and could not construct an escape. The gate is independent of every publication surface.
The CLI/GitHub boundary holds: nothing under `crates/` knows GitHub exists, and only
`crates/gnr8/src/changes.rs` was touched there at all.

One real defect was found and fixed: the CRLF normalization added in `d269037` was applied to the
comment-ownership match but not to the body-digest guard in the same jq program, leaving the
write-avoidance guard required by §5.1 A2 inert on exactly the bodies that fix was written for.

**Gates: `cargo test --workspace --locked` and `make check` both pass, before and after the fix.**

---

## 2. Findings

| # | Sev | File:line | Finding | Status |
|---|-----|-----------|---------|--------|
| F1 | **MED** | `scripts/upsert-action-comment.sh:27` (pre-fix `:25`) | Digest guard compared the raw body with `startswith("<digest marker>\n")` while the ownership match on the same line read CR-trimmed lines. A body GitHub stored with CRLF endings never matched, so the guard reported "changed" and every run issued a needless PATCH. | **fixed** `fcb734c` |
| F2 | LOW | `docs/operations/artifacts-and-ci.md:312-316` | Comment-identity docs said matrix jobs own different comments without stating that the key is only job id + `working-directories` + `base-ref`. A matrix varying any other dimension shares one comment and can transiently duplicate. | **fixed** `fcb734c` |
| F3 | LOW | `scripts/emit-action-annotations.py:52`; `crates/gnr8-core/src/changes/diff.rs:237` | `Change.file` is copied from the span with no `graph::is_module_relative` filter, and `relativize` (`crates/gnr8-sdk/src/graph.rs:1152-1161`) deliberately leaves out-of-module paths absolute. `os.path.join` then drops `project_dir` for an absolute `file`, and `normpath` can resolve a `../` provenance onto an unrelated in-tree file — a misplaced annotation, not just a dropped one. | **wontfix** — this is Open Q 3 of the contract, decided deliberately and documented at `artifacts-and-ci.md:345-351`. Worth noting the contract's phrasing ("degrades to *no annotation*, never to a wrong one") is slightly optimistic for the `../` case. |
| F4 | LOW | `action.yml` (`working-directories` accepts absolute paths); `emit-action-annotations.py:52` | An absolute `working-directories` (e.g. `${{ github.workspace }}/services/books`) makes every `file=` absolute, so GitHub silently drops every annotation with no warning. Same class as F3. | **wontfix** — same open question; no fallback path search is permitted (rule 3), and the only honest fix is a graph-level workspace-relative path. |
| F5 | LOW | `scripts/run-action-changes.sh:46-49` | `cat "$stderr" >&2` re-emits the CLI's stderr into the runner log unfiltered, so a diagnostic beginning a line with `::` (or containing a bare CR followed by `::`) would be parsed as a workflow command. | **wontfix** — pre-existing (unchanged by this PR), and inherent to any Action that runs a compiler: the `.gnr8` crate build's own output passes through the same log. Not a regression. |
| F6 | LOW | `crates/gnr8/src/changes.rs:167-172` (headings) | The group partition is a hard-coded 4-tuple array, not an exhaustive `match`. A fifth `ChangeKind` would be silently dropped from the Markdown report. | **wontfix** — `kind_label` (`:265-271`) is an exhaustive match, so a new variant breaks the build in the same file. Adequate tripwire; noted for maintenance. |
| F7 | INFO | `scripts/emit-action-annotations.py:32-41` | `doc_only` findings are `continue`d before any counter, so they are excluded from the "further findings not annotated" total. Correct by design (§5.2 B1), but `artifacts-and-ci.md:342-344` reads as if the notice covers every omission. | **no change** — the count that matters (capped + unanchorable) is right; rewording risks implying doc-only findings were suppressed rather than never eligible. |
| F8 | INFO | `scripts/upsert-action-comment.sh:16-17,33` | The guard trusts the stored digest marker line, so a collaborator who edits the comment body below line 1 while the report is unchanged keeps their edit — we skip the PATCH rather than restoring the report. | **no change** — this is literally what §5.1 A2 specified (Bump.sh's stored-digest precedent). Requires repo write access to exploit; recorded so the trade is explicit. |

Nothing rose to HIGH. No blocking issue remains.

---

## 3. Per-question verdicts

### Q1 — Injection / containment · **PASS**

*Annotations.* `encode_data` (`emit-action-annotations.py:16-18`) replaces `%` first, then `\r`→`%0D`,
`\n`→`%0A`; `encode_property` (`:21-23`) additionally escapes `:` and `,`. That is exactly GitHub's
documented rule set, and the ordering is the one that survives a literal `%0A` in the source (it
becomes `%250A`, decoding back to the literal text rather than to a newline).

I ran the encoder against 15 hostile inputs. Results, all single-line, all contained:

| Input | Emitted |
|---|---|
| `message = "ok\rEVIL::error file=x::pwn"` | `…::ok%0DEVIL::error file=x::pwn` — the CR is encoded, so the runner (whose stdout reader treats a bare `\r` as a line terminator) cannot be made to see a second command |
| `message = "ok\r\n::error::pwn"` | `…::ok%0D%0A::error::pwn` |
| `message = "%0A%0D%25"` | `…::%250A%250D%2525` — literals preserved, not re-decoded |
| `file = "a\rb.go"` | `file=examples/bookstore/a%0Db.go` |
| `code = "c\r::error x=1"` | `title=gnr8%3A c%0D%3A%3Aerror x=1` — `:` escaped, so no property or command boundary can be forged |
| `file = "a=b,c=d.go"` | `file=examples/bookstore/a=b%2Cc=d.go` — `,` escaped; `=` need not be, the runner splits each pair on the first `=` only |
| U+2028 / NUL in message | passed through, harmless — the runner's line reader splits on `\r`/`\n` only |

The one structural subtlety I checked by hand: `title` carries a literal space (`gnr8: <code>`). The
runner takes the command name as everything before the *first* space and the whole remainder as the
property string, splitting only on `,` and `=`, so an embedded space cannot promote data into a new
property. The repo's own `test_hostile_values_cannot_add_commands_or_properties`
(`scripts/test_emit_action_annotations.py:63-70`) asserts the one-line invariant directly.

Non-string values (`message`, `code`, `span`, a list-shaped `change`) all raise
`TypeError`/`AttributeError`, which `main()` catches (`:81-85`) and reports without echoing the JSON
or the exception text — the right call, since the exception text would itself be analyzed source.

*Comment body.* Findings stay inside a 4-space-indented code block and every value outside it is
HTML-escaped (`crates/gnr8/src/changes.rs:230-263`), unchanged by this PR. The new group headings are
compile-time `&'static str` plus an integer (`:167-172`) — nothing from the graph reaches them. The
Action's one contribution to the document, the per-project heading, escapes `$dir` *and* now flattens
`\r`/`\n` first (`run-action-changes.sh:15-21`), and the `::group::` line no longer interpolates
`$dir` at all (`:128`) — that is a real injection fix, covered by the control-character case at
`scripts/test-action-changes.sh:287-298`.

*Containment across group boundaries.* `markdown_report_cannot_be_broken_out_of_by_analyzed_source`
was relaxed to allow empty lines (`changes.rs:608-612`), which alone would be a weakening. It is
compensated by `markdown_report_partitions_all_four_groups_in_stable_order` (`:638-702`), which feeds
five findings with `\n`-bearing messages and a ` ``` `-bearing code and asserts that the *only*
unindented non-empty lines after the header are the four static headings. That is the stronger
assertion and it does cover the boundaries. Empty lines cannot be attacker-produced anyway: `one_line`
collapses newlines and every finding line carries a 4- or 8-space prefix from the format string.

### Q2 — Marker ownership · **PASS with a documented residual**

Key: `<!-- gnr8-api-changes:gnr8-api-changes-<job>-<8 hex> -->` (`run-action-changes.sh:60-62`), one
derivation feeding both the artifact name and the marker. The comment script re-validates it against
`^<!-- gnr8-api-changes:gnr8-api-changes-[a-zA-Z0-9_-]+-[0-9a-f]{8} -->$` before it can enter the jq
program (`upsert-action-comment.sh:11`), so only our own generated key is ever interpolated. GitHub's
job-id grammar is a subset of that character class, so the guard never spuriously rejects.

*Duplicate collapse is idempotent.* Verified by running the real script twice against a seeded state
of three matching comments: run 1 = PATCH id 1, DELETE ids 2 and 3; run 2 = list only; final state one
comment. Oldest wins, and `--paginate` preserves ascending-id order across pages.

*CRLF.* The `rtrimstr("\r")` in the ownership match matches what GitHub returns for a body written
through the web UI (HTML form submission normalizes a textarea to CRLF). I confirmed with real `jq`
that the *digest* half did **not** tolerate it — see F1 — and fixed it. Post-fix, a CRLF-stored body
with an unchanged report produces the list call and no write.

*Residual race.* Two invocations sharing a key (a matrix that varies something outside
`working-directories`/`base-ref`) still have a list→PATCH window and can transiently create two
comments. The design is self-healing rather than race-free — the next run collapses them — which the
contract accepted. The docs did not state the limit; F2 fixes that.

*Threat of a planted marker.* Anyone can post a comment carrying the bare `<!-- gnr8-api-changes -->`
and have us adopt it. Worst case is that we overwrite the attacker's own text, which §5.1 A2 weighed
explicitly and which is the same call `bufbuild/buf-action` makes. The bare match is `TODO`-scoped to
one release (`upsert-action-comment.sh:22`).

### Q3 — Budgets · **PASS**

900 KiB summary (`run-action-changes.sh:73`), whole project blocks only, checked as
`summary_bytes + project_bytes <= 921600` *before* appending. No off-by-one: the worst case is
921600 bytes plus a ~140-byte truncation notice, against a 1 MiB limit. `summary_bytes` is seeded from
`wc -c` of the whole existing summary file (`:76`), so anything a previous step wrote is counted —
conservative in the right direction. Units are bytes on both sides, matching GitHub's file-size limit.

Truncation cannot split a code block: only complete project blocks are appended, and the notice is a
static, unindented paragraph appended after the CLI's trailing blank line, so it opens a new Markdown
block rather than breaking containment. `summary_stopped` then suppresses every later append, so the
notice appears exactly once (asserted at `test-action-changes.sh:207`) and the artifact keeps
everything (`:211-212` asserts a >1800 KiB `report.md` with both project markers intact).

60 KiB comment budget (`action.yml:350`) is measured on the report file; the body adds only the
~60-byte digest marker. Counting **bytes** against a character-denominated platform limit errs safe
for multibyte UTF-8, which is the correct direction.

### Q4 — Gate independence · **PASS**

Every publication failure is swallowed into a warning: summary write (`run-action-changes.sh:35-38`,
`:154-165`), annotations (`:169-173`), comment (`action.yml:352-354`). The gate step
(`action.yml:385-390`) is still `always()` and keyed only on the `gating` output.

Mid-loop failure: `report-root`, `artifact-name` and `marker` are written before the loop
(`run-action-changes.sh:66-70`); the summary is appended per project inside it (`:151-168`); `gating`
and `combined-report` only after (`:177-180`). I confirmed the shape from the test at
`test-action-changes.sh:182-197` and by reading the control flow: `report()`'s `exit "$status"` for a
non-gate status terminates the script from the function body (not a subshell), so projects 1..n-1
survive in the summary and the artifact, and the comment is correctly skipped because
`combined-report` is unset.

Two honest notes:
- A missing `python3` with `annotate-api-changes: true` **does** fail the job (`:106-109`). That is
  contract-mandated (§5.2 B4: a named error, never a silent skip), not a violation of §5.9 — it is a
  prerequisite failure, not a publication failure.
- The `Upload API change reports` step is not `continue-on-error`, so an upload failure fails the job.
  Pre-existing and untouched by this PR. C1 does not make it worse: a pre-loop failure now leaves
  `report_root` containing `report.md`, so `if-no-files-found: error` still cannot newly trigger.

### Q5 — Boundary · **PASS**

`grep -rn "GITHUB_\|::error\|::warning\|::notice" crates/` returns four hits, all false positives:
`std::error::Error`, `super::error_model_graph` ×2, and a doc-comment "tag/group". Zero `GITHUB_*`
reads. The only file changed under `crates/` is `crates/gnr8/src/changes.rs`. Every byte of GitHub
dialect lives in `action.yml` and `scripts/*action*`.

No fallback chains. The branches I audited all select a *message* or evaluate a *precondition*:
fork-vs-permission message (`action.yml:348-354`), budget preconditions (`:350`,
`run-action-changes.sh:153`), annotation on/off (`:169`), digest skip-a-write
(`upsert-action-comment.sh:33`). The two accepted marker spellings are one predicate over one
response, evaluated in a single pass — two spellings of one identity, not "try A then B" — and are
`TODO`-scoped to one release.

### Q6 — Contract fidelity · **PASS**

All of §5.1 A1–A4, §5.2 B1–B7, §5.3 C1–C3, §5.4 D1–D3, §5.5 E and §5.6 F1–F7 are present and match
the specified behaviour, including the exact wording of the cap notice and the truncation notice.
The §5.7 test matrix is implemented case for case (12 changes cases, 13 comment cases after my
addition, 4 annotation cases, 12 Python unit tests, 4 Rust golden/containment tests), and the dogfood
matrix is widened to two `report_changes: "true"` projects with distinct `working-directories`, hence
distinct keys (`.github/workflows/generated-sdk-check.yml:54-64`).

§5.10 out-of-scope: no `--include-tag`, no `--allow`, no Checks API, no shipped `workflow_run`
workflow, no cross-tool comparison. §6 rejected alternatives: no `ReportFormat` variant for workflow
commands, no re-rendering in the Action, no Markdown parsing (the emitter reads `report.json`), no
bullet layout, no `<details>` fold, no `pull_request_target` support, no delete-when-empty (asserted
at `test-action-comment.sh:156-162`), no `comment:` input, no doc-only annotations. The single
`pull_request_target` occurrence in the diff is the documentation saying it is unsupported.

One wording note, not a defect: §5.7's test table row "two projects, distinct keys" reads as one key
per project block, while §5.1 A1 specifies one key per *invocation*. The implementation follows A1
(normative) and the docs state it plainly; the combined report simply carries the same marker line
once per project block, which the `any(...)` match handles.

### Q7 — Invariants · **PASS**

`scripts/check-invariants.sh` → *"invariants: clean — one native contract, no foreign coupling"*.
`scripts/check-ci-budget.py` → *"CI budget: every job is capped at 5 minutes"*. No `baseline`,
`compat`, `legacy`, `brownfield`, `migration` or `profile` vocabulary anywhere in the non-`thoughts/`
diff. The CLI remains GitHub-ignorant (Q5) and there is still exactly one renderer per format — the
Action asks the CLI for Markdown twice and never derives report text from the JSON.

### Q8 — Rust correctness · **PASS**

`schema_version` is the first field of `MachineReport` (`changes.rs:19`) fed from
`CHANGE_REPORT_SCHEMA_VERSION` (`:15`), and serde emits struct fields in declaration order, which
`json_report_carries_base_policy_sides_and_summary` pins byte-exactly with
`starts_with("{\n  \"schema_version\": 1,\n")` (`:456`) as well as by value (`:458`).

The partition (`:167-183`) is complete and disjoint: `gating` is derived as
`kind == ChangeKind::Breaking && scope.checked` (`crates/gnr8-core/src/changes/diff.rs:235`), so a
non-breaking finding can never be gating, and the short-circuit `kind != ChangeKind::Breaking ||
change.gating == gating` places every Additive/DocOnly change in exactly one group. Order within a
group is the machine report's order (no re-sort), and the four groups are emitted in the contract's
order, which `markdown_report_partitions_all_four_groups_in_stable_order` asserts both by heading
sequence and by the relative positions of findings `[1, 2, 0, 3, 4]`.

The golden test really does assert the new behaviour, not just tolerate it: `:542-547` pins
`"Breaking — not gating (1)\n\n"` and `"        Code: operation.removed\n"` in the byte-exact
expected string, and `empty_markdown_report_is_explicit` (`:633-635`) asserts *no* heading appears
when there are no findings.

---

## 4. Fix applied

`fcb734c` — `fix(action): apply the CRLF line normalization to the digest guard`

- `scripts/upsert-action-comment.sh:26-27` — bind the CR-trimmed line array once as `$lines` and read
  both the ownership match and the digest guard from it (`$lines[0] == "$digest_marker"`), replacing
  `.body | startswith("$digest_marker\n")`.
- `scripts/test-action-comment.sh` — the boundary fake now asserts and models the new query shape; new
  13th case: a CRLF-stored body whose content is unchanged performs the list call and **no** write.
- `docs/operations/artifacts-and-ci.md:312-321` — state the matrix-key limitation (F2) and that the
  digest comparison is CR-trimmed.

Verified in both directions. Against the pre-fix `startswith` semantics (both script and fake reverted
in a scratch copy) the new case fails at `test 2 -eq 1` — two calls, list plus PATCH. Against the
fixed script it passes with one call. Separately, driving the real `upsert-action-comment.sh` through
the repo's own fake `gh` with a CRLF-normalized, content-identical body: pre-fix issued a PATCH,
post-fix issues only the list call.

No CHANGELOG entry: the digest guard is itself unreleased (added in this PR), and the existing
`## Unreleased` line "Unchanged report bodies avoid API writes" is now true rather than aspirational.

---

## 5. Gates

Run on this machine, `PATH="/opt/data/home/.local/bin:$PATH"`, before and after `fcb734c`:

```
$ cargo test --workspace --locked
...
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   Doc-tests gnr8_engine
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
[exited with code 0]

$ make check
...
action changes tests: OK (12 cases)
action comment tests: OK (13 cases)      # 12 before the fix
action annotation tests: OK (4 cases)
...
[exited with code 0]

$ scripts/check-invariants.sh
invariants: clean — one native contract, no foreign coupling
$ python3 scripts/check-ci-budget.py
CI budget: every job is capped at 5 minutes
```

`make check` runs `invariants fmt-check clippy tsextract-deps test fixture-build goextract-build
pyextract-test tsextract-test action-test examples-check`; `go`, `node`, `python3` and `ruby` were all
present in this sandbox, so no gate was skipped.

---

MERGEABLE: YES
