# Migration Index

This directory tracks the current high-level distro-builder migration work as numbered design tracks.

## Tracks

1. [01_MIGRATION_FEDORA_SWAP.md](01_MIGRATION_FEDORA_SWAP.md)
   Status: completed
   Scope: replace the current Rocky Stage 01 DVD/rootfs source path with Fedora Server DVD sourcing for the Levitate/Ralph family.

2. [02_MIGRATION_BOOTC.md](02_MIGRATION_BOOTC.md)
   Status: cancelled
   Scope: cancelled; the repo is keeping the current A/B runtime/update model instead of migrating to `bootc`.

3. [03_MIGRATION_STAGELESS.md](03_MIGRATION_STAGELESS.md)
   Status: ready
   Scope: active filesystem-first migration away from stage-numbered composition toward explicit products, artifact transforms, and release-engineering outputs, followed by a final Phase 9 purge of both literal `stage` naming and numbered stage-artifact families like `00Build`, `01Boot`, and `02LiveTools`.

4. [04_MIGRATION_RING_MODEL.md](04_MIGRATION_RING_MODEL.md)
   Status: in_progress
   Scope: replace the remaining mixed stage-era manifest ownership with ring-native ownership across `identity`, `build_host`, `ring3_sources`, `ring2_products`, `ring1_transforms`, `ring0_release`, and `scenarios`, then delete the old stage-era manifest families entirely.

5. [05_MIGRATION_RING_EXECUTION_MODEL.md](05_MIGRATION_RING_EXECUTION_MODEL.md)
   Status: in_progress
   Scope: make ring/process orchestration real after ownership migration by requiring outer-target selection, inward dependency resolution, and inner-to-outer materialization without manual stage choreography.

6. [06_MIGRATION_VARIANT_LAYOUT.md](06_MIGRATION_VARIANT_LAYOUT.md)
   Status: in_progress
   Scope: make the per-OS `distro-variants/<distro>` filesystem tree reflect the ring/owner model by moving flat root files into sibling owner directories without turning rings into nested physical containers.

## Recommended Order

1. Keep the current A/B runtime/update model and improve its contract/install/test ownership as part of the product-model transition.
2. Start the filesystem-first migration from `distro-contract`, then move builder/test routing after product ownership is real.
3. After Track 03 semantics are in place, start Track 04 to redistribute mixed manifest ownership into the ring model before attempting final naming purges.
4. After Track 04 ownership is real, complete Track 05 so the planner and default operator flow are ring-native in execution as well as naming/ownership.
5. After Tracks 04 and 05 are materially stable, run Track 06 to align the physical variant directory tree with the ring/owner model without mixing layout churn into ownership or orchestration debugging.

## Why This Split Exists

- The Fedora swap was the concrete near-term migration that unblocked the later tracks.
- The `bootc` track was evaluated and cancelled in favor of keeping the current A/B model.
- The filesystem-first/product-model migration is now the primary architecture track and should start at contract ownership instead of surface-level CLI renames.
- The ring-model track exists because Track 03 revealed the remaining problem is mixed ownership, not just leftover stage vocabulary.
- The ring-execution track exists because ownership migration alone does not guarantee that the real build path stops behaving like a stage-driven system.
- The variant-layout track exists because even correct ownership and execution still leave the repo hard to reason about if every variant root remains a flat mixed-owner directory.

## Audit Snapshot

Date: 2026-03-27

### Landed on the canonical path

- Track 04 ownership is materially landed for active variants:
  - owner directories
  - short ring manifest filenames
  - nested `build-host` support paths
  - `ring0/hooks/*`
  - `ring2/overlays/live/*`
- Track 05 execution is materially landed on canonical release/product entrypoints:
  - planner-owned release prerequisite closure
  - planner-owned resolved parent rootfs inputs for product preparation
  - canonical scenario script installation
  - canonical scenario-first operator/docs cleanup on the default path
- Track 06 layout is materially landed for active variants:
  - active variants visibly follow the sibling owner-directory tree

### Still open

1. Track 06 compatibility-window closure
   - `distro-contract/src/variant.rs` still loads flat-root manifests
   - `distro-contract/src/variant.rs` still accepts legacy owner-dir ring filenames
   - `distro-contract/src/variant.rs` still accepts `profile_overlay` as a compatibility key
2. Track 03/05 stage-residue reduction
   - `distro-contract` validation/runtime/error surfaces still center `StageId`
   - `distro-builder/src/bin/artifact_paths.rs` still exposes compatibility stage-path helpers

### Recommended Next Slices

1. Close the variant-layout compatibility window in `distro-contract` once fixtures/tests/docs are updated.
2. Reduce validation/reporting stage residue after the default UX no longer teaches stage-first operation.
3. Harden compatibility aliases further only after the canonical path stays stable.
