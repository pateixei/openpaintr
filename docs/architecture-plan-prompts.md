# Prompts for implementing the architecture plan

Prompts to run in Claude Code, one per session (or `/clear` between them), from the repository root on the development machine (native Ubuntu 26.04). They implement [`architecture-improvement-plan.md`](architecture-improvement-plan.md) in order. Each implementation prompt ends by asking for an independent review by the `plan-reviewer` subagent (`.claude/agents/plan-reviewer.md`).

Conventions for every prompt:

- One plan item per branch, named `plan/<item>-<slug>`, created from the branch of the previous item until the work is merged into `main`.
- Commit locally. Do not push or open pull requests until the owner says so (`AGENTS.md` sign-off rule).
- Evidence means the commands that were run and their output, not a statement that they passed.

## R — Reconcile the working tree with the CI branch

Run this first. The working tree on `main` has a large uncommitted refactor (46 modified files plus new modules such as `src/gui/session.rs`, `storage.rs`, `tool_options.rs`, `widgets.rs`, and `clipboard/`), and the branch `ci/0.1-continuous-integration` was created from `92fdc75` without it. That branch also reformatted the whole codebase, so merging or rebasing it would conflict in almost every file.

```text
Context: the working tree on main contains my finished, uncommitted refactor.
The branch ci/0.1-continuous-integration (already pushed) implements plan
item 0.1 on top of the old commit 92fdc75 and includes a whole-codebase
rustfmt commit, so it must not be merged or rebased.

1. On the current working tree, run `cargo build --locked`, `cargo test --locked`
   and `cargo test --locked --no-default-features`. If anything fails, stop
   and report; do not fix it yet.
2. Create the branch `refactor/gui-modules` from main and commit the working
   tree in two commits:
   a. source, tests, Cargo.toml/Cargo.lock, translations and README changes
      ("Split GUI modules and move persistence, sessions and clipboard
      into their own files");
   b. documentation only: docs/architecture-improvement-plan.md,
      docs/architecture-plan-prompts.md, .claude/agents/plan-reviewer.md and
      the edits to AGENTS.md, docs/roadmap.md, docs/arquitetura-plugins.md and
      docs/extensibilidade.md ("Record the architecture improvement plan and
      owner decisions").
   Check `git status` first: never commit target/, local settings files
   (atalhos.json, paleta.txt, recentes.json) or anything matched by .gitignore.
3. Create `plan/0.1-continuous-integration` from `refactor/gui-modules` and
   redo item 0.1 on it:
   a. bring over only `.github/workflows/ci.yml` and `deny.toml` from
      ci/0.1-continuous-integration (`git checkout <branch> -- <paths>`);
   b. run `cargo fmt --all` and commit the result alone ("Format the codebase
      with rustfmt");
   c. run `cargo clippy --locked --all-targets --all-features -- -D warnings`,
      fix every warning without changing behavior, and commit the fixes
      separately;
   d. commit the workflow and deny.toml.
4. Run every command of the CI workflow locally (fmt check, clippy, both test
   runs, and `cargo deny check advisories bans licenses sources` if cargo-deny
   is installed; otherwise say so) and show the output.
5. Do not delete or force-push ci/0.1-continuous-integration. Tell me it can
   be retired once the new branch is accepted.
6. Use the plan-reviewer subagent to review plan/0.1-continuous-integration
   against item 0.1, and report its findings.
```

## 0.4 — `unsafe` lint for the whole binary

```text
Implement item 0.4 of docs/architecture-improvement-plan.md on a branch
plan/0.4-unsafe-lint created from plan/0.1-continuous-integration. Follow
AGENTS.md and touch nothing outside the item.

Add `#![deny(unsafe_code)]` and `#![deny(unsafe_op_in_unsafe_fn)]` to
src/main.rs and put `#[expect(unsafe_code, reason = "...")]` on the single
`Backend::from_foreign_display` statement in src/gui/clipboard/wayland.rs,
keeping its SAFETY comment. Keep `#![forbid(unsafe_code)]` in src/lib.rs.

Prove the lint works: temporarily add an `unsafe {}` block elsewhere in the
GUI, show that the build fails, then remove it.

Done means: fmt check, clippy with -D warnings, `cargo test --locked` and
`cargo test --locked --no-default-features` pass, with output shown. Commit,
do not push. Then use the plan-reviewer subagent to review the branch
against item 0.4.
```

## 0.2 — Translation catalog check

```text
Implement item 0.2 of docs/architecture-improvement-plan.md on a branch
plan/0.2-translation-check created from plan/0.4-unsafe-lint. Follow AGENTS.md.

Add a test (tests/translations.rs, or a test in src/gui/i18n.rs if the
catalogs are only reachable there) asserting that each of the ten bundled
catalogs in translations/ has exactly the keys of en-us.json, the same
numbered placeholders ({0}, {1}, …) per key, and no empty values. Failure
messages must name the catalog and the key.

If the current catalogs fail the test, list the problems and stop before
fixing them; I will decide.

Prove the test works: temporarily remove a key from fr-fr.json and change a
placeholder in ja-jp.json, show both failures, then restore the files.

Done means: fmt, clippy -D warnings and both test runs pass, output shown.
Commit, do not push. Then use the plan-reviewer subagent to review the branch
against item 0.2.
```

## 0.3 — Per-tool performance baseline

```text
Implement item 0.3 of docs/architecture-improvement-plan.md on a branch
plan/0.3-perf-baseline created from plan/0.2-translation-check. Follow AGENTS.md.

Start in plan mode: read phase1_interactive_performance_baseline and the
headless test driver in src/gui, then propose how to add the workloads listed
in item 0.3 (as an #[ignore] test run with `cargo test --release -- --ignored`),
which metrics each one records, and the JSON-lines format written to
target/perf/. Wait for my approval before writing code.

After approval:
1. Implement the workloads on the typical (800×600, 4 layers) and large
   (2560×1440, 8 layers) documents. Record median and maximum frame time,
   VmHWM and undo bytes per history entry, plus the environment (uname -a,
   CPU model, rustc version, git commit).
2. Run it on this machine (native Ubuntu 26.04) with 2 warm-ups and 10 timed
   runs, the same method as phase 1.
3. In docs/roadmap.md, under the phase 4 Performance item, add the new
   baseline table and a note that development moved from WSL2 to native
   Ubuntu 26.04, so these numbers are not comparable with the phase 1 table.
   Do not change the phase 1 numbers or any completed item.
4. Add a CI step that runs the benchmark and uploads target/perf/ as an
   artifact without failing the build on timings.

Done means: fmt, clippy -D warnings and both test runs pass; the benchmark
output and the roadmap diff are shown. Commit, do not push. Then use the
plan-reviewer subagent to review the branch against item 0.3.
```

## Template for the remaining items

Use this for every later item, replacing the item number, branch, and base. For items marked as high risk in the review (2.1, 2.2, 2.4, 2.5, 3.1, 3.4, 4.2, 5.1, 5.6), split the work as described after the template.

```text
Implement item <N> of docs/architecture-improvement-plan.md on a branch
plan/<N>-<slug> created from <previous branch>. Follow AGENTS.md and the
item's text exactly; do not implement other items.

Start in plan mode: read the files the item names, then show me the plan,
the files you will change, and how each acceptance criterion will be
verified. Wait for my approval.

After approval, implement it. Done means: every acceptance criterion of the
item is met with evidence; fmt, clippy -D warnings, `cargo test --locked`
and `cargo test --locked --no-default-features` pass; for performance items,
before-and-after numbers from the item 0.3 benchmark; new user-facing text
has keys in all ten catalogs. Show the command output. Commit, do not push.
Then use the plan-reviewer subagent to review the branch against item <N>.
```

**High-risk items.** Run two sessions:

1. **Golden tests first.** "For item <N>, write only the tests that pin the current behavior byte for byte (pixels, coverage, undo contents) on the PDN fixtures and on the cases the item lists. Do not change production code. Show that they pass on the current code." Review and commit these before the optimization.
2. **Then the change**, with the template above, adding: "The golden tests from the previous commit must pass unchanged."

For item 5.1 (moving code out of `src/gui/mod.rs`), add: "Move code with scripted or editor moves; never retype code. Show `git diff --color-moved=zebra --stat` and confirm the diff contains only moves and `use` changes."
