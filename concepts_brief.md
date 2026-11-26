# OpenPaintR workspace and roadmap concept brief

**Date** 2026-11-25

**Status** Historical brainstorm synthesis. The [approved roadmap](docs/roadmap.md) defines persistence in Paint.NET PDN and Photoshop interoperability for PSD.

## Overview

Phase 1 describes workspace layout and controls before establishing where users struggle. Capture the current Linux editor before adding Phase 1 features. Use the study findings to propose the redesign scope and schedule. The redesign should have its own visual identity and help users complete layered raster workflows, with a mean System Usability Scale (SUS) score above 80/100. The target users, recruiting plan, device budget, and redesign priority still need a decision. See `docs/roadmap.md:5–16` and `README.md:7–15`.

## Phase 1 baseline and redesign evaluation

Freeze the OpenPaintR build and verify every shared task on Linux before running sessions. Record build and fixture IDs, starting document state, distribution, display protocol and compositor, portal, viewport and scale, and input device. Use equivalent layered PDNs with clear names and distinguishable artwork, visible and hidden layers, transparency, and known-good rendered output. Establish the expected result with reference files before involving participants. Verify the current Linux build first. A failed preflight means the baseline is pending. Classify fixture, importer, compositor, portal, and file-write failures so the results reveal which part of the workflow caused friction.

| Task | Observable outcome |
| --- | --- |
| Open a layered PDN | The expected document opens with the canvas and the intended set of layers available. |
| Locate and identify a layer | The participant selects the intended layer by name or by its contribution to the artwork. |
| Change opacity and undo a stroke | The intended layer reaches the requested opacity. The next, visible stroke is undone, restoring its affected pixels while retaining the opacity change. |
| Zoom and edit a detail | The participant reaches and edits a marked region. Choose a region reachable using today's Fit, 100%, and 200% zoom and two-axis scrolling. |
| Export the visible composition to PNG | A PNG at the requested fresh destination matches the expected visible composition and alpha. |
| Save, close, and reopen an editable project, once usable | Reopening preserves the supported canvas and layer state. The participant can continue editing a layer. |

The current build lacks editable project save and reopen. Assess that workflow in a separate session or after the comparable SUS measurement when a persistence build is usable. Include it in the common scored session once both compared builds support it. Apply the same rule to Phase 2 navigation additions. If substantial features change between the initial baseline and the redesign, consider a capability-matched build of the existing workspace over the same editing core. Record the extra QA needed to keep both builds and fixtures comparable. See `src/gui/mod.rs:121–198,223–363,392–465` and `docs/roadmap.md:13,22`.

Use goal-based prompts without naming commands. Set task start and stop events, time limits, assistance and retry rules, and denominators before collecting data. Reset independent tasks to a known state and specify dependencies for intentional sequences. Record unaided completion, elapsed time, and errors such as selecting the wrong layer, undoing an extra edit, or exporting to the wrong path. Counterbalance version order and equivalent files if participants use both builds. Matched separate groups reduce learning carryover and require more recruitment.

Give each participant the same 10-item SUS once after the comparable tasks for each version. Report mean, sample size, spread and an uncertainty interval, alongside per-task outcomes. Apply the threshold strictly to the redesign mean above 80/100. A pilot can expose friction and provide a directional score. Decide the evidence standard and sample size before using the study to accept a release. Review the interval and task failures alongside the point estimate. For a Brazilian Portuguese study, use the validated adaptation. [Original SUS instrument](https://germanupa.de/sites/default/files/2021-12/brooke1996sus.pdf) · [Brazilian Portuguese adaptation](https://aquichan.unisabana.edu.co/index.php/aquichan/article/view/16497)

## Interactive performance

In Phase 1, collect a separate reproducible benchmark on fixed Linux hardware. Record CPU, GPU and driver, build profile, display setup, desktop backend, input and fixture dimensions, layer counts and blend modes. Repeat cold and warm runs with typical and large fixtures. Measure from open to usable canvas, stroke input to presented frame, zoom and scroll frame times, export duration, and peak memory. Add save/reopen timings when available. Report medians, tail timings and failures. Separate time spent in dialogs, decoding, composition, texture upload, encoding, and disk writes where measurements permit.

The current GUI can run decoding and encoding synchronously and fully recompose and upload a texture after an edit. Those are starting points for profiling. A 4096 × 4096 document with 16 full-canvas RGBA8 layers uses 1 GiB for raw layer pixels before history and render caches. That is an illustrative memory calculation. The current PDN importer limits the total decoded layer pixels to 64 Mi pixels, so choose benchmark PDNs within that limit and report rejected sizes separately. Measure the limits before setting device budgets or choosing caching, partial updates, SIMD, or a GPU backend. Phase 5 covers the broader benchmarking and optimization program. See `src/gui/mod.rs:121–198,366–389`, `src/lib.rs:158–185`, and `docs/roadmap.md:68–70`.

## Persistence and phase dependencies

**Phase 1 document lifecycle.** Specify four acceptance checks with separate evidence.

| Workflow | Evidence needed |
| --- | --- |
| Native Save and Save As | A versioned project reopens with documented dimensions, RGBA pixels and alpha, layer order and names, visibility, opacity, and blend-mode IDs. The user can continue editing. Define version and size limits and decide whether active layer, selection, viewport, history, and other metadata persist. |
| PDN import and composition | Versioned 3.x, 4.x, and 5.x fixtures verify supported fields and visible output. Record source version, provenance, expected results, checksums and redistribution rights. Reconcile the README description of normal composition with non-Normal rendering and publish which blend modes and outputs were verified. |
| PDN writing, if approved | Define interoperability profiles by version and verify produced files by reopening in OpenPaintR and declared Paint.NET versions. The JFLib-generated decoder test establishes an import case. |
| PNG export | Compare visible composition and alpha to reference output. Keep editable project Save easy to distinguish from PNG delivery and from importing a PDN. |

Keep a saved revision per document, independent of the bounded undo stack and redo branches. Preserve the current document and last good save on a failed or canceled save. Keep an imported source intact during a first native save. Specify dirty-close behavior, file replacement and durability, and newer or malformed file handling. The history code currently records layer indices and undoes addition by removing the top layer. Reliable layer identity or proven reversible indexing is a prerequisite for editing layer order, deletion and merge. For merge, define hidden-layer, blend-mode and partial-alpha behavior, including effects of underlying layers. Per-document state, such as active layer, view and pending gesture, needs clear ownership when expanding from one document to tabs. Keep shared editing and persistence behavior reusable from either workspace. See `docs/roadmap.md:9–16,102`, `README.md:15,19,33–35`, `src/gui/editor.rs:8–59,71–104`, `src/lib.rs:35–47,63–73,115–119`, and `src/pdn_io.rs:14–99,115–152`.

**Phases 2 and 3.** Phase 1 can establish where per-document selection state belongs. Phase 2 supplies actual selection operations and richer navigation, and needs an interim DPI policy before Phase 5 metadata. Preserve undo and redo in the workspace and address the documented Ctrl+Shift+Z conflict with Girar/Zoom as part of the Phase 2 shortcut decision. Phase 3 tool controls can reuse workspace interaction patterns and be evaluated as those tools ship. See `docs/roadmap.md:9,12,18–36,63`.

**Phase 4 adjustments and effects.** Treat menu discoverability and implementation coverage as separate evidence. Enable commands as features become available. Retain the 13 adjustments and 48 effects as the proposed catalog. The six starter effects are included in the 48, with historical variants evaluated separately. Use the starter effects to validate the selection mask and alpha rules, initial RGBA8 working-color assumptions before managed color, edge handling, cancellation and obsolete-preview suppression, preview/commit agreement, and one undo entry per committed operation. Define representative input fixtures, parameter ranges and defaults, deterministic seeds when applicable, and output tolerances per effect before estimating the whole catalog. Profile CPU behavior before committing work to a GPU backend. See `docs/roadmap.md:14,38–59,63,97–103`.

**Phase 5 formats and Linux integrations.** Track per-codec read/write direction, bit depth, alpha, metadata and animation rules, distro dependencies, and behavior for unsupported variants. Verify portals, file and clipboard integration on Wayland and X11, and package/device support separately from the fixed-machine study. For PSD, establish how imported offsets map to the current full-canvas raster layer model, including clipping and memory cost. Specify warnings and flattened fallback for unsupported layer structures. Windows Paint.NET and Photoshop serve as reference generators for compatibility tests. See `docs/roadmap.md:61–95,99–103`.

## Alternatives weighed

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Target the hardest tasks with changes in the current workspace | Fastest way to test improvements to labels, shortcuts, active layer visibility and discoverability. | Local fixes may leave friction across panels or tasks. |
| Prototype a distinct workspace on shared document and editing commands | Supports layout comparison while keeping workflow meaning and observable outputs consistent. A tool rail and persistent layer inspector favor recognition. A collapsible contextual inspector leaves more room for the canvas. | Persistent panels use canvas space. Collapsible controls can hide frequent actions. Both require interaction testing across document, history, and keyboard paths. |
| Prototype a different shell if the baseline shows a toolkit limitation | Can test an interaction or platform requirement the current shell cannot deliver. | A temporary command bridge and another shell add routing, maintenance, and Linux integration work. |
| Wait for broad catalog coverage or combine the redesign and renderer rewrite | Can exercise a wider set of eventual features in one evaluation. | Delays feedback, increases regression scope, and makes it harder to attribute task or SUS changes to the workspace. |

The reviewers favored starting with the baseline and prototyping the specific workflows it identifies. No layout, toolkit, or change in phase sequence has been approved.

## Decision checkpoints

1. Confirm study participants and recruitment, the Linux device and input setup, fixture coverage, task rules, and the comparison design. Budget preparation, recruiting, moderation, and analysis before setting a date.
2. Run the Phase 1 baseline and separate performance checks. Review the common-task results, persistence readiness and observed friction. Choose whether to prioritize save and reopen once available or other workspace workflows, and decide redesign scope and scheduling at that point.
3. Prototype the chosen approach with shared editing behavior. Compare capability-matched builds, review regressions and resource measurements, then assess the redesign against mean SUS >80/100 and the task outcomes. Budget implementation and retesting separately from catalog and compatibility work.

## Reviewer status

| Lens | Status |
| --- | --- |
| Design critic | Completed |
| Codebase details | Completed |
| Zoom out | Completed |
| Comprehension | Completed |
| Implementation readiness | Completed |
| Structural | Completed |
| Inventive catalyst | Completed |

All seven reviewers confirmed their assigned readings and returned independent reports. The study and benchmarks are proposals. The current capabilities cited here were assessed from documentation and source inspection. This brief proposes roadmap changes for discussion. No new phase order, redesigned layout, or performance target has been approved.
