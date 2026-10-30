# Varmint agent instructions

Varmint is a lightweight gaming-focused Linux VM for Apple Silicon Macs.

The project intentionally keeps its own VMM and host integration relatively small. Graphics translation is primarily implemented in upstream dependencies such as Neptune, virtio-win-mesa, virglrenderer, DXMT, D3DMetal, ANGLE, and MoltenVK.

The goal when working on Varmint is not only to make something work, but to keep the resulting system understandable and maintainable by humans.

## Architecture

Respect the existing ownership boundaries.

- `src/vmm.rs` is primarily VM setup, lifecycle, and composition.
- `src/devices/` owns runtime device composition, MMIO routing, and host-to-device event delivery.
- `src/virtio/` owns virtio device behavior and protocol semantics.
- `src/display/` owns scanout, host presentation, presentation-side IOSurface/ANGLE integration, and display statistics. Renderer-facing EGL/ANGLE winsys integration lives under `src/virtio/virgl_ffi.rs`.
- Host backends such as audio, networking, storage, clipboard, and stdio should remain independent of guest protocol details where practical.
- Guest provisioning and guest-side graphics mode/backend selection belong under `guest/` and related scripts. The host Neptune backend (`dxmt` / `d3dmetal`) is configured and applied on the host side.
- Graphics implementation fixes that logically belong to Neptune, Mesa, virglrenderer, DXMT, D3DMetal, or another dependency should not be reimplemented as Varmint-specific policy in the VMM.

These boundaries are not immutable. A task may reveal that an existing boundary is wrong. If a change crosses several layers or introduces a new architectural concept, make that decision explicit and explain why the existing ownership model is insufficient.

Do not create cross-layer shortcuts merely because they make the current task easier.

## Development workflow

Investigation code and production code have different standards.

During investigation, temporary logging, hardcoded experiments, instrumentation, alternative implementations, and disposable changes are allowed when they help establish evidence.

Once the cause is understood, reconsider the implementation from scratch. Do not automatically preserve investigation scaffolding in the final solution.

Before considering work complete, ask:

- What is the actual root cause?
- Which layer owns the fix?
- Can the final diff be smaller now that the problem is understood?
- Did temporary instrumentation or experimental structure leak into production code?
- Did the change introduce duplicated state or a new lifecycle unnecessarily?
- Is a new abstraction actually needed, or can an existing project concept express the behavior?

Prefer boring, direct code over speculative abstraction.

Do not introduce managers, services, providers, generic event buses, trait hierarchies, configuration knobs, background threads, or duplicated state unless the problem genuinely requires them.

Do not perform unrelated cleanup or refactoring as part of a feature or bug fix. Mention worthwhile follow-up cleanup separately.

## Code comments

Do not add comments that merely restate what the code already says. Prefer clear names, types, and structure over comments.

Add comments when they preserve non-obvious information: the rationale for an unusual implementation, invariants, lifetime or synchronization constraints, external API or specification requirements, or the reason behind a workaround.

Keep comments short, concrete, and in normal technical language. Do not narrate obvious control flow or generate documentation-like prose from the implementation.

If removing a comment would not make the code harder to understand or easier to accidentally break, remove it.

## Commit messages

Keep commit messages concise, specific, and natural.

Follow the repository's existing style. Prefer a simple description of what changed, such as:

`neptune: fix resource lifetime handling`
`vmm: add gamepad input`
`display: report presentation metrics`

Do not write verbose, overly polished, or documentation-like commit messages.
Avoid filler such as "implement comprehensive support for", "enhance robustness of", or long explanations of motivation when a direct description is enough.

The subject should read like something a human maintainer would naturally type after making the change.
Use the commit body only when important context would otherwise be lost.

## Graphics dependency changes

External dependency checkouts are disposable development state.

Do not finish a task with the only meaningful changes living in a local checkout such as `virtio-win-mesa-neptune`, `neptune-protocol`, DXMT, virglrenderer, or a build dependency directory.

A dependency change intended for Varmint must ultimately be represented by the repository's normal pinned-version / patch mechanism, unless the task is explicitly only an investigation.

Treat upstreamability as a design constraint.

A dependency fix should ideally make sense without knowledge of a specific Varmint game configuration. Prefer fixing API semantics, synchronization, resource management, translation correctness, or a generally reproducible workload over adding application-specific behavior.

Do not optimize for a low patch count at the expense of correctness. Several small independent semantic fixes are better than one opaque workaround.

## Performance work

Performance work must be evidence-driven.

Establish a baseline before changing behavior. Prefer structured measurements and reproducible workloads over subjective impressions.

When a game exposes a bottleneck, try to reduce the relevant behavior to a small synthetic workload that exercises the real graphics path. Validate that the synthetic workload is a useful proxy before relying on it for optimization.

Do not accept a performance change solely because it improves the target workload. Check relevant regression workloads before treating it as landable.

Small performance differences may be measurement noise. Use repeated measurements when the expected effect is small.

## Correctness and visual debugging

Prefer reducing game bugs to deterministic semantic or rendering reproducers. Once a failure is localized to a specific resource, data path, or API mechanism, move to a synthetic reproducer that preserves the workload's relevant topology (aliasing, offsets, sizes, lifetimes, queues, synchronization). A passing reproducer rules out only the configuration it exercised.

When a backend or mode toggle changes a symptom, confirm from the running process that it took effect, then audit what Varmint and its dependency glue do differently in each mode before reverse-engineering the backend itself.

Games are important integration tests, but should not become the inner development loop when a smaller reproducer is practical.

Use existing platform debugging facilities before building large Varmint-specific debugging infrastructure. Add instrumentation incrementally in response to demonstrated blind spots.

Avoid building a general capture/replay, tracing, or telemetry framework unless repeated real investigations show that simpler tooling is insufficient.

When evidence isolates a bug to a specific platform, backend, OS version, API generation, or third-party dependency, check relevant authoritative documentation and known issues before starting broad instrumentation. Use external research to narrow the hypothesis space, not as a substitute for local evidence.

## Tests

Keep production code easy to read from top to bottom. Small inline unit-test modules belong at the end of the source file, after all production code. Substantial test modules should preferably live in sibling test files. Do not expose private implementation details solely to make them testable.

A bug fix should include the smallest practical regression test or reproducer.

Tests should target the semantics that were wrong, rather than encoding incidental behavior of one game.

For graphics work, distinguish between:

- protocol/semantic correctness,
- synthetic end-to-end workloads,
- performance regression checks,
- real-game integration validation.

Not every change requires every level, but significant graphics changes should not rely only on manual game testing.

## Agent behavior

Work autonomously inside the task's stated scope. Do not ask the user to manually perform routine steps that can be performed with the available development environment.

Use hypotheses rather than random experimentation:

observation → hypothesis → experiment → result.

If a hypothesis is falsified and the available evidence does not motivate a specific next step, stop the investigation. An experiment falsifies a hypothesis only if it was capable of detecting the effect on the failing path. For a negative A/B result, verify that the toggle actually changes that path using a positive control, runtime evidence, or source-grounded proof. Summarize what was established, what tested negative, and what remains unknown. Do not invent another experiment merely to continue making progress.

Record negative results with their scope: what was exercised, under which conditions, and what the method could not detect. Handoffs and investigation notes must not promote "not observed" to "ruled out".

It is acceptable for an investigation to end without a fix.

When reporting completed work, distinguish clearly between:

- evidence established,
- temporary experiments performed,
- final changes intended to remain,
- tests or measurements performed,
- unresolved uncertainty.

Optimize for a codebase that a human maintainer can still understand without reconstructing the agent's investigation history.

## Neptune development workflow

Use the existing Neptune development workflow instead of inventing a separate guest deployment path.

Preferred interfaces include:

- `make neptune-sync` to verify guest dependency trees against the current pinned revisions and patch stack.
- `make neptune-sync-force` to restore the guest trees to the current Varmint patch stack while preserving build directories.
- `make neptune-build` to build guest Neptune binaries.
- `make neptune-capture` to capture tracked guest-side dependency changes back into the development patch area.
- SSH through `${VARMINT_GUEST_SSH:-varmint-dev}` for deterministic guest commands.

Prefer deterministic command channels such as SSH or a serial/maintenance shell over GUI terminal automation. Do not use GUI typing, clipboard automation, temporary HTTP transfer, or similar mechanisms while the existing command-line workflow is sufficient.

Choose the development side according to the layer:

- For experimental guest Neptune/Mesa changes, prefer the guest when it provides the shortest build/test loop. Once validated, move the minimal resulting change into Varmint's normal patch mechanism.
- For host virglrenderer, VMM, or other host-only changes, develop and build on the host.
- For protocol generator or schema changes that affect both guest and host, make one canonical generator change and regenerate both sides together. Do not independently hand-edit generated guest and host protocol code.

External dependency checkouts may contain temporary experimental changes during investigation. The repository patch stack remains the durable source of truth for changes intended to remain.

Do not create an alternative deployment or synchronization mechanism unless the existing workflow is demonstrably insufficient. If it is insufficient, establish the concrete limitation before replacing it.

## Runtime validation

Before runtime validation:

- Run basic source-state checks such as formatting or `git diff --check` where relevant before spending time on builds or VM testing.
- Use only artifacts built from the current checkout unless the task explicitly says otherwise.
- Do not launch an installed, stale, or alternate Varmint.app, including `/Applications/Varmint.app` or test installations, without explicit instruction.
- Verify the executable, bundle, VM disks, and relevant process IDs belong to the intended current build.
- When a protocol or ABI change affects both host and guest, verify both sides have been rebuilt from compatible versions before running graphics tests.
- Do not knowingly continue validation with mismatched host and guest protocol versions.

## Interactive game state
- Do not silently wait for a game state that no longer exists.
- If validation requires restarting a game, reproducing a menu/state, or other manual interaction, stop and tell the user exactly what state is needed before continuing.
- Do not assume a frozen or terminated render context will recover on its own.

## Screen/privacy
- Do not start screen recording, desktop capture, or continuous screen observation without explicit user permission for that specific task.
- GPU process captures such as gpucapture are allowed when they target the intended renderer process and do not capture the desktop.
- Prefer logs, GPU traces, synthetic tests, and user-provided screenshots over screen recording.