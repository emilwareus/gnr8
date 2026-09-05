# Research: publishing API change reports on pull requests

Date: 2026-09-05 · Branch base: `origin/main` @ `78c89a8` · Workspace version `0.12.0`
(`Cargo.toml:9`)

Question:

> Issue #76 asks for "a GitHub Action mode that publishes the `gnr8 changes` result as a pull-request
> report". Issue #75 shipped `gnr8 changes` on 2026-09-04, two days after #76 was written, and it
> brought a Markdown renderer, a job summary, artifacts, and a marker-owned comment with it. What of
> #76 is therefore already done, what is genuinely missing, and what should the missing part look
> like?

Everything under **Verified** was read in this checkout or measured on this machine. Everything under
**Recommendation** / **Open** is judgement, not measurement.

---

## 1. Verified: the acceptance bar #76 actually sets

Read directly via `gh api repos/oaiz-io/gnr8/issues/76` (the `gh issue view` GraphQL path fails on
this repo with a Projects-classic deprecation error; the REST path works). Opened 2026-09-02
14:54Z, last touched 2026-09-02 15:03Z, **still open**, unlabelled. The body, in full:

> Add a GitHub Action mode that publishes the `gnr8 changes` result as a pull-request report.
>
> The report must show breaking, additive, and documentation-only changes, affected SDK operations,
> and source locations. It must also be available as Markdown and JSON artifacts when pull-request
> comments are not permitted.
>
> Expected workflow step:
>
> ```yaml
> - uses: oaiz-io/gnr8@v1
>   with:
>     report-api-changes: "true"
>     base-ref: origin/main
> ```
>
> Expected pull-request report:
>
> ```text
> API changes: 1 breaking, 2 additive
>
> - BREAKING: POST /books request field `title` is now required
> - ADDITIVE: GET /books returns optional field `nextCursor`
> ```
>
> API changes must be visible where maintainers review source changes. A pull-request report gives
> value before a project adopts generated SDK publication.

Decomposed into testable obligations:

| # | Obligation | Status in `78c89a8` |
|---|---|---|
| A1 | An Action mode that publishes `gnr8 changes` as a PR report | **done** |
| A2 | Inputs are exactly `report-api-changes` + `base-ref` | **done** |
| A3 | Report shows breaking / additive / documentation-only | **done** |
| A4 | Report shows affected SDK operations | **done** |
| A5 | Report shows source locations | **done** |
| A6 | Markdown **and** JSON available as artifacts | **done** |
| A7 | …**when PR comments are not permitted** | **partial** — see §2 |
| A8 | "visible where maintainers review source changes" | **not done** — see §2.1 |

The headline finding: #76 was written against a `gnr8 changes` that did not exist yet. PR #82 /
v0.12.0 implemented most of #76 as a side effect of shipping #75. The residue is small, sharply
defined, and mostly about *failure modes and placement*, not about report content.

### 1.1 The report content obligations (A3–A5) are satisfied by the CLI, not the Action

`gnr8 changes --markdown` is the single renderer. `crates/gnr8/src/changes.rs:129` `render_markdown`
emits, in order:

- `crates/gnr8/src/changes.rs:131-136` — `Base: <code>{ref}</code> → <code>{resolved-sha}</code>`
- `changes.rs:144-148` — `Exempt tags: <code>internal</code>` or `Exempt tags: none`
- `changes.rs:149-156` — `Summary: {n} breaking, {n} additive, {n} doc-only, {n} gating.`
- `changes.rs:161-170` — one indented line per finding:
  `    BREAKING  DELETE /books/{id}  operation removed  (exempt on base side; not gating)`
- `changes.rs:171-187` — **A4**: `        SDK operations: deleteBook (DELETE /books/{id}), listBooks (GET /books)`
- `changes.rs:188-198` — **A5**: `        Source: handlers/books.go:42` (or bare `file` when `line` is `None`)

**A3** is the `kind_label` mapping at `changes.rs:239-245`: `Breaking → "BREAKING"`,
`Additive → "ADDITIVE"`, `DocOnly → "DOC-ONLY"`. Those are exactly #76's three categories under
different spellings.

The golden test for the whole block is `changes.rs:456-517`
(`markdown_report_carries_base_policy_summary_and_finding_detail`), which asserts byte-exact output
including the SDK-operations line and the `Source:` line.

Two design properties of that renderer matter for everything downstream:

1. **It is the only implementation of the format.** `changes.rs:118-128` states it: the Action "asks
   the CLI for it rather than re-deriving it from the JSON report, so a change to the layout cannot
   leave the two disagreeing." This is CLAUDE.md rule 3 applied to a report format — one source per
   fact, no second renderer to drift.
2. **It is injection-hardened.** Findings sit in an indented code block; every value outside the
   block is HTML-escaped (`changes.rs:222-237`) and every value is collapsed to one line
   (`changes.rs:203-220`) so nothing can escape its container. `changes.rs:519-577`
   (`markdown_report_cannot_be_broken_out_of_by_analyzed_source`) proves it against
   `"operation removed\r\n## injected heading\n```"` and a `refs/heads/<script>` base ref, and asserts
   every line after the first finding still starts with four spaces. This matters because the report
   is rendered into a PR comment on a repo where the analyzed source may come from a fork.

### 1.2 What a finding carries — the raw material for any richer report

`crates/gnr8-core/src/changes/diff.rs:67-100`:

```rust
pub struct Change {
    pub kind: ChangeKind,                                  // breaking | additive | doc_only
    pub code: String,                                      // stable dotted taxonomy code
    pub operation: Option<String>,                         // "DELETE /books/{id}"
    pub operation_id: Option<String>,
    pub subject: Option<String>,                           // param / field / status / schema
    pub affected_operations: Sides<Vec<AffectedOperation>>,
    pub tags: Sides<Vec<String>>,
    pub exempt: Sides<bool>,
    pub gating: bool,
    pub message: String,
    pub file: Option<String>,                              // current side only
    pub line: Option<u32>,                                 // current side only, 1-based
    pub span: Option<SourceSpan>,                          // current side only
}
```

`ChangeKind` is `diff.rs:13-25`, `#[serde(rename_all = "snake_case")]` → `breaking` / `additive` /
`doc_only`. `Sides<T>` (`diff.rs:27-34`) is `{ base: Option<T>, current: Option<T> }` — an absent
graph side is `null`, never a silent empty vec.

`SourceSpan` is **not** in `gnr8-core`; it is the public graph type at
`crates/gnr8-sdk/src/graph.rs:824-832` — `{ file: String, start_line: u32, end_line: u32 }` where
`graph.rs:820-822` records that "the analyzed-module prefix has been stripped from `file` for
portability" and `:838-841` that every span is normalized against `module_root` for byte-stability.
Four consequences, all load-bearing for §5:

- **`file` is module-relative, not workspace-relative.** A GitHub annotation needs a path relative to
  `$GITHUB_WORKSPACE`. With `working-directories: services/books`, the finding says
  `handlers/books.go` and the annotation must say `services/books/handlers/books.go`. Nothing does
  that join today because nothing emits annotations.
- **`span.end_line` exists and is discarded.** `changes.rs:188-198` reads only `file` and `line`. An
  annotation can use it.
- **Provenance is current-side only, and removals deliberately have none.** `Collector::push`
  (`diff.rs:217-241`) fills the location from `scope.current_span` and nothing else; base provenance is
  never read; every removal-shaped finding explicitly clears it with `scope.at(None)`
  (`diff.rs:360, 478, 831, 1071, 1159, 1168, 1581, 1810, 1900`) and document-scope findings are always
  unlocated (`:2104`). **`operation.removed` — the most consequential code in the taxonomy — carries no
  `file:line` at all.** Any placement design that assumes every finding can be anchored to a diff line
  is wrong; see §5.2 B2.
- **Granularity stops above the field.** `provenance` exists on `Operation` (`graph.rs:543`), `Param`
  (`:585`) and `Schema` (`:693`) — `Field` and `Response` have none, so a
  `response.property.required.added` finding reports the enclosing schema's span.

`ChangeSummary` (`diff.rs:43-54`) is `{ breaking, additive, doc_only, gating }` — note `gating` is a
*fourth* count, not a subset flag, and `breaking` counts exempt findings too
(`diff.rs:46` "whether gating or exempt").

`gating` per finding is derived, not stored policy: `gating: kind == ChangeKind::Breaking &&
scope.checked` (`diff.rs:235`), and `checked` is `base_exempt.is_some_and(|v| !v) ||
current_exempt.is_some_and(|v| !v)` (`diff.rs:2012-2013`) — a finding is exempt only when **every
extant side** is exempt. Untagged means "no tag matched the exempt set", so `exempt = false`, so
checked: the safe default, exactly as decided on 2026-09-03.

Report order is one stable sort at `diff.rs:261-268`: `kind` → `operation` → `code` → `subject` →
`message`. Because `ChangeKind` derives `Ord` in declaration order (`diff.rs:14-25`), **breaking
findings already sort first**. A category-grouped report (§2.9) is a partition of an already-ordered
list, not a re-sort.

### 1.3 The taxonomy

`Change::code` is documented at `diff.rs:70` as the "stable dotted taxonomy code". Extracting every
dotted literal from `crates/gnr8-core/src/changes/diff.rs` yields 89 strings, 8 of which are suffix
fragments composed onto a prefix at call sites (`required.added`, `required.removed`,
`nullability.added`, `nullability.removed`, `enum.value.added`, `enum.value.removed`,
`constraints.changed`, `type.changed`) — leaving **81 whole codes**, matching the count recorded when
#75 shipped. Distribution by first segment:

| Prefix | Codes | Examples |
|---|---|---|
| `request.` | 24 | `request.body.required.added`, `request.parameter.removed`, `request.property.nullability.added`, `request.parameter.serialization.changed` |
| `response.` | 18 | `response.status.removed`, `response.body.schema.changed`, `response.property.required.added`, `response.media_type.removed` |
| `schema.` | 15 | `schema.removed`, `schema.name.changed`, `schema.property.constraints.changed`, `schema.enum.order.changed` |
| `operation.` | 9 | `operation.removed`, `operation.path.changed`, `operation.name.changed`, `operation.tags.changed`, `operation.exemption.added` |
| `security.` | 7 | `security.scheme.removed`, `security.operation.changed`, `security.global.changed` |
| `document.` | 7 | `document.base_path.changed`, `document.server.order.changed`, `document.title.changed` |
| `sdk.` | 1 | `sdk.group.changed` |

All 81 are printed verbatim in the fenced block at `docs/cli/commands.md:170-250` (fences at `:169`
and `:251`). I diffed that block against the literals extracted from `diff.rs`: **identical**, once
the 8 composed suffixes are removed. The taxonomy is therefore already a published contract, not an
implementation detail.

**The Markdown and human renderers never print `code`.** `changes.rs:101-114` (human) and
`changes.rs:161-198` (Markdown) read `kind`, `operation`, `message`, exemption, affected operations,
and `file`/`line` — never `change.code`. The 81 stable identifiers exist only in `--json` today
(`changes.rs:69-85` serializes the whole `Change`). A maintainer reading the PR comment cannot cite a
stable identifier for what they are looking at. §3 shows every comparable tool surfaces its rule id
in the human-facing report; §5 treats this as the smallest high-value change in the plan.

### 1.4 The delivery surfaces that exist today

`action.yml` is a composite action; the changes-mode path is four steps — run
(`:302-318` → `scripts/run-action-changes.sh`), upload (`:320-326`, `actions/upload-artifact@v6`, name
and path from step outputs, `if-no-files-found: error`), comment (`:328-340`,
`scripts/upsert-action-comment.sh`, failure downgraded to `::warning::`), gate (`:371-376`,
`exit 1` on `steps.api-changes.outputs.gating == 'true'`). Inputs: `report-api-changes` (`:25-28`,
default `"false"`), `base-ref` (`:29-32`, default `origin/main`), `exempt-tags` (`:33-36`).
`action.yml:118` validates the boolean spelling; `:124-127` rejects an empty `base-ref` when the mode
is on. **A2 is satisfied verbatim** — the issue's `with:` block works as written.

`scripts/run-action-changes.sh` is the engine:

- `:37-41` — `report_root` is a fresh `mktemp -d` under `$RUNNER_TEMP`; `combined` is
  `$report_root/report.md`; `work_root` holds intermediates *outside* `report_root` so the uploaded
  artifact is exactly the reports.
- `:59-66` — argv seeded as `(changes --base "$BASE_REF")` with `--exempt-tag` appended per non-empty
  line. The seeding is deliberate (`:56-58`): expanding an empty array under `set -u` is fatal on
  bash 3.2, which GitHub's macOS runners ship.
- `:71-74` — per-directory `git rev-parse --verify --end-of-options "${BASE_REF}^{commit}"` precheck,
  failing with a message that names `fetch-depth: 0`. The one guard that turns a raw git error into an
  actionable one — and the precedent §5.3 reuses.
- `:88-95` — **two CLI invocations per project**, `--json` then `--markdown`, into
  `$report_root/NNN/report.json` and a work-root body; `:92-95` hard-fails if the two disagree on the
  gate status. `:86-87` explains the second invocation is cheaper than a second renderer.
- `:100-104` — writes `$report_root/NNN/report.md` = `<!-- gnr8-api-changes -->` + an escaped
  `## API changes for {dir}` heading + the CLI's body. That heading is the **only** report text this
  script renders, and `:15-18` escapes it with the same five-character rule the CLI uses.
- `:110` — `cat "$combined" >> "$GITHUB_STEP_SUMMARY"`; `:111-117` — outputs `gating`, `report-root`,
  `combined-report`, and `artifact-name=gnr8-api-changes-${GITHUB_JOB}-{8 hex}` where the suffix is
  `git hash-object` of `WORKING_DIRECTORIES` + `BASE_REF`.

`scripts/upsert-action-comment.sh` is 19 lines (`:9-19`): list comments with `gh api --paginate`,
`--jq` select on `.user.login == "github-actions[bot]"` **and** body `contains("<!-- gnr8-api-changes -->")`,
`tail -n 1`; PATCH if found, `gh pr comment --body-file` if not.

**A1 and A6 are already true**: both formats are produced by the CLI per project and uploaded, and the
combined Markdown reaches both `$GITHUB_STEP_SUMMARY` and the PR comment.

### 1.5 The gate

`crates/gnr8/src/main.rs:120-123` — `if report.is_gating() { flush; exit(1) }`;
`diff.rs:113-119` — `is_gating()` is `summary.gating > 0`. The runner treats only 0 and 1 as gate
answers (`run-action-changes.sh:30-33`); any other status is a failed analysis that aborts the step.
The final `action.yml:371-376` step converts `gating=true` into a job failure, and it is
`if: always()` so `gnr8 check` still runs first. CHANGELOG v0.12.0 records the split as a breaking
change: "Status 1 is reserved for a command's domain gate… Execution and configuration failures now
exit with status 2" (`CHANGELOG.md:23-27`).

### 1.6 The tests that already exist, and their idiom

`make action-test` (`Makefile:95-100`) runs four scripts; two are ours, and both establish the pattern
any #76 work extends: **stub the boundary binary, drive the script, assert on argv logs and on the
files it wrote** — no network, no runner, no YAML evaluation.

`scripts/test-action-changes.sh` (151 lines) drives `run-action-changes.sh` with a fake `gnr8` injected
via `GNR8_BIN` (`:15-63`) that logs argv with `printf '%q '` into `$FAKE_LOG` and returns the *real*
Markdown or JSON shape depending on argv. `:13-14` states the principle: the fake "returns what the
real CLI returns for each format, so the runner is tested against the CLI's contract rather than
against a second implementation of the report." `GITHUB_OUTPUT`, `GITHUB_STEP_SUMMARY`, `RUNNER_TEMP`
and `GITHUB_JOB` are plain files/values (`:72-81`). Three cases: a full run with hostile inputs
(`:72-101` — duplicate, `#`-prefixed and space-padded tags; asserts the SDK-operations and `Source:`
lines reached the summary and that no `## injected heading` or fence escaped the code block); empty
`EXEMPT_TAGS` against a directory literally named `a<b>&c` (`:106-135`); and an unresolvable base ref
that must fail with `checkout with fetch-depth: 0` on stderr (`:137-149`).

`scripts/test-action-comment.sh` (53 lines) puts a fake `gh` first on `PATH` (`:9-19`) whose response
is switched by `$EXISTING_COMMENT_ID`. Two cases: update (`:26-39`, asserts the PATCH URL and that
`gh pr comment` was *not* called) and create (`:41-51`, asserts `gh pr comment 82 --repo …` and that
the jq filter still names `github-actions[bot]` — pinning the very behaviour G-3 identifies as wrong).

### 1.7 What the docs already promise

`docs/operations/artifacts-and-ci.md:245-314` is the canonical Action page: a multi-project workflow
with `fetch-depth: 0` (`:247-271`), the input table (`:275-290`), and the behavioural paragraph
(`:294-301`) promising a combined Markdown report "with affected SDK operations and current source
locations to the job summary", uploaded Markdown and JSON, and a "marker-owned pull-request comment
when the workflow token permits comments… A comment permission failure does not hide or weaken the
gate." `README.md:132-155` shows the minimal form. `docs/guides/` contains nothing about either.

`docs/cli/commands.md:121-254` documents the command; exit codes at `:284-292` reserve `1` for the
domain gate. Two constraints matter downstream. The limitation, verbatim at `:141-144`:

> `ConfigurePagination` and `ConfigureSdkRuntime` policy is not yet compared, so a change to
> pagination, retry, or timeout configuration alters generated SDK methods without producing a
> finding. Response headers and the schemas of additional request-body variants are likewise outside
> this comparison; their media types still participate in `request.body.media_type.*`.

And the rule that governs §5, at `:149-150`:

> The GitHub Action publishes this output rather than formatting one of its own.

**Neither the README nor the CI doc shows a `permissions:` block.** Grepping `permissions:` across
`docs/`, `README.md` and `.github/workflows/` returns four hits, all in this repo's own workflows
(`ci.yml:8`, `generated-sdk-check.yml:8`, `release-dry-run.yml:17` — `contents: read`;
`release.yml:26` — `contents: write`), none in a caller-facing example, and **`pull-requests: write`
appears nowhere in the repository.** See G-2.

Two stale pins, noticed in passing: `README.md:148` and `docs/operations/artifacts-and-ci.md:257`
both say `oaiz-io/gnr8@v0.11.0 # first release with API change reporting`. It shipped in **0.12.0**
(`CHANGELOG.md:12`, `:49-51`).

### 1.8 The repo dogfoods the mode — with commenting deliberately disabled

`.github/workflows/generated-sdk-check.yml:8-9` is `permissions: contents: read`.
`:108-129` calls `uses: ./` with `report-api-changes: ${{ matrix.report_changes }}` and
`base-ref: origin/main`, and `:123-127` explains the choice:

> The workflow token is contents:read, so the action's pull-request comment step cannot post and
> degrades to its documented warning. That is deliberate: this repository's own pull requests do not
> want the comment, and the report still reaches the job summary and the uploaded artifact, which is
> what this job asserts the composite path produces.

So the *degraded* path is exercised in CI on every PR; the *comment* path is exercised only by
`test-action-comment.sh` against a fake `gh`. That is a reasonable split, and it means the
"comments not permitted" branch of A7 is genuinely live — but it is live as a `::warning::`, not as a
report.

---

## 2. Verified: the gaps

Ranked by how much of #76's stated motivation they block.

### 2.1 G-1 — the report is not where maintainers review source changes (blocks A8)

#76's closing argument is "API changes must be visible where maintainers review source changes." A PR
comment is on the Conversation tab. The Files-changed tab — the place a maintainer is actually
reading `handlers/books.go` when the breaking change is on line 42 — shows nothing.

The data to fix this is already on every finding: `Change::file`, `Change::line`, and
`Change::span.end_line` (`diff.rs:90-99`). GitHub's `::error file=…,line=…,endLine=…,title=…::message`
workflow command renders inline in the diff. The Action emits **zero** workflow annotations today;
the only `::` commands in `run-action-changes.sh` are `::group::` / `::endgroup::` (`:84`, `:107`) and
the only `::warning::` is the comment-failure notice (`action.yml:339`).

This is the single largest gap and the one that most directly answers the issue's motivation
sentence. It is also the one with a hard ceiling: §1.2 establishes that removals carry no span, so an
annotation-only design would silently omit `operation.removed`, `schema.removed`,
`response.status.removed` and every other removal code. Annotations must be an *addition* to the
comment and summary, never a replacement for them.

### 2.2 G-2 — "when comments are not permitted" is a warning, not a documented fallback (A7)

Three distinct not-permitted conditions collapse into one generic `::warning::` at `action.yml:339`:

1. **Repo/org default token is read-only** and the workflow declares no `permissions:`. Because no
   documented example declares one (§1.7), this is the *default outcome for a new adopter following
   the README*.
2. **`pull_request` from a fork.** The token is read-only regardless of the workflow's `permissions:`
   block; nothing the caller writes in their workflow can fix it.
3. **Body too large / API rejection.** Indistinguishable from the other two in the current message.

The warning text — "the step summary and artifacts contain the report" — is accurate but does not
distinguish a *fixable* misconfiguration (case 1: add `permissions:`) from an *unfixable* platform
constraint (case 2: use the `workflow_run` pattern or read the summary). A maintainer who forgot the
permissions block gets no signal that a one-line fix exists.

Also: the fallback surfaces are never *linked*. The warning does not name the artifact, and the
artifact name is a hash suffix (`run-action-changes.sh:111-116`) the caller never chose.

### 2.3 G-3 — the upsert is owned by an author string, not by the marker

`upsert-action-comment.sh:11` filters `select(.user.login == "github-actions[bot]" and …)`.

`GITHUB_TOKEN` posts as `github-actions[bot]`, so the default path works. But callers who run the
Action with a GitHub App installation token or a PAT — the standard way to make a comment trigger
downstream workflows, and a common org policy — post under a different login. The list filter then
never matches, the `else` branch runs, and **every push to the PR appends a new comment**. There is
no cap and no cleanup. `test-action-comment.sh:51` asserts the literal `github-actions[bot]` string is
in the jq filter, so this is pinned behaviour, not an accident.

The author check is presumably there to stop an attacker planting a comment containing the marker and
having the Action overwrite it. That threat is weak (overwriting a comment the attacker wrote), and
the mitigation costs correctness under a legitimate configuration.

### 2.4 G-4 — one global marker, N possible invocations

`run-action-changes.sh:101` writes the constant `<!-- gnr8-api-changes -->` into every project's
Markdown. The **artifact** name is disambiguated by `$GITHUB_JOB` plus a hash of
`WORKING_DIRECTORIES` + `BASE_REF` (`:111-116`). The **comment marker** is not disambiguated at all.

Failure scenario: a monorepo runs the Action in a matrix (`services/books`, `services/orders`) or in
two jobs with different `base-ref`s. Both find the same marker. Whichever finishes last PATCHes the
one comment, and the other job's report is destroyed — silently, with a green comment step. This is
exactly the shape the artifact naming already anticipated and the comment naming did not.

**This repository has already hit it and worked around it in the workflow rather than in the
Action.** `.github/workflows/generated-sdk-check.yml:54-56`:

> `report_changes` runs the action's changes mode against the real binary. One project is
> enough — that path is language-agnostic — and one keeps the pull-request comment step
> from racing five jobs over the same marker comment.

The five-way matrix was narrowed to one entry *because* the marker races. That is a defect in the
Action, recorded as a constraint on its only caller.

### 2.5 G-5 — no size guard on either published surface

A large refactor produces a finding per changed field. `render_markdown` has no cap
(`changes.rs:161-199` is an unbounded loop over `report.changes`) and `run-action-changes.sh:106`
concatenates every project's report into one `combined`, which is then appended to the step summary
(`:110`) and used as the comment body (`action.yml:335`).

**The step summary has a documented hard limit and a documented failure mode.** GitHub: "Each step is
restricted to a maximum size of 1MiB", and "If more than 1MiB of content is added for a step, then
the upload for the step will fail and an error annotation will be created… Upload failures for job
summaries do not affect the overall status of a step or a job"
([workflow commands](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands)).

That is the worst possible interaction with the current design: the step summary is the surface the
docs promise as the fallback when comments are not permitted
(`docs/operations/artifacts-and-ci.md:297-298`), and it fails **silently and greenly** when the report
is large. A monorepo with several `working-directories` reaches 1 MiB with a few thousand findings —
well inside what a schema-wide rename produces.

For the **comment**, I could not verify a documented body limit. Four pages checked
([issue comments REST](https://docs.github.com/en/rest/issues/comments),
[Actions limits](https://docs.github.com/en/actions/reference/limits)) state none. The widely-repeated
65,536-character figure is an observed 422 error string, not documentation. **The plan must therefore
not encode that number**; it should budget against the documented 1 MiB and treat an API rejection as
a reported outcome rather than a silent one.

Nothing today truncates, folds into `<details>`, or says "N further findings — full report in the
artifact".

### 2.6 G-6 — a mid-loop failure discards every report

`run-action-changes.sh` writes `$GITHUB_STEP_SUMMARY` at `:110` and `$GITHUB_OUTPUT` at `:112-117`,
both **after** the `for dir in "${dirs[@]}"` loop closes at `:108`. The `report` helper exits the
whole script on any status other than 0 or 1 (`:30-33`), and the base-ref precheck exits 2 (`:71-74`).

Failure scenario: three projects; projects 1 and 2 analyze cleanly, project 3's pipeline fails with
status 2. The script exits before line 110. `report-root` is never set, so `action.yml:321`'s
`steps.api-changes.outputs.report-root != ''` guard is false and the artifact upload is skipped;
`combined-report` is empty so the comment step is skipped; nothing reaches the job summary. Two
perfectly good reports that were already written to disk are thrown away, and the maintainer sees
only the failure text. The `always()` conditions on the downstream steps were written to survive this
and cannot, because the outputs they key on are never emitted.

### 2.7 G-7 — the stable codes are invisible in every human-facing surface

Established in §1.3. `code` is in `--json` only. Consequences:

- A reviewer cannot say "we accept `response.property.nullability.added` here" using the vocabulary
  the tool itself publishes in `docs/cli/commands.md:165+`.
- The deferred `--allow <id>` work (recorded as out of scope for #75) has no visible handle to
  reference in the report, so the report cannot ever tell a user what to allow.
- Every comparable tool prints its rule id in the human report (§3). This is the cheapest gap to
  close and it unblocks the largest amount of future work.

### 2.8 G-8 — event coverage is `pull_request` only

`action.yml:329` gates the comment step on `github.event_name == 'pull_request'`. Not covered:
`pull_request_target` (the documented way to get a writable token on fork PRs, with the well-known
caveat that it must not check out fork code) and `merge_group` (merge-queue runs, where a gate is
wanted but a PR comment is not applicable). The `merge_group` case is *correct* by accident — there is
no PR to comment on — but the gate and the summary should still work, and they do.

`pull_request_target` is a real omission only if we choose to support it; §5 recommends **not**
supporting it and documenting `workflow_run` instead.

### 2.9 G-9 — the issue's literal report shape differs from ours

#76 sketches:

```text
API changes: 1 breaking, 2 additive

- BREAKING: POST /books request field `title` is now required
- ADDITIVE: GET /books returns optional field `nextCursor`
```

We render `Summary: 1 breaking, 0 additive, 0 doc-only, 0 gating.` and an indented code block, not a
bullet list. The differences are: no bullets, no grouping by category, doc-only and gating counts
added, no `<details>` fold.

I read the issue's block as illustrative of *content*, not a byte contract — it omits the exempt-tag
policy and the base revision, both of which a real gate must show, and its bullets would be a
markdown-injection surface the current code-block design deliberately avoids. The genuine content gap
is **grouping**: with 40 findings, an ungrouped list makes the reviewer scan for the breaking ones.
`ChangeSummary` already has the counts; the renderer just does not partition.

### 2.10 G-10 — the machine report has no version field

`MachineReport` (`crates/gnr8/src/changes.rs:15-21`) has exactly four fields: `base`, `policy`,
`summary`, `changes`. There is no `schema_version`. The *graph* artifact has one
(`crates/gnr8-core/src/graph_artifact.rs:15`, `GRAPH_ARTIFACT_SCHEMA_VERSION: u32 = 1`) and
`crates/gnr8/src/main.rs:99-105` hard-fails on a mismatch — so the project already holds the position
that a committed machine document must be version-detectable.

`report.json` is now an uploaded artifact (`action.yml:320-326`), which means third parties will
consume it out-of-band: a merge-queue bot, a release-notes generator, a dashboard. Adding a version
later is a breaking change for every one of them; adding it now costs one line. This is not part of
#76's text, but #76 is what turns the JSON from a CLI convenience into a published artifact, so it is
the right moment.

### 2.11 Non-gaps, recorded so they are not re-litigated

- **A2 is exact.** `report-api-changes` and `base-ref` exist with those spellings and defaults
  (`action.yml:25-32`).
- **A6 is exact.** Both formats are produced by the CLI and uploaded (`run-action-changes.sh:89-91`,
  `action.yml:320-326`).
- **The Markdown is not a second renderer.** Deliberate, documented at `changes.rs:118-128` and
  `docs/operations/artifacts-and-ci.md:305-306`.
- **`fetch-depth: 0` is handled.** Precheck plus a named error plus a test (`:137-149`).
- **The gate cannot be silenced by a comment failure.** `action.yml:338-340` swallows the comment
  script's status into a warning; the gate step at `:371-376` is independent and `always()`.

---

## 3. Verified: the platform mechanics that bound any design

Every claim here is from docs.github.com or an official `actions/*` repository, cited inline. Where I
could not find a documented statement I say so — several widely-repeated numbers are **not** in
GitHub's docs and this plan does not encode them.

### 3.1 Which permission governs a PR comment

A PR comment *is* an issue comment: "Every pull request is an issue, but not every issue is a pull
request" ([REST: issue comments](https://docs.github.com/en/rest/issues/comments)). The fine-grained
permission reference lists `POST /repos/{o}/{r}/issues/{n}/comments`,
`PATCH /repos/{o}/{r}/issues/comments/{id}` and the corresponding `GET` under **both** the *Issues*
and the *Pull requests* repository permissions, and explains why: "Some endpoints require more than
one permission. Other endpoints work with any one permission from a set of permissions"
([fine-grained permissions](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens)).
The `GITHUB_TOKEN` reference describes `pull-requests` as "Work with pull requests" and `issues` as
"`issues: write` permits an action to add a comment to an issue"
([GITHUB_TOKEN reference](https://docs.github.com/en/actions/reference/github_token-reference)).

**Honest limitation:** no documented sentence says which scope governs the issue-comment endpoints
*on a pull request*. The dual listing implies the resource type selects it. The plan documents
`pull-requests: write` (the resource is a PR) and says why.

### 3.2 Fork PRs cannot be fixed by the caller

- "If the workflow was triggered by a pull request from a forked repository, and the *Send write
  tokens to workflows from pull requests* setting is not selected, the permissions are adjusted to
  change any write permissions to read only."
- "You can use the `permissions` key to add and remove `read` permissions for forked repositories,
  but typically you can't grant `write` access."
- "Workflow runs triggered by Dependabot pull requests run as if they are from a forked repository,
  and therefore use a read-only `GITHUB_TOKEN`."

(all: [GITHUB_TOKEN reference](https://docs.github.com/en/actions/reference/github_token-reference))

"With the exception of `GITHUB_TOKEN`, secrets are not passed to the runner when a workflow is
triggered from a forked repository"
([events that trigger workflows](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows)).

This is the decisive fact for G-2: **on a fork PR, no `permissions:` block the caller writes can make
the comment step work.** The distinction between "you forgot `permissions:`" (fixable in one line) and
"this is a fork" (not fixable) is real, deterministic, and knowable before the API call — the Action
has `github.event.pull_request.head.repo.fork` in context.

### 3.3 `pull_request_target` is the wrong answer; `workflow_run` is the documented one

`pull_request_target` "runs in the context of the default branch of the base repository" and grants a
read/write token even from a public fork — the hazard GitHub warns about. GitHub's documented order of
preference is `pull_request` > `workflow_run` > `pull_request_target`. The `workflow_run` split is
privileged by design — "The workflow started by the `workflow_run` event is able to access secrets and
write tokens, even if the previous workflow was not" — runs on the **default branch**, and requires
the workflow file to exist there to trigger at all
([events that trigger workflows](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows)).
"Workflows triggered on `workflow_run` should treat artifacts uploaded from other workflows with
caution" ([secure use](https://docs.github.com/en/actions/reference/security/secure-use)).

Two mechanics the recipe must get right:

1. **The PR number is not in the `workflow_run` context.** GitHub's security-lab writeup — the
   canonical source — has the unprivileged half do `echo ${{ github.event.number }} > ./pr/NR` and
   upload it, and the privileged half validate it (`Number(raw)` + `Number.isInteger`) before use,
   because the artifact is attacker-influenced
   ([preventing pwn requests](https://securitylab.github.com/resources/github-actions-preventing-pwn-requests/)).
2. **Cross-run artifact download needs `actions: read`** — the fine-grained reference lists
   `GET .../actions/runs/{run_id}/artifacts` and `GET .../actions/artifacts/{id}/{archive_format}`
   under *Actions: read*, and `actions/download-artifact` takes `run-id` + `github-token` for exactly
   this ([actions/download-artifact](https://github.com/actions/download-artifact)).

### 3.4 The step summary: documented limit, documented silent failure

- Env var `GITHUB_STEP_SUMMARY`; write with `>> $GITHUB_STEP_SUMMARY`; GitHub-flavored Markdown.
- **"Each step is restricted to a maximum size of 1MiB."**
- **"If more than 1MiB of content is added for a step, then the upload for the step will fail and an
  error annotation will be created."** and **"Upload failures for job summaries do not affect the
  overall status of a step or a job."**
- "A maximum of 20 job summaries from steps are displayed per job."
- "Job summaries are isolated between steps" — each step gets its own file and its own budget.

(all: [workflow commands](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands))

**Unverified:** no docs.github.com page states that step summaries work on fork PRs. The mechanism is
a runner-side file write, not a REST call, and the workflow-commands page attaches no `permissions:`
requirement to it — so it is strongly implied, and this repo's own `contents: read` dogfood job
(`generated-sdk-check.yml:8`) demonstrates it under a read-only token. Confirm empirically on a fork
before documenting it as the fallback.

### 3.5 Annotations: syntax documented, placement and cap not

From the [workflow-commands page](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands):

```
::error file={name},line={line},endLine={endLine},title={title}::{message}
::warning file={name},line={line},endLine={endLine},title={title}::{message}
::notice file={name},line={line},endLine={endLine},title={title}::{message}
```

Documented parameters: `title`, `file` (default `.github`), `col`, `endColumn`, `line` (default `1`),
`endLine` (default `1`). The message is associated "with a particular file in your repository,
optionally specifying a position within the file."

**Two things are NOT documented, and the plan must not assert them:**

1. **Where workflow-command annotations render.** The docs never enumerate run summary vs
   Files-changed vs Checks tab. A8's value rests on the Files-changed rendering, so it must be
   confirmed empirically before it is documented as the answer.
2. **Any cap on annotation count.** The workflow-commands page, its Enterprise Cloud mirror, and
   [the Actions limits page](https://docs.github.com/en/actions/reference/limits) contain no
   annotation-count limit — the limits page does not mention annotations at all. The commonly-quoted
   "10 per step / 50 per job" is not in current GitHub documentation. The one documented number is a
   different thing: "The Checks API limits the number of annotations to a maximum of 50 per API
   request" ([REST: check runs](https://docs.github.com/en/rest/checks/runs)) — a batching limit on
   `POST .../check-runs`, not a display cap on workflow commands.

Because the cap is unknown, the plan caps annotations itself at a number it chooses and documents.

### 3.6 Artifacts

"Artifacts created by `upload-artifact@v4` are immutable"; a second upload under the same name fails
unless `overwrite: true`, and overwriting "will give the Artifact a new ID, the previous one will no
longer exist" ([actions/upload-artifact](https://github.com/actions/upload-artifact)). "A job can
accumulate up to 500 workflow artifacts across all steps"; default retention is 90 days
([workflow commands](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands),
[Actions limits](https://docs.github.com/en/actions/reference/limits)).

`action.yml:322` pins `@v6` and the artifact name already carries `$GITHUB_JOB` plus an 8-hex digest
(`run-action-changes.sh:111-116`), so immutability is respected for the normal case. Two invocations
in one job with identical inputs would still collide — a narrow edge, noted not fixed.

### 3.7 Comment API and rate limits

- List: `GET /repos/{o}/{r}/issues/{n}/comments`, `per_page` default **30**, max **100**.
- Create: `POST /repos/{o}/{r}/issues/{n}/comments` — "This endpoint triggers notifications" and
  "Creating content too quickly using this endpoint may result in secondary rate limiting."
- Update: `PATCH /repos/{o}/{r}/issues/comments/{id}` — keyed on the comment id at repo level, not
  nested under the issue.

(all: [REST: issue comments](https://docs.github.com/en/rest/issues/comments))

`GITHUB_TOKEN` is limited to "1,000 requests per hour per repository"
([Actions limits](https://docs.github.com/en/actions/reference/limits)).

**Unverified:** GitHub documents no HTML-comment marker convention for bot comment ownership — it is a
community pattern, not a platform feature — and documents no maximum comment body size. Both are
design choices we own, which is the right posture: the marker is *our* protocol on *our* comment.

The "prefer PATCH over POST" property is worth keeping explicitly: the create endpoint is the one the
docs flag for notifications and secondary rate limiting, so a working upsert is not just tidy, it is
the documented way to avoid notifying every reviewer on every push.

### 3.8 Composite-action mechanics, and one trap for the fork recipe

Inputs are **strings** surfaced as `INPUT_<NAME>`, so `inputs.report-api-changes == 'true'` is the
correct comparison; `action.yml:108-118` already validates the literal spelling rather than relying on
truthiness ([metadata syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/metadata-syntax)).
The changes-mode step outputs (`gating`, `report-root`, `combined-report`, `artifact-name`) are
deliberately internal — read via `steps.api-changes.outputs.*` inside the same action, never
re-exported through `action.yml:81-87`.

`gh` is "preinstalled on all GitHub-hosted runners" and each step using it "must set an environment
variable called `GH_TOKEN`" — `action.yml:332` does
([using GitHub CLI in workflows](https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/using-github-cli-in-workflows)).
Caveat: "A minimal set of tools is installed on the `ubuntu-slim` runner image"
([GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).

**The trap:** the canonical `concurrency: ${{ github.workflow }}-${{ github.ref }}` collapses every PR
into one group under `workflow_run`, because `github.ref` there is the default branch (§3.3). A
`workflow_run` commenting workflow must key on the PR number or
`github.event.workflow_run.head_branch`
([concurrency](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax-for-github-actions)).

---

## 4. Prior art: how comparable tools publish an API diff on a pull request

Surveyed on the axis #76 cares about — *placement and mechanics of the PR-visible report* — not on
diff coverage. Every row was read from the tool's own repository or documentation.

| Tool | PR surface | Upserts? | Stable change ids | Location resolved | Affected SDK ops | Gating | Hosted service |
|---|---|---|---|---|---|---|---|
| oasdiff (+ `oasdiff-action`) | comment **linking** a hosted review, plus workflow annotations | yes (marker + PATCH) | yes — kebab-case, 681 checks | in the **spec file** | no | `fail-on: ERR\|WARN` | for the linked review (`review: true`) |
| Optic | comment (HTML table) + check runs | **yes** — hidden marker | **no** — free-form prose | no | no | exit 1, `--severity none` | **was**; cloud sunset |
| GraphQL Inspector | **check-run annotations on the changed line** — never a comment | n/a | rule names | in the **SDL file** | n/a | `fail-on-breaking`, `approve-label` | GitHub App |
| `buf breaking` (+ `buf-action`) | comment (pass/fail table) + `--error-format github-actions` | **yes** — workflow+job-keyed marker | yes — `SCREAMING_SNAKE` | in the **`.proto`** | n/a | non-zero exit | no |
| Azure `oad` | step summary + artifact + commit status + one upserted comment | **yes** — no-op when byte-identical | yes — `Code = nameof(…)`, `docUrl` derived from id | in the **spec file** | no | commit status + override labels | no |
| Bump.sh action | comment | **yes** — identity marker **and** body digest; deletes when empty | schema has them; the Action discards them | no | no | `fail_on_breaking` | yes (`BUMP_TOKEN`) |
| **Speakeasy** | **PR description** + commit message + release notes | n/a | no | no | **yes — per language, per method** | **no CI gate** | yes |
| OpenAPITools/openapi-diff | none (files only) | n/a | no | no | no | `--fail-on-incompatible` / `--fail-on-changed` | no |
| OpenAPI Generator | **none** | — | — | — | — | — | — |
| **gnr8 today** | comment + job summary + artifacts | yes — one global marker (G-4) | **yes — 81 dotted** | **in the application source** | **yes** | exit 1 on checked breaking | **no** |

Two columns carry the finding, and the first one is narrower than it first looks:

- **Affected SDK operations is not unique to gnr8.** Speakeasy does it, and does it well: its generated
  notes carry "method-level change tracking (a record of the methods that were added, modified, or
  removed)" with "breaking change indicators", published in commit messages, **PR descriptions** and
  public release notes ([SDK changelogs](https://www.speakeasy.com/docs/sdks/manage/sdk-changelogs)).
  What it does *not* do is gate: that page frames the notes as helping maintainers "validate SDK
  changes easily before merging" — human review, not CI enforcement. So the pairing gnr8 has —
  SDK-operation impact **and** a binary gate — is the actually-rare combination.
- **Nothing resolves a finding back to the application source.** Every tool that resolves a location at
  all resolves it *inside the API description* — the OpenAPI YAML, the `.proto`, the SDL. They cannot
  do otherwise: they diff two documents and have no link back to the code that produced them. gnr8
  diffs two `ApiGraph`s, which carry `provenance` on every node, so `handlers/books.go:42` is a fact it
  already has (§1.2). **That is #76's A5, and it is the one thing in this survey nobody else can
  offer.** It is also exactly what the issue's closing sentence is asking for.

### 4.1 oasdiff — the closest analogue, and the clearest thing not to copy

`oasdiff-action` ships four sub-actions (`breaking`, `changelog`, `diff`, `validate`) with `base` and
`revision` required, `fail-on: ERR|WARN`, `format: text|json|yaml|markdown|html`, `exclude-elements`,
`filter-extension`, and `err-ignore` / `warn-ignore` files of regexes
([oasdiff-action](https://github.com/oasdiff/oasdiff-action)). Its documented comment behaviour is the
sentence to read twice:

> When changes are found it posts a side-by-side review link as a PR comment; drop `github-token` and
> the `pull-requests: write` permission to keep that link in the job summary instead.

Three things to take:

1. **The permission and the fallback are documented in the README's own example**, which carries
   `permissions: { contents: read, pull-requests: write }` inline. gnr8's equivalent examples carry no
   `permissions:` block at all (§1.7). That is the single cheapest correction in this document, and a
   competitor already does it.
2. **The comment/summary degradation is stated as designed behaviour**, not as a warning emitted at
   runtime. WS-A3 adopts that posture.
3. **`fail-on` as a severity threshold** is a coherent design, and gnr8 deliberately does not have it:
   the gate is tag scope, not severity (`2026-09-03-api-tags-breaking-change-gating.md:453-476`). Not a
   gap; a different, already-decided answer.

What not to copy: the comment carries a **link to oasdiff.com**, with `review` defaulting to `true`.
The report lives on someone else's server; the PR gets a URL. gnr8's report must remain wholly
self-contained — the Markdown *is* the comment, the artifact *is* the data.

Two further details worth recording. `--format` accepts `githubactions`, so the substance oasdiff puts
in the PR is a stream of `::error`/`::warning`/`::notice` workflow commands with `file=`/`line=` and
the check id in `title=` — independent confirmation that WS-B's mechanism is the one a peer tool
reaches for, and that carrying the id in `title=` is the right place for it. And `include-checks` is
now a **deprecated, ignored input** on the action
([`breaking/action.yml`](https://github.com/oasdiff/oasdiff-action/blob/main/breaking/action.yml)),
replaced by severity levels — a small caution against building a per-check selection surface before the
taxonomy has settled, which is one more reason §5.10 leaves `--allow` out.

Its taxonomy is worth noting for contrast: ~681 checks (317 breaking / 17 warning / 347
informational) with kebab-case descriptive ids — `api-path-removed-without-deprecation`,
`new-required-request-property`, `response-property-became-nullable`
([breaking-changes catalogue](https://www.oasdiff.com/docs/breaking-changes)). gnr8's 81 dotted codes
(§1.3) are an order of magnitude fewer and hierarchically sorted, which is the right trade for a report
that is grouped and read by humans — but only if the report actually *prints* them (G-7). oasdiff's
ignore files, by contrast, match **rendered English prose**, the same fragility Optic has (§4.2).

### 4.2 Optic — the upsert marker to steal, and the coupling that killed it

Optic's comment carries a hidden ownership marker as its first bytes
(`projects/optic/src/commands/ci/comment/common.ts` at
[`a7bf21e`](https://github.com/opticdev/optic/blob/a7bf21ebc3ff1fa19d167efb5cac44c5e9a2a456/projects/optic/src/commands/ci/comment/common.ts)):

```
<!--
DO NOT MODIFY
app_id: optic-comment-3UsoJCz_Z0SpGLo5Vjw6o
commit_sha: ${commit.sha}
-->
```

The `app_id` is a random constant used to find and **edit** the previous comment. Two ideas worth
taking: the `DO NOT MODIFY` line (the marker is machine state, and saying so costs nothing), and
carrying a *fact* in the marker (they carry the commit; WS-A1 carries the report key). The comment
body itself is a raw `<table>` of API | Changes | Rules | Tests | `[View report]`, with per-operation
detail only under `--verbose` — evidence that *grouping and counts first, detail second* is what
survives contact with real PRs, which is WS-D2.

Their documented posting rule: "Optic posts a summary of the run as a comment on pull / merge requests
**when there is something meaningful to report**"
([setup-ci](https://web.archive.org/web/20240908150818/https://www.useoptic.com/docs/setup-ci)).
Considered and not adopted — §6 item 7.

The cautionary half. Optic's rule identity is a free-form prose `name` on `RuleResult`
(`projects/openapi-utilities/src/results.ts`) — `'prevent operation removal'` — with no id, and
`x-optic-exemptions` matches that prose string verbatim. An internal
`` `${rulesetName}:${ruleName}` `` alias exists in `rule-filters.ts` but is never emitted. So the
user-facing handle for "allow this finding" is an English sentence that a rename silently breaks.
gnr8's dotted codes are the better design and G-7 is the reason they aren't yet visible.

And the structural lesson: Optic was acquired by Atlassian (2024-04-25) and `v1.0.0` (2024-08-07)
"Removed connections to optic cloud servers" and made `optic run` — the command that posted the PR
comment — unsupported ([v1.0.0 release](https://github.com/opticdev/optic/releases/tag/v1.0.0),
[PR #2851](https://github.com/opticdev/optic/pull/2851), which deleted 7,083 lines).
`app.useoptic.com` no longer resolves. **The PR report was coupled to a hosted service and died with
it.** gnr8's report has no server in the path — the CLI renders it, the runner publishes it — and §5
keeps it that way.

### 4.3 GraphQL Inspector — the annotation precedent, and where it differs

The strongest prior art for WS-B. Its GitHub Action "will annotate every change, next to the line in
the code where it happened", via a **Check Run with annotations**; `annotations` is an input defaulting
to `true`, `fail-on-breaking` defaults to `true`, and `approve-label` (default
`approved-breaking-change`) lets a maintainer mark a break intentional
([Inspector action docs](https://the-guild.dev/graphql/inspector/docs/products/action)).

Three readings:

1. **The "annotate next to the line" experience is a real, shipped pattern** — it is not something #76
   is inventing, and it is exactly what "visible where maintainers review source changes" means.
2. **They use Check Runs, we would use workflow commands.** Check Runs need `checks: write` and a
   GitHub App, and carry the same fork blocker (§3.2), while workflow commands need no permission at
   all. That is why §5.10 rules Check Runs out — but it also means their annotation rendering is not
   direct evidence for ours, which is Open Q 1.
3. **`approve-label` is a genuinely good idea we do not have.** A PR label as the approval surface for
   a deliberate break is cheaper than a content-addressed `--allow <id>` and lives where the reviewer
   already is. Out of scope for #76 (it belongs with the deferred allowance work,
   `2026-09-03-api-tags-breaking-change-gating.md:919-922`), recorded here so it is not lost.

### 4.4 `buf breaking` — the taxonomy shape, and the one design we consciously diverge from

Buf organizes breaking rules into four **strictness categories** — `FILE` (default), `PACKAGE`,
`WIRE_JSON`, `WIRE` — where each names the guarantee it protects ("Detects changes that break wire
(binary) encoding"), with `SCREAMING_SNAKE` rule ids: `ENUM_NO_DELETE`, `FIELD_SAME_CARDINALITY`,
`FILE_SAME_PACKAGE`, `MESSAGE_NO_DELETE`, `RPC_SAME_REQUEST_TYPE`
([buf breaking rules](https://buf.build/docs/breaking/rules/)). Stable ids plus named categories, in a
tool whose whole value is CI gating — the same bet gnr8 made with 81 dotted codes and three kinds.

`buf breaking` ships `--error-format` with
`[text,json,msvs,junit,github-actions,gitlab-code-quality]`
([buf breaking reference](https://buf.build/docs/reference/cli/buf/breaking/)) — i.e. **the CLI itself
emits GitHub workflow annotations**. That is the alternative §6 item 1 rejects, shipped by a
well-regarded tool, and the divergence should be stated honestly: buf is a CI-first binary with no
integration layer, so the format belongs in its CLI; gnr8 ships a composite action that already owns
100% of its GitHub dialect, so the format belongs there. If gnr8 ever ships integrations for a second
CI host, that boundary is what stops the CLI from growing one flag per host.

Buf's category names are also a better *frame* than a severity ladder: `FILE` ⊃ `PACKAGE` ⊃
`WIRE_JSON` ⊃ `WIRE` answer "compatible with respect to what?", not "how bad is it". gnr8's three kinds
are a severity ladder, and that is the right shape for an HTTP contract — but the framing is worth
remembering if the taxonomy ever needs a second axis.

And `bufbuild/buf-action` ships **exactly the comment identity WS-A1 proposes**, which I read directly
(`src/comment.ts:23-41`):

```ts
const oldCommentTag = "<!-- Buf results -->";
// commentTag ... It is unique to the workflow and job.
function commentTag(): string {
  return `<!-- buf ${context.workflow}:${context.job} -->`;
}
```

Three things in eighteen lines. They **migrated from a bare constant to a keyed marker** — the exact
G-4 fix — and kept the old tag in the lookup with a `TODO: Remove the old comment tag check in a
future release` so existing comments are adopted rather than orphaned; gnr8 should do the same when
WS-A1 lands, matching the old bare `<!-- gnr8-api-changes -->` alongside the new key for one release.
`findCommentOnPR` paginates `listComments` and matches **on the tag alone — there is no author
filter**, which is WS-A2's argument made by a shipped tool.

Buf also feeds the *same* `core.summary` buffer to both the comment and the step summary, so the two
cannot disagree — the same one-renderer discipline `changes.rs:118-128` states for gnr8.

### 4.5 The rest, briefly

- **Speakeasy** is the only surveyed tool that reports *SDK method* impact: added / modified /
  removed methods with breaking indicators, in commit messages, PR descriptions and public release
  notes ([SDK changelogs](https://www.speakeasy.com/docs/sdks/manage/sdk-changelogs)). It publishes
  into the PR *description*, not a comment, and the same page frames the value as helping maintainers
  "validate SDK changes easily before merging" — i.e. **human review, not a CI gate**. Worth knowing
  precisely, because it means gnr8's claim is not "we show SDK impact" but "we show SDK impact, the
  source line, and fail the build".
- **OpenAPITools/openapi-diff** exports console / markdown / html / json / asciidoc and gates with
- **Bump.sh's action** (`command: diff`) comments on the PR and its README example carries
  `permissions: { contents: read, pull-requests: write }`, with `fail_on_breaking` as the gate — but it
  requires a `BUMP_TOKEN` and diffs against documentation hosted on Bump.sh
  ([bump-sh/github-action](https://github.com/bump-sh/github-action)). Same hosted coupling as oasdiff's
  review link. Its comment mechanic, read from `bump-sh/github-action` `src/github.ts:125-190`, is the most refined in the
  survey and has one idea gnr8 should take: `createOrUpdateComment(body, digest)` carries **two**
  markers — one for identity, one for the body digest — and only calls `updateComment` when
  `digest !== existingDigest`, so an unchanged report costs zero writes; it also has a `deleteComment`
  path for when the diff empties. The digest guard is worth adopting (it removes a needless PATCH on
  every no-op push against the 1,000-request/hour token budget, §3.7); the delete-when-empty behaviour
  is the same judgement call as §6 item 7.
- **Azure `oad`** is wired into Azure's spec repos as GitHub **checks** — "Swagger Breaking Change"
  and "Breaking Change(Cross-Version)" ([Azure/openapi-diff](https://github.com/Azure/openapi-diff)) —
  and its surrounding pipeline is the most complete in the survey: step summary *and* artifact *and* a
  commit status *and* one upserted aggregate comment that is a **no-op when the body is byte-identical**.
  Two ideas: deriving each rule's docs URL from its id so the two cannot drift, and override labels
  that encode the *reason* (`-Approved-BugFix`, `-Approved-Security`) rather than a bare approval — a
  better shape than GraphQL Inspector's single `approve-label`, and worth remembering for the deferred
  allowance work. `UNVERIFIED:` I confirmed neither the rule-code scheme nor the label spellings from a
  primary source; do not cite specifics without checking.
- **OpenAPI Generator** has **no** diff, changelog or change-report surface at all
  ([OpenAPITools/openapi-generator](https://github.com/OpenAPITools/openapi-generator)); breaking-change
  detection in that ecosystem is left to the dedicated tools above. Worth stating plainly because it is
  the tool gnr8 is most often compared to.
- **`pb33f/openapi-changes`** renders changes as graphs, trees, diffs, JSON and markdown
  ([pb33f/openapi-changes](https://github.com/pb33f/openapi-changes)) — a richer *visualisation* answer
  to the same problem, with no PR-comment integration surveyed here.

### 4.6 What this survey decides

**Steal:** a documented `permissions:` block in the example (oasdiff, Bump.sh); a marker keyed to
workflow+job, matched without an author filter, with the previous bare marker kept in the lookup for
one release (buf, verified in `bufbuild/buf-action` `src/comment.ts:23-41`); a body-digest guard so an unchanged report costs
no API write (Bump.sh, `bump-sh/github-action` `src/github.ts:148`); one buffer feeding both the comment and the step summary
(buf); counts-and-groups first with detail second (Optic); annotate the changed line (GraphQL
Inspector); the check id carried in the annotation's `title=` (oasdiff); stable machine ids surfaced to
the human (buf, oasdiff, Azure — and pointedly *not* Optic).

**Do not comply:** no hosted-service link standing in for the report, no second server in the report
path, no adoption of any tool's id vocabulary or ignore-file grammar, no matching of anyone's output.
Reading and emitting *OpenAPI documents* is legitimate and unaffected; comparing gnr8's SDK to another
generator's is forbidden (CLAUDE.md 0.2) and nothing here proposes it.

**Four recurring failure modes to design against**, each visible in more than one tool:

1. **Suppression keyed on prose.** oasdiff's ignore files and Optic's `x-optic-exemptions` both match
   rendered English. A rule rename silently un-suppresses. gnr8's dotted codes avoid this *if* G-7 is
   closed so a reviewer can see them.
2. **Rule namespaces that exist only in config and never on a change.** Optic's
   `` `${ruleset}:${rule}` `` alias is computed and never emitted; the same shape appears elsewhere.
   Whatever id a tool expects in its config must be the id it prints — WS-D1.
3. **Reporting and gating masking each other.** Several tools let an output-selecting flag disable the
   failure flag. gnr8's split is already correct — `action.yml:371-376` keys only on `gating`, and a
   comment failure is swallowed at `:338-340` — and §5.9 makes keeping it an invariant.
4. **A comment that is only a link or a status table**, with the substance in the log or on a server.
   oasdiff, Bump.sh and buf's comment all land here. gnr8's comment already *is* the report; that is
   the property WS-C protects when it budgets the body rather than replacing it with a pointer.

**The differentiator to lead with:** source locations in the *application source*, which nothing
surveyed has, paired with a binary gate, which the one tool that does map to SDK operations does not
have. gnr8 already computes both; §5 is about putting them where they can be seen.


---

## 5. Recommendation: close the residue; do not rebuild the mode

#76 does not need a new Action mode. It needs the mode that shipped with #75 to be **placed**,
**honest about failure**, and **bounded**. Six workstreams, each closing named gaps, each landable
alone.

| WS | Closes | Surfaces touched | Ships |
|---|---|---|---|
| A — comment identity and one honest failure path | G-2, G-3, G-4 | `run-action-changes.sh`, `upsert-action-comment.sh`, `action.yml` | shell only |
| B — inline annotations | G-1 / A8 | new `scripts/emit-action-annotations.py`, `action.yml` | shell + python |
| C — bound both surfaces | G-5, G-6 | `run-action-changes.sh` | shell only |
| D — the report shows its own vocabulary | G-7, G-9 | `crates/gnr8/src/changes.rs` | Rust |
| E — version the machine report | G-10 | `crates/gnr8/src/changes.rs` | Rust |
| F — documentation | G-2 (doc half) | `README.md`, `docs/operations/artifacts-and-ci.md`, `docs/cli/commands.md`, `CHANGELOG.md` | docs |

A, C and F together are the minimum that closes #76's literal text. B is what answers its motivation
sentence. D and E are small, high-leverage, and unblock later work.

### 5.1 WS-A — one comment key, one owner, one honest failure message

**A1. Key the marker to the same identity the artifact already uses.** `run-action-changes.sh:111`
already computes the digest, and `:116` already builds the name inline:

```bash
artifact_suffix="$(printf '%s\n%s\n' "$WORKING_DIRECTORIES" "$BASE_REF" | git hash-object --stdin | cut -c1-8)"
...
  echo "artifact-name=gnr8-api-changes-${GITHUB_JOB:-job}-$artifact_suffix"
```

Hoist that name into a variable (`artifact_name=`) above the loop — it depends only on the inputs —
emit the marker as `<!-- gnr8-api-changes:${artifact_name} -->` at `:101` instead of the bare
constant, and pass it to the comment script as `$MARKER`. **One derivation, two uses** — no second key to drift,
and no new fact. Two jobs, or a matrix, now own two comments instead of fighting over one (G-4).

Consequence to document: changing `working-directories` or `base-ref` changes the key, so the previous
comment is orphaned rather than updated. That is the correct trade — an orphaned stale comment is
visible and harmless; a clobbered fresh one is neither.

**A2. Own the comment by marker, not by author.** `upsert-action-comment.sh:11` drops
`.user.login == "github-actions[bot]"` and takes the marker from `$MARKER`:

```bash
ids="$(gh api --paginate "repos/$REPOSITORY/issues/$PR_NUMBER/comments" \
  --jq ".[] | select(.body | contains(\"$MARKER\")) | .id")"
```

Update the **first** id and `DELETE` the rest. Deterministic (oldest wins, stable across runs) and
self-healing (a repository that already accumulated duplicates from the G-3 bug converges to one on
the next run). `DELETE /repos/{o}/{r}/issues/comments/{id}` needs the same write scope the update
already needs (§3.7), so this costs no new permission.

This also fixes G-3 outright: an App-installation token or a PAT now upserts instead of appending.
The dropped author check was defending against an attacker planting a marker comment for us to
overwrite — a threat whose worst outcome is overwriting the attacker's own text. `bufbuild/buf-action`
matches on the tag alone with no author filter (`src/comment.ts:31-35`), which is the same call by a
peer tool.

Two refinements taken from the survey (§4.4, §4.5):

- **Keep the old bare marker in the lookup for one release.** Buf did exactly this when it moved from
  `<!-- Buf results -->` to its keyed tag, with a `TODO` to drop it later. Matching
  `<!-- gnr8-api-changes -->` *or* `<!-- gnr8-api-changes:<key> -->` means existing comments are
  adopted rather than orphaned on upgrade.
- **Guard the update with a body digest.** Bump.sh only calls `updateComment` when the new body's
  digest differs from the stored one (`src/github.ts:148`). Appending the digest to the marker line and
  comparing before the PATCH removes a needless write on every no-op push — meaningful against the
  documented 1,000-requests/hour token budget (§3.7) on a busy repository.

**A3. Decide the failure message before the call, from a fact the Action already has.** §3.2 makes the
fork case a platform certainty, not a guess, and `github.event.pull_request.head.repo.fork` is in the
composite action's context. So `action.yml:328-340` becomes:

- **fork PR** → skip the API call entirely and emit a `::notice::` (nothing is misconfigured):
  `gnr8 action: pull-request comments are unavailable on fork pull requests (the token is read-only);
  the full report is in this job's summary and in the "<artifact-name>" artifact. See <docs link> for
  the workflow_run recipe.`
- **not a fork, call failed** → `::warning::` naming the fix:
  `gnr8 action: could not comment on the pull request. Add "permissions: pull-requests: write" to the
  workflow. The full report is in this job's summary and in the "<artifact-name>" artifact.`

This is **not** a fallback chain (CLAUDE.md rule 3). The report is derived once and published to the
summary and the artifact by one path each. What branches is a *message*, chosen by one deterministic
precondition; neither branch produces the report a second way, and neither recovers a fact the other
failed to produce.

**A4. Leave the event filter alone — G-8 closes as "no change".** `action.yml:329` stays
`github.event_name == 'pull_request'`. `pull_request_target` is deliberately not supported (§5.9);
`merge_group` correctly has no comment to post, and its gate and step summary already work. G-8 is
recorded as a gap because the omission was undocumented, not because the behaviour is wrong; WS-F
documents it and nothing in the Action changes.

### 5.2 WS-B — annotations, with the ceiling stated up front

**B1. What it is.** For each finding that carries a current source location, emit a GitHub workflow
command so the finding renders against the file the reviewer is reading. Level mapping:

| Finding | Command | Why |
|---|---|---|
| breaking, `gating: true` | `::error` | it fails the job; the annotation should match |
| breaking, `gating: false` (exempt) | `::warning` | real, reported, deliberately not blocking |
| additive | `::notice` | informational |
| documentation-only | *nothing* | inline noise on every prose edit; it is in the comment and the summary |

**B2. The hard ceiling, stated in the docs and the code.** §1.2: removal-shaped findings clear their
span by construction (`diff.rs:360, 478, 831, 1071, …`), so `operation.removed`, `schema.removed`,
`response.status.removed` and friends **cannot be annotated** — the code they describe is gone from
the current tree. The emitter skips unanchored findings and the final summary notice reports how many
were skipped. This is why annotations are an *addition* to the comment and summary, never a
replacement, and why the docs must say so plainly.

Second ceiling: provenance stops at `Operation`/`Param`/`Schema` (§1.2), so a field-level finding
annotates the enclosing schema's span. Accurate, coarser than the `file:line` shape suggests.

**B3. Where the code lives, and why not in the CLI.** The emitter is
`scripts/emit-action-annotations.py`, invoked once per project from `run-action-changes.sh` with the
project's `report.json` and its working directory. It reads the **JSON** — never the Markdown — so
`docs/cli/commands.md:149-150` ("The GitHub Action publishes this output rather than formatting one of
its own") is preserved: there is still exactly one renderer per format, and the annotation stream is a
*placement* of the machine report, not a fourth rendering of the human one.

Putting the emitter in `crates/gnr8/` would move GitHub's workflow-command dialect into the CLI. Today
100% of that dialect lives in `action.yml` and `scripts/*action*.sh` (`::group::` at
`run-action-changes.sh:84`, `$GITHUB_STEP_SUMMARY` at `:110`, `$GITHUB_OUTPUT` at `:112`, `::warning::`
at `action.yml:339`), and the CLI knows nothing about GitHub. **Keeping that boundary is the whole
answer to the invariant question**: gnr8 ships a GitHub integration, so the integration layer speaks
GitHub; the product core does not, and a second CI host would be a second integration rather than a
second CLI flag.

**B4. Why python3, and what it costs.** The action path today uses `bash`, `git` and `gh` and nothing
else — no `jq`, no `python3` (`gh --jq` is gh's built-in, not the `jq` binary). Adding an interpreter
is a real decision, so state it:

- The message field is raw in JSON and *does* contain newlines — the repo's own test fixture is
  `"operation removed\n## injected heading"` (`scripts/test-action-changes.sh:55`). Workflow-command
  data must be percent-encoded (`%25`, `%0A`, `%0D`) and property values additionally escape `:` and
  `,`. Getting that right in bash, on a string that comes from analyzed source, is exactly the class
  of bug the Markdown renderer's `one_line`/`escape_html` pair exists to prevent.
- The repo already owns the `scripts/*.py` + `scripts/test_*.py` pattern with a `make` target
  (`scripts/release-notes.py` / `scripts/test_release_notes.py` / `Makefile:102-103`), so the emitter
  gets real unit tests rather than shell assertions.
- `jq` was rejected because I could not verify it on the runner images, and because the encoding rules
  above are worse in jq+bash than in Python's stdlib.

A missing `python3` must be a **named error**, not a silent skip (rule 3). The off switch is the input,
not a fallback.

**B5. Surface.** New input on `action.yml`:

```yaml
  annotate-api-changes:
    description: Emit workflow annotations for findings that carry a current source location.
    required: false
    default: "true"
```

validated by the existing `validate_boolean` at `action.yml:108-118`. It has effect only when
`report-api-changes` is `true`.

**B6. Path join — the one correctness trap.** `Change.file` is relative to the **analyzed module**
(`crates/gnr8-sdk/src/graph.rs:820-822`); `file=` in a workflow command must be relative to
`$GITHUB_WORKSPACE`. The emitter takes the project directory as argv and emits
`normpath(join(project_dir, change["file"]))`. Verified against a real artifact: the bookstore graph's
`module` is `example.com/bookstore` and every operation's provenance is `main.go` with the source at
`examples/bookstore/main.go` — so the join is `working-directory + provenance.file` and nothing else.

The residual risk is a pipeline whose `Source` inputs point below the project root (`inputs(["./cmd"])`),
where the module root and the working directory differ. GitHub drops annotations for files not in the
run's tree, so a wrong join degrades to "no annotation", never to a wrong one. Recorded as Open Q 3.

**B7. Cap.** §3.5: GitHub documents **no** annotation-count limit, so the plan sets its own rather than
discovering the platform's the hard way. Cap at **50 annotations per project**, chosen because 50 is
the one annotation number GitHub does document (the Checks API per-request batch,
[REST: check runs](https://docs.github.com/en/rest/checks/runs)) and is therefore a defensible,
citable constant rather than an invented one. Emit a final
`::notice::gnr8: N further findings not annotated (M unanchorable); see the job summary and the
"<artifact>" artifact.` The JSON is already sorted breaking-first (§1.2), so the cap keeps the findings
that matter.

### 5.3 WS-C — make both published surfaces survive size and failure

**C1. Publish incrementally so a late failure cannot erase early work (G-6).** In
`run-action-changes.sh`, move the writes that do not depend on the loop's outcome to before it, and
move the summary append inside it:

- before the loop: `report-root=` and `artifact-name=` → `$GITHUB_OUTPUT` (both are pure functions of
  the inputs, computed at `:37` and `:111`);
- inside the loop, after each project's `report.md` is written (`:104`): append that project's block to
  `$GITHUB_STEP_SUMMARY` as well as to `combined`;
- after the loop: `gating=` and `combined-report=`.

A failure in project 3 then still uploads the artifact (`report-root` is set,
`action.yml:321`'s guard passes) and still shows projects 1–2 in the summary. The comment is correctly
skipped, because `combined-report` is unset and the report is genuinely incomplete.

**C2. Budget the step summary (G-5).** §3.4: 1 MiB per step, and overflow "will fail… Upload failures
for job summaries do not affect the overall status of a step or a job" — the silent-loss case. Track
bytes appended; at a documented budget of **900 KiB** stop appending project blocks and append instead:

```
Report truncated at 900 KiB (GitHub limits a step summary to 1 MiB).
Full Markdown and JSON: the "<artifact-name>" artifact.
```

**C3. Budget the comment.** Measure `combined` before calling `gh`. Over budget → skip the call and
emit the specific warning naming the artifact. **Do not encode 65536**: §3.7 establishes it is an
observed error string, not documentation. Use a conservative, documented-in-our-docs figure (recommend
**60 KiB**) and say in the docs that it is gnr8's budget, not GitHub's published limit.

Both are **preconditions**, evaluated once before acting — the same shape as the existing
`fetch-depth: 0` precheck at `run-action-changes.sh:71-74` — not retries after a failure.

### 5.4 WS-D — let the report speak the vocabulary the docs publish

**D1. Print the change code in the Markdown report (G-7).** `render_markdown`
(`crates/gnr8/src/changes.rs:161-199`) gains one indented sub-line, in the same style as the two that
already exist:

```
    BREAKING  POST /books         request field `title` became required
        Code: request.property.required.added
        SDK operations: createBook (POST /books)
        Source: handlers/books.go:42
```

**Markdown only.** The human report's three columns are a documented contract
(`docs/cli/commands.md:157-165`) and `--json` already carries `code` for machines; changing one surface
is the whole blast radius. This is also the prerequisite for the deferred `--allow <id>` work
(`2026-09-03-api-tags-breaking-change-gating.md:919-922`): an allowance surface is useless if the
report never shows the reviewer what to allow.

**D2. Group by category (G-9).** Partition the already-sorted findings (§1.2: `kind` sorts first) into
four blocks, each with a static, non-interpolated heading and its own count:

```
Summary: 3 breaking, 2 additive, 1 doc-only, 2 gating.

Breaking — gating (2)

    BREAKING  ...

Breaking — not gating (1)

    BREAKING  ...  (exempt on both sides; not gating)

Additive (2)

    ADDITIVE  ...

Documentation-only (1)

    DOC-ONLY  ...
```

Empty groups are omitted. This is the honest reading of #76's `API changes: 1 breaking, 2 additive`
sketch: the *content* it asks for, in the containment-safe layout we already have. The issue's bullet
form is not adopted, and §6 records why.

**D3. What must not change here.** No `<details>` fold. GFM does not render an indented code block
inside raw HTML, so folding would destroy the containment property that
`markdown_report_cannot_be_broken_out_of_by_analyzed_source` (`changes.rs:519-577`) exists to prove.
Every group heading is a compile-time constant plus an integer — never an interpolated value from the
graph.

### 5.5 WS-E — version the machine report

`MachineReport` (`crates/gnr8/src/changes.rs:15-21`) gains `schema_version: u32` as its first field,
with a `CHANGE_REPORT_SCHEMA_VERSION: u32 = 1` constant beside it. This mirrors
`graph_artifact.rs:15` and the hard version check at `main.rs:99-105`, and it is the moment to do it:
#76 is what turns `report.json` from a CLI convenience into an artifact other systems consume.

Four lines of code, one assertion added to `json_report_carries_base_policy_sides_and_summary`
(`changes.rs:404-440`), one sentence in `docs/cli/commands.md:152-155`.

### 5.6 WS-F — documentation

1. **`permissions:` in both examples.** `README.md:136-152` and
   `docs/operations/artifacts-and-ci.md:247-271` gain:

   ```yaml
   permissions:
     contents: read
     pull-requests: write
   ```

   with one sentence saying that without `pull-requests: write` the comment degrades to the job
   summary and the artifact, and that on fork PRs it degrades regardless (§3.2). This is the single
   highest-value doc change: today a reader who copies the README gets a warning on every PR forever
   and no hint that one line fixes it.
2. **Fix the stale pins.** `README.md:148` and `docs/operations/artifacts-and-ci.md:257` say
   `@v0.11.0 # first release with API change reporting`; it shipped in 0.12.0 (`CHANGELOG.md:12`,
   `:49-51`).
3. **New "Fork pull requests" subsection** in `docs/operations/artifacts-and-ci.md`, with the
   `workflow_run` recipe from §3.3: unprivileged `pull_request` job uploads the report *and the PR
   number*; privileged `workflow_run` job with `permissions: { actions: read, pull-requests: write }`
   downloads, validates the PR number as an integer before use, and comments. Include the two traps:
   the workflow file must be on the default branch, and `concurrency: ${{ github.ref }}` collapses all
   PRs into one group under `workflow_run`. Say explicitly that gnr8 does **not** ship this workflow —
   a composite action cannot declare triggers — and that `pull_request_target` is not recommended.
4. **Document annotations**: the level mapping, the 50-per-project cap, the input, that removals are
   not anchorable and why, and the `python3` requirement.
5. **Document the comment identity**: the marker key shape, that one comment exists per
   job + working-directories + base-ref, and that changing those orphans the old comment.
6. **`docs/cli/commands.md`**: the `Code:` line, the grouping, `schema_version`.
7. **`CHANGELOG.md`** under `## Unreleased` (`:10`, currently empty): Added — annotations, comment
   keying, `schema_version`; Fixed — non-bot tokens now upsert, partial reports survive a failed
   project, step-summary truncation.

### 5.7 Test strategy

Extend the repo's existing pattern, don't invent one. `make action-test` (`Makefile:95-100`) is the
gate; it runs inside `make check` (`:159`) and `.github/workflows/ci.yml:144`.

**`scripts/test-action-changes.sh`** — new cases in the established idiom (fake `gnr8` on `GNR8_BIN`,
argv logged with `printf '%q '`, `GITHUB_OUTPUT`/`GITHUB_STEP_SUMMARY` as plain files,
`grep`/inverted-`grep` assertions):

| Case | Asserts |
|---|---|
| marker is keyed | `report.md` contains `<!-- gnr8-api-changes:gnr8-api-changes-test-XXXXXXXX -->` and the key equals the `artifact-name` output |
| two projects, distinct keys | one marker per project block; both match their own `artifact-name` |
| mid-loop failure (fake exits 2 on project 2) | `report-root` and `artifact-name` are in `$GITHUB_OUTPUT`; project 1's block is in the summary; `combined-report` and `gating` are absent |
| oversized report | a fake emitting > 900 KiB produces the truncation notice and a summary file under 1 MiB |
| annotations off | `annotate-api-changes=false` produces no `::error`/`::warning`/`::notice` lines |

**`scripts/test-action-comment.sh`** — new cases (fake `gh` on `PATH`, behaviour switched by env):

| Case | Asserts |
|---|---|
| non-bot author | a comment authored by `some-app[bot]` carrying the marker is PATCHed, not appended |
| duplicates collapse | three matching ids → PATCH the first, DELETE the other two |
| marker from env | the jq filter contains `$MARKER`, and a *different* key is not matched |
| no author filter | `github-actions[bot]` no longer appears in the argv log (inverting the current `test-action-comment.sh:51`) |
| old marker adopted | a comment carrying the bare `<!-- gnr8-api-changes -->` is PATCHed, not duplicated |
| digest guard | a second run with an identical body performs the list call and **no** PATCH |

**New `scripts/test-action-annotations.sh`** driving `emit-action-annotations.py` against a fixture
`report.json` containing, deliberately: a gating breaking with `file`/`line`/`span`; an exempt
breaking; an additive; a doc-only; a removal with no `file`; a message containing `\n`, `%`, `::` and
`,`; and 60 findings to trip the cap. Assertions: exact `::error file=examples/bookstore/main.go,line=25,endLine=25,title=…::…`
lines; the working-directory join; percent-encoding of the hostile message; no output at all for the
doc-only finding; the unanchored removal skipped and counted; the cap notice naming both counts.
Plus python-level unit tests in `scripts/test_emit_action_annotations.py` for the encoder, wired into
`Makefile:102-103` alongside `release-notes-test`.

**Rust** — `crates/gnr8/src/changes.rs` golden tests. `markdown_report_carries_...` (`:456-517`) gains
the `Code:` line and the group headings; `markdown_report_cannot_be_broken_out_of_by_analyzed_source`
(`:519-577`) is extended with a finding whose `code` is hostile (it cannot be — codes are
`&'static str` literals — so instead assert the group headings are constants and the four-space
invariant still holds across group boundaries); `empty_markdown_report_is_explicit` (`:579-596`)
asserts no group headings when there are no findings; `json_report_carries_...` (`:404-440`) asserts
`schema_version`. One new test for a report with all four groups populated.

**Budget.** `scripts/check-ci-budget.py:13` caps every job at 5 minutes. The annotation emitter is one
python3 pass over a JSON file of tens of KB (the largest committed example graph is 27 KB;
`report.json` is the same order), and the new shell cases are stub-driven with no network. No new job
is needed — `make action-test` already runs at `ci.yml:144`.

**Dogfood.** `.github/workflows/generated-sdk-check.yml` keeps `permissions: contents: read`
(`:8-9`) — the repository has stated it does not want the comment on its own PRs (`:123-126`), and
that keeps the degraded path exercised on every PR, which is exactly the path #76's A7 is about. But
**widen `report_changes: "true"` back beyond `bookstore`** once WS-A lands: `:54-56` narrowed the
matrix to one project specifically "to keep the pull-request comment step from racing five jobs over
the same marker comment", and A1 removes that reason. Two projects is enough to prove the keys differ.

### 5.8 Sequencing

1. **WS-D + WS-E** (Rust, self-contained, no Action changes). Renderer tests are golden strings; land
   them first so the shell fixtures downstream can quote the real output.
2. **WS-A** (shell). Independently valuable; unblocks widening the dogfood matrix.
3. **WS-C** (shell). Touches the same file as A; land after so the diffs do not fight.
4. **WS-B** (new script + input). The largest new surface, and the one with an open empirical question
   (§3.5) — it should not block the other five.
5. **WS-F** (docs). Continuously, but the `permissions:` fix and the stale pins can land immediately
   and independently of everything else; they are the cheapest real improvement in this document.

### 5.9 What must NOT change

- **The CLI stays ignorant of GitHub.** No `ReportFormat` variant for annotations, no `--github-*`
  flag, no `$GITHUB_*` read in `crates/`. Every byte of workflow-command dialect stays in `action.yml`
  and `scripts/*action*`.
- **One renderer per format.** The Action never re-renders the Markdown, never parses it, and never
  derives report text from the JSON. Annotations are a placement of machine facts, and the JSON is the
  machine contract.
- **Containment of analyzed source.** Findings stay inside an indented code block; every value outside
  it stays HTML-escaped; every value stays collapsed to one line
  (`changes.rs:203-237`, proved by `:519-577`). Nothing in this plan introduces raw HTML around
  finding text.
- **The gate is independent of publication.** `action.yml:371-376` stays `always()` and keyed only on
  `gating`. No comment failure, annotation failure, size truncation or artifact problem may change the
  exit status — the existing warning-swallow at `:338-340` is the correct shape and stays.
- **No fallback chains.** One path per fact: the base graph is the committed artifact and nothing else
  (`base.rs:23-25`); the report is rendered once per format by the CLI; the annotation stream reads the
  JSON. Branches in this plan select *messages* or *preconditions*, never alternative derivations.
- **`pull_request_target` is not supported.** GitHub's own preference order is
  `pull_request` > `workflow_run` > `pull_request_target` (§3.3), and a composite action that runs a
  user-authored Rust `.gnr8` crate (`CLAUDE.md` rule 4) under a write-scoped token in a fork context
  would be a code-execution hazard, not a convenience. The `workflow_run` recipe is documented instead.
- **Vocabulary.** `scripts/check-invariants.sh:108` forbids `baseline` in any declaration and
  `:114` forbids the `--accept-generated-baseline` family; `:120` forbids `*compat*` as a path
  segment. The shipped vocabulary — `base`, `base-ref`, `changes`, `exempt-tag`, `gating`, `report`,
  `marker` — is already clean, and every new name here (`annotate-api-changes`,
  `emit-action-annotations.py`, `MARKER`, `schema_version`) passes. `docs/`, `scripts/`, `.github/` and
  `action.yml` are all inside the gate's scope (`:17-38`); `thoughts/` is not (`:7-8`).

### 5.10 Out of scope

Recorded so the plan is not quietly widened:

- **`--include-tag` (include-only gating).** Rejected for v1 in
  `2026-09-03-api-tags-breaking-change-gating.md:720-741` and `:886`; nothing in #76 changes that.
- **`--allow <id>` content-addressed allowances.** Open decision 2 of that document (`:919-922`).
  WS-D1 is a prerequisite for it, not a delivery of it.
- **Comparing pagination / SDK-runtime policy, response headers, or additional request-body variant
  schemas.** The documented limitation at `docs/cli/commands.md:141-144` stands unchanged; #76 is about
  publishing the report, not widening it.
- **Check Runs / the Checks API.** More permission (`checks: write`), the same fork blocker, and a
  second annotation mechanism — a fallback by another name.
- **Shipping a `workflow_run` workflow.** A composite action cannot declare triggers; this is a
  documented recipe the caller owns.
- **Any comparison of gnr8's output against another tool's output**, any "does this match what your
  previous generator produced" surface. Forbidden by CLAUDE.md 0.2 and not asked for. Comparing two
  *OpenAPI documents* would be legitimate; comparing two *graphs* is what `gnr8 changes` already does.
- **Publishing the report anywhere other than GitHub.** A second CI host is a second integration, not
  a flag on this one.

---

## 6. Alternatives considered and rejected

1. **Add a fourth `ReportFormat` to the CLI that prints workflow commands.** It would be the smallest
   diff and it would put GitHub's `::error file=` dialect inside `crates/gnr8/`, where nothing today
   knows GitHub exists. The integration layer is the correct home for an integration's protocol.
   **Rejected** — §5.2 B3.
2. **Have the Action re-render the report from the JSON into a nicer comment.** Two renderers for one
   report, contradicting `changes.rs:118-128` and `docs/cli/commands.md:149-150`, and guaranteed to
   drift. **Rejected.** The Action publishes what the CLI renders; when the format needs to change, the
   CLI's renderer changes (WS-D).
3. **Parse the Markdown to extract locations for annotations.** Couples the annotation stream to a
   presentation format and breaks the moment WS-D lands. **Rejected in favour of** reading the JSON,
   which is the machine contract and already written to disk at `run-action-changes.sh:89`.
4. **Adopt #76's literal bullet layout.** `- BREAKING: POST /books request field \`title\` is now
   required` is a Markdown list item containing text drawn from analyzed source. The current indented
   code block exists precisely so a crafted path or field name cannot inject markup into a comment, and
   `changes.rs:519-577` proves it. **Rejected for containment**; WS-D2 delivers the grouping the sketch
   is really asking for.
5. **Fold `Additive` and `Documentation-only` into `<details>`.** GFM does not render an indented code
   block inside raw HTML, so the fold would silently destroy the containment guarantee.
   **Rejected** — §5.4 D3.
6. **Support `pull_request_target` so fork PRs get comments.** It grants a write token in a fork
   context, and this Action's core operation is compiling and running a user-authored Rust crate
   (`CLAUDE.md` rule 4). GitHub's own ordering puts it last. **Rejected in favour of** documenting
   `workflow_run` — §5.6 F3.
7. **Suppress or delete the comment when there are no findings.** Optic comments "when there is
   something meaningful to report"
   ([setup-ci](https://web.archive.org/web/20240908150818/https://www.useoptic.com/docs/setup-ci)), and
   Bump.sh goes further with a `deleteComment` path when the diff empties (`src/github.ts:190`).
   Attractive for noise, but a report that is present only sometimes is a report a reviewer cannot
   trust the absence of, and deletion destroys the record that a previous push *did* have findings.
   The upsert already means one comment, edited, not one per push. **Rejected**, but recorded as
   Open Q 4 — it is a genuine judgement call and both peers landed on the other side of it.
8. **Add a `comment: "true"/"false"` input.** The comment is already conditional on being on a PR and
   on the token permitting it; a third switch adds a configuration surface without adding a capability.
   **Rejected** — a caller who does not want the comment sets `permissions:` accordingly, exactly as
   this repository does (`generated-sdk-check.yml:123-126`).
9. **Emit annotations for documentation-only findings.** Inline noise on every prose edit, on the
   surface where noise is most expensive. **Rejected**; they remain in the comment, the summary and the
   JSON.

---

## 7. Open decisions

1. **Where workflow-command annotations actually render.** Undocumented (§3.5). WS-B's entire value
   proposition for A8 rests on the Files-changed tab. **Confirm empirically on a real PR before WS-B
   ships**, and if annotations turn out to render only on the run summary, WS-B's priority drops below
   WS-A/C and the honest answer to A8 becomes "the comment, linked from the summary."
2. **Whether a cap of 50 annotations is right.** GitHub documents no cap (§3.5). 50 is borrowed from
   the Checks API's documented per-request batch, which is a defensible citation but not evidence about
   workflow commands. If a real large-refactor PR shows GitHub truncating earlier, the number moves —
   it is our constant, documented in our docs, not a platform fact.
3. **The module-root / working-directory join.** §5.2 B6: the join is exact when the pipeline's source
   inputs are the project root, which is true for all five committed examples. A pipeline with
   `inputs(["./cmd"])` would produce a path that does not exist in the workspace. The failure is benign
   (GitHub drops the annotation) but silent. Deciding whether the graph should carry a
   workspace-relative path — or whether the Action should verify the joined path exists before
   emitting — is a real design question this document does not settle.
4. **Whether to comment when there is nothing to report.** §6 item 7. Ties to how noisy the comment
   feels in practice on a repo with many small PRs; genuinely needs a week of real use to answer.
5. **`python3` as an Action-path prerequisite.** §5.2 B4 argues for it and makes its absence a named
   error behind an off switch. I did not verify `python3` on every runner image gnr8 supports, and
   `ubuntu-slim` explicitly ships "a minimal set of tools"
   ([GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).
   Confirm before WS-B ships; if it fails, the alternative is `jq` (same verification problem) or
   accepting the dialect in the CLI (§6 item 1), and the trade changes.
6. **Whether `report.json` should be a documented public artifact.** WS-E versions it, which implies a
   commitment. `docs/cli/commands.md:152-155` already describes the payload, so the commitment is
   arguably already made; making it explicit — or explicitly declaring it unstable — is a decision
   worth taking deliberately rather than by omission.
7. **Whether the repository should grant `pull-requests: write` to its own dogfood workflow.**
   `generated-sdk-check.yml:123-126` says the maintainers do not want the comment on gnr8's own PRs,
   and keeping `contents: read` keeps the degraded path exercised — which is exactly A7's requirement.
   But it also means the comment path is only ever tested against a stubbed `gh`
   (`scripts/test-action-comment.sh`). A separate, opt-in workflow on a throwaway PR would close that
   without changing the default. Not decided here.
