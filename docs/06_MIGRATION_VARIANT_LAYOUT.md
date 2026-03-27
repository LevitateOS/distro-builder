# 06 Variant-Directory Layout Migration

Status: in_progress

## Purpose

Make the on-disk `distro-variants/<distro>` filesystem layout match the
ring/owner model that Tracks 04 and 05 define semantically.

This track exists because the repo currently has a mismatch:

- logical ownership is increasingly ring-native
- orchestration is becoming ring-native
- but the physical variant directory layout is still mostly a flat root with
  mixed-owner files

That flat layout is not a semantic correctness bug by itself.
It is a maintainability and operator-comprehension bug.

Track 04 solves canonical ownership.
Track 05 solves canonical targeting/execution.
This track solves canonical filesystem layout.

## Exact Concern

The concern is:

- if each OS is supposed to have its own full ring stack
- then that should be visible in the per-OS directory tree
- without implying the wrong physical nesting model

Important clarification:

- rings are dependency layers
- they are not parent directories that physically contain each other

So the correct filesystem model is not:

- `ring0/ring1/ring2/ring3` nested inside each other

The correct filesystem model is:

- one variant directory per OS
- sibling owner/ring directories inside that variant

That keeps the mental model honest:

- Ring 0 is the outer target
- Ring 3 is the innermost source layer
- but neither physically "contains" the other on disk

## Non-Goals

This track does not:

- redefine ring ownership semantics
- redefine planner execution semantics
- move canonical shared scenario scripts into per-variant directories
- duplicate shared release helpers into every distro directory
- change the scenario/test ladder model

## Current Repo Reality

Today each active variant already follows the owner-directory model on the
canonical path:

- owner manifests:
  - `identity/identity.toml`
  - `build-host/build-host.toml`
  - `ring3/sources.toml`
  - `ring2/products.toml`
  - `ring1/transforms.toml`
  - `ring0/release.toml`
  - `scenarios/scenarios.toml`
- nested build-host support:
  - `build-host/kernel/kconfig`
  - `build-host/recipes/kernel.rhai`
  - `build-host/evidence/build-capability.sh`
- ring0 hook ownership:
  - `ring0/hooks/build-release.sh`
  - `ring0/hooks/boot-release.sh`
  - `ring0/hooks/live-tools-release.sh`
- ring2 overlay ownership for OpenRC variants:
  - `ring2/overlays/live/**`
- shared release helpers:
  - `_shared/ring0/hooks/build-release.sh`
  - `_shared/ring0/hooks/release-artifacts.sh`

What remains is mostly compatibility surface and documentation cleanup:

- flat-root manifest compatibility still exists in the resolver for legacy
  scaffolds and tests
- legacy owner-dir manifest filenames still load temporarily for migration
  safety
- some migration docs still describe earlier intermediate layouts

## Audit Snapshot

Date: 2026-03-27

### What is landed

- active variants now use sibling owner directories on disk
- canonical ring manifest filenames are the short owner basenames:
  - `ring3/sources.toml`
  - `ring2/products.toml`
  - `ring1/transforms.toml`
  - `ring0/release.toml`
- canonical `build-host` support and `ring0/hooks` paths are already migrated
- OpenRC live overlay seeds already live under `ring2/overlays/live/`

### What still remains

1. Compatibility loader removal
   - `distro-contract/src/variant.rs` still accepts:
     - flat-root manifests
     - legacy owner-dir ring filenames such as `ring3-sources.toml`
2. Legacy key removal
   - `distro-contract/src/variant.rs` still accepts `profile_overlay` as a
     compatibility alias for `seed_overlay`
3. Fixture/doc cleanup
   - some tests and migration docs still intentionally exercise or describe the
     older compatibility layouts

### Recommended next slices

1. Update remaining fixtures/docs so canonical owner-layout examples dominate.
2. Remove flat-root and legacy owner-filename loading from the resolver.
3. Remove the `profile_overlay` compatibility alias after the resolver cleanup lands.

## Canonical Target Tree

Every active distro variant should follow this model:

```text
distro-variants/
  _shared/
    ring0/
      hooks/
        build-release.sh
        release-artifacts.sh

  <distro>/
    README.md

    identity/
      identity.toml

    build-host/
      build-host.toml
      kernel/
        kconfig
      recipes/
        kernel.rhai
      evidence/
        build-capability.sh

    ring3/
      sources.toml
      assets/
        rootfs-source/

    ring2/
      products.toml
      overlays/
        live/
        installed/

    ring1/
      transforms.toml

    ring0/
      release.toml
      hooks/
        build-release.sh
        boot-release.sh
        live-tools-release.sh

    scenarios/
      scenarios.toml
      assets/
        variant/
```

Notes:

- `assets/` subdirectories are optional and only need to exist when a variant
  actually owns extra files there.
- `README.md` may remain at variant root.
- variant root should otherwise contain only owner/ring directories.
- canonical shared scenario scripts remain in:
  - `testing/install-tests/test-scripts/`

## Canonical Layout Rules

### 1. Variant root stays minimal

Allowed at variant root:

- `README.md`
- owner/ring directories:
  - `identity/`
  - `build-host/`
  - `ring3/`
  - `ring2/`
  - `ring1/`
  - `ring0/`
  - `scenarios/`

Forbidden at variant root after migration:

- free-floating `*.toml` owner manifests
- free-floating release hook scripts
- free-floating `kconfig`
- free-floating `build-capability.sh`
- root-level `recipes/`
- root-level `profile/`

### 2. Rings are siblings, not nested containers

The correct tree is:

- `<variant>/ring0/`
- `<variant>/ring1/`
- `<variant>/ring2/`
- `<variant>/ring3/`

It must not become:

- `<variant>/ring0/ring1/ring2/ring3/...`

Nested physical rings would falsely imply containment ownership.
The repo should model rings as layered siblings.

### 3. Canonicalize owner manifest basenames by owner

The canonical owner filenames are now:

- `identity/identity.toml`
- `build-host/build-host.toml`
- `ring3/sources.toml`
- `ring2/products.toml`
- `ring1/transforms.toml`
- `ring0/release.toml`
- `scenarios/scenarios.toml`

Legacy owner-dir ring filenames remain loader-compatible temporarily, but they
are no longer canonical.

### 4. Support assets live under their owner

Examples:

- `kconfig` belongs under `build-host/kernel/`
- variant-local evidence hooks belong under `build-host/evidence/`
- release hooks belong under `ring0/hooks/`
- variant-local live overlay material belongs under `ring2/overlays/`

### 5. Shared assets mirror owner layout

Shared helpers under `_shared/` should use the same ownership logic:

- shared release helpers under `_shared/ring0/hooks/`

This avoids a clean per-variant layout that still relies on a flat shared root.

### 6. Scenario script ownership stays global

The scenario evidence filenames in `scenarios.toml` name installed/on-ISO
scripts such as:

- `live-boot.sh`
- `live-tools.sh`
- `install.sh`
- `installed-boot.sh`
- `automated-login.sh`
- `installed-tools.sh`

Those should remain canonical shared test assets under
`testing/install-tests/test-scripts/`.

They must not be copied into per-variant tree layout as a second canonical owner.

## Exact Current-To-Target Mapping

### Per-Variant Common Mapping

| Current path | Target path |
|---|---|
| `<variant>/identity.toml` | `<variant>/identity/identity.toml` |
| `<variant>/build-host.toml` | `<variant>/build-host/build-host.toml` |
| `<variant>/ring3/sources.toml` | `<variant>/ring3/sources.toml` |
| `<variant>/ring2/products.toml` | `<variant>/ring2/products.toml` |
| `<variant>/ring1/transforms.toml` | `<variant>/ring1/transforms.toml` |
| `<variant>/ring0/release.toml` | `<variant>/ring0/release.toml` |
| `<variant>/scenarios.toml` | `<variant>/scenarios/scenarios.toml` |
| `<variant>/kconfig` | `<variant>/build-host/kernel/kconfig` |
| `<variant>/recipes/kernel.rhai` | `<variant>/build-host/recipes/kernel.rhai` |
| `<variant>/build-capability.sh` | `<variant>/build-host/evidence/build-capability.sh` |
| `<variant>/build-release.sh` | `<variant>/ring0/hooks/build-release.sh` |
| `<variant>/boot-release.sh` | `<variant>/ring0/hooks/boot-release.sh` |
| `<variant>/live-tools-release.sh` | `<variant>/ring0/hooks/live-tools-release.sh` |

### Variant-Specific Mapping

| Current path | Target path |
|---|---|
| `acorn/profile/live-overlay/**` | `acorn/ring2/overlays/live/**` |
| `iuppiter/profile/live-overlay/**` | `iuppiter/ring2/overlays/live/**` |

### Shared Mapping

| Current path | Target path |
|---|---|
| `_shared/build-release.sh` | `_shared/ring0/hooks/build-release.sh` |
| `_shared/release-artifacts.sh` | `_shared/ring0/hooks/release-artifacts.sh` |

## Required Loader And Tooling Changes

This track is only complete if path resolution stops being open-coded in
multiple crates.

### 1. `distro-contract` must become the canonical variant-path resolver

The main path-sensitive owner is:

- `distro-contract/src/variant.rs`

It currently hardcodes flat layout assumptions such as:

- `variant_dir.join("identity.toml")`
- `variant_dir.join("build-host.toml")`
- `variant_dir.join("ring3/sources.toml")`
- `variant_dir.join("ring2/products.toml")`
- `variant_dir.join("ring1/transforms.toml")`
- `variant_dir.join("ring0/release.toml")`
- `variant_dir.join("scenarios.toml")`
- `variant_dir.join("kconfig")`
- `variant_dir.join("recipes/kernel.rhai")`
- `variant_dir.join("build-capability.sh")`

This should be replaced by a single canonical helper surface, for example:

- `VariantOwnerPaths`
- or equivalent helper functions in `variant.rs`

That resolver must own:

- manifest paths
- build-host support-file paths
- release hook paths
- variant-local overlay asset roots

No other crate should open-code those paths after this migration.

### 2. `distro-builder` distro discovery must stop probing flat files

`distro-builder/src/bin/workflows/parse.rs` currently discovers distros by
checking for `identity.toml` directly under the variant root.

After migration it should discover variants via the canonical path helper and
the owner path:

- `identity/identity.toml`

### 3. Release hook lookup must move to `ring0/hooks`

Current product metadata uses root-level script names:

- `build-release.sh`
- `boot-release.sh`
- `live-tools-release.sh`

Current lookup in:

- `distro-builder/src/bin/workflows/release_hook.rs`

assumes:

- `bundle.variant_dir.join(release_hook_script)`

That should change to one of:

1. `BuildProduct` stores a canonical relative hook path such as:
   - `ring0/hooks/build-release.sh`
2. or the resolver maps a product identity to a ring0 hook path

The final path must not be root-level.

### 4. Build-host support file resolution must move under `build-host/`

The following path-sensitive code must stop assuming root-level `kconfig`:

- `distro-builder/src/pipeline/kernel.rs`
- `distro-contract/src/runtime.rs`
- `xtask/src/tasks/kernels/common.rs`
- `testing/install-tests/src/preflight.rs`

Canonical target:

- `build-host/kernel/kconfig`

The same applies to:

- `build-host/evidence/build-capability.sh`
- `build-host/recipes/kernel.rhai`

This also implies updating:

- `distro-contract/src/build_host_legacy.rs`

so the current constants:

- `REQUIRED_VARIANT_KCONFIG`
- `REQUIRED_VARIANT_RECIPE_DECL`

no longer point at root-level paths.

### 5. Ring 2 overlay paths must stop using root-level `profile/`

Current variant-specific overlay declarations include:

- `distro-variants/acorn/profile/live-overlay`
- `distro-variants/iuppiter/profile/live-overlay`

Those declarations should move to:

- `distro-variants/acorn/ring2/overlays/live`
- `distro-variants/iuppiter/ring2/overlays/live`

Code and tests that must follow:

- `testing/install-tests/src/distro/acorn.rs`
- `testing/install-tests/src/distro/iuppiter.rs`
- any builder/runtime code or docs that still mention `profile/live-overlay`

### 6. Scenario config readers should stop bypassing the contract loader

`testing/install-tests/src/distro/mod.rs` still reads:

- `distro-variants/<distro>/scenarios.toml`

directly.

After this migration it should either:

1. load `scenarios/scenarios.toml` via the canonical path resolver
2. or better, stop reparsing raw TOML and use `distro-contract`

The second option is the preferred end state.

## Source Audit Findings

This section records the current code reality so the migration can be done in
the safest order instead of as a filesystem rename blast radius.

### A. `distro-contract` is already the natural path owner

The strongest signal from the source audit is that `distro-contract` should own
this migration first.

Why:

- `distro-contract/src/variant.rs` already performs:
  - repo-root discovery
  - variant directory discovery
  - ring-manifest loading
  - support-file validation
  - `LoadedVariantContract` construction
- `LoadedVariantContract` is already the cross-crate object used by:
  - `distro-builder`
  - `testing/install-tests`

Historical flat-layout assumptions were concentrated there:

- `load_ring_manifest_bundle`
- top-level `validate_layout` requirements for:
  - `kernel/kconfig`
  - `recipes/kernel.rhai`
  - `evidence/build-capability.sh`
- test scaffolds that write ring manifests directly under variant root

That was why the resolver-first migration was the correct opening move.

### B. `distro-builder` has a few high-value open-coded path assumptions

The builder is not the best place to define canonical layout, but it has a few
important consumer assumptions that must be migrated next.

#### 1. Variant discovery is still flat-layout probing

`distro-builder/src/bin/workflows/parse.rs` discovers distros by checking for
`identity.toml` directly under variant root.

That makes builder discovery a flat-layout owner even though the contract loader
already exists.

#### 2. Release hook lookup is still root-level and product-local

`distro-builder/src/bin/distro-builder.rs` and
`distro-builder/src/bin/workflows/parse.rs` still treat release hook identity as
root-level filenames stored on `BuildProduct`.

`distro-builder/src/bin/workflows/release_hook.rs` then resolves:

- `bundle.variant_dir.join(release_hook_script)`

That is the exact place where ring0 hook relocation will break unless hook
resolution is lifted into a canonical path helper first.

#### 3. Build-host asset resolution has been lifted out of raw `variant_dir` joins

The high-risk consumer path here has already been migrated:

- build-host support files now live under `build-host/`
- `distro-builder` resolves declared build-host paths through the canonical
  contract loader rather than assuming root-level `kconfig` or
  `build-capability.sh`

#### 4. Some direct ring-file reads are now test-only and should stay secondary

`distro-builder/src/pipeline/source.rs` still reads
`variant_dir.join("ring3/sources.toml")`, but that path is gated behind
`#[cfg(test)]`.

This matters, but it is not on the highest-risk critical path compared to:

- loader path ownership
- kernel support files
- release hook lookup

### C. `testing/install-tests` has two different kinds of migration debt

The audit found two categories here.

#### 1. Runtime/preflight code mostly already goes through the contract

`testing/install-tests/src/preflight.rs` already consumes
`LoadedVariantContract`, so once the canonical resolver exists its main path
dependencies should improve automatically.

#### 2. A few helpers still bypass the contract or hardcode current layout

`testing/install-tests/src/distro/mod.rs` still reparses:

- `distro-variants/<distro>/scenarios.toml`

directly for install experience.

This should be deleted in favor of the already loaded contract.

Also, the variant-specific distro files still hardcode compile-time overlay
paths:

- `testing/install-tests/src/distro/acorn.rs`
- `testing/install-tests/src/distro/iuppiter.rs`

via `include_str!(...)` pointing at:

- `profile/live-overlay/...`

Those are isolated and should be migrated late, after the resolver and general
consumer migration are already stable.

### D. `xtask` is a separate migration slice because it does not yet depend on `distro-contract`

`xtask/src/tasks/kernels/common.rs` currently open-codes:

- `root.join("distro-variants").join(distro_id).join("kconfig")`

But `xtask/Cargo.toml` does not currently depend on `distro-contract`.

That means `xtask` should not be the first consumer migrated.

The lowest-risk order is:

1. stabilize canonical path resolution in `distro-contract`
2. migrate `distro-builder` and `testing/install-tests`
3. then add the small `xtask` dependency/configuration change needed to consume
   the same resolver

### E. The highest-risk constraint is the build-host path declaration lock

The current source has a hard lock on old build-host paths:

- `distro-contract/src/build_host_legacy.rs`

with constants:

- `REQUIRED_VARIANT_KCONFIG = "kconfig"`
- `REQUIRED_VARIANT_RECIPE_DECL = "recipes/kernel.rhai"`

And `distro-contract/src/variant.rs` validates against those exact root-level
locations today.

This means:

- moving build-host support files early would create avoidable validator/runtime
  churn
- the migration must loosen or dual-home this validation before any actual file
  move for build-host assets

## Best Migration Path

The safest migration order is:

1. move path ownership into one resolver first
2. migrate all active consumers to that resolver
3. only then move files on disk
4. remove compatibility last

This is the opposite of a naive "move folders, then fix breakage" approach.

That naive approach would create simultaneous fallout in:

- `distro-contract`
- `distro-builder`
- `testing/install-tests`
- `xtask`
- variant shell hooks
- compile-time `include_str!` tests

The resolver-first path keeps the problem bounded.

## Recommended PR Stack

This is the recommended implementation order based on the source audit.

### PR 1. Introduce Canonical Variant Path Resolution In `distro-contract`

Goal:

- create a single resolver API for:
  - owner manifest paths
  - build-host support files
  - ring0 hook paths
  - optional ring2 overlay roots

Recommended shape:

- add a public path struct such as:
  - `VariantOwnerPaths`
- attach it to:
  - `LoadedVariantContract`
- include layout mode information such as:
  - `FlatRoot`
  - `OwnerDirectories`

Required behavior:

- accept either old flat layout or new owner-directory layout
- reject duplicate old+new ownership for the same file

Do not move any files in this PR.

Why first:

- this is the smallest change that removes the biggest architectural risk

### PR 2. Migrate `distro-builder` To Resolver-Owned Paths With No File Moves

Goal:

- eliminate open-coded variant layout assumptions from the active builder path

Required work:

- change distro discovery in `src/bin/workflows/parse.rs`
- change release hook lookup in `src/bin/workflows/release_hook.rs`
- change build-host support-file resolution in `src/pipeline/kernel.rs`
- remove any remaining diagnostics that hardcode old flat owner file locations

Recommended cleanup:

- stop storing root-level hook filenames on `BuildProduct`
- prefer either:
  - a logical hook kind
  - or a canonical resolver-owned relative hook path

Do not move files in this PR either.

Why second:

- once builder consumers use the resolver, actual file movement becomes a data
  migration instead of a code + data migration

### PR 3. Migrate `testing/install-tests` Off Raw Flat Paths

Goal:

- remove direct flat-layout parsing from the test harness

Required work:

- delete direct `scenarios.toml` reparsing in `src/distro/mod.rs`
- load install experience through `distro-contract`
- let preflight/runtime continue consuming `LoadedVariantContract`

Do not touch `include_str!` overlay instrumentation yet.

Why separate:

- this PR is low-risk, easy to test, and keeps the late compile-time overlay
  paths isolated

### PR 4. Add `distro-contract` Dependency To `xtask` And Migrate Kernel Path Resolution

Goal:

- remove open-coded variant `kconfig` path assembly from `xtask`

Required work:

- add `distro-contract` as a direct dependency in `xtask/Cargo.toml`
- replace `distro-variants/<distro>/kconfig` path joining in
  `xtask/src/tasks/kernels/common.rs`
- consume the same canonical resolver as the rest of the repo

Why here:

- `xtask` is not on the critical path for loader correctness
- migrating it after the resolver API stabilizes avoids churn

### PR 5. Move Owner Manifest Files Into Owner Directories

Goal:

- relocate:
  - `identity.toml`
  - `build-host.toml`
  - `ring3/sources.toml`
  - `ring2/products.toml`
  - `ring1/transforms.toml`
  - `ring0/release.toml`
  - `scenarios.toml`

for all active variants.

Required work:

- move files on disk
- update fixture/test scaffolds that write flat manifest files
- keep compatibility window active

Why now:

- by this point all major consumers should already be layout-agnostic

### PR 6. Move Build-Host Support Assets And Update Declared Paths

Goal:

- completed
- build-host support assets now live under:
  - `build-host/kernel/kconfig`
  - `build-host/recipes/kernel.rhai`
  - `build-host/evidence/build-capability.sh`

Required work:

- update build-host declarations as needed
- update `distro-contract/src/build_host_legacy.rs`
- update validator/runtime expectations
- update temp/test fixtures that still write the old root-level files

Why after manifest moves:

- this is the highest-risk path slice because validators, runtime checks, and
  kernel tooling all touch it

### PR 7. Move Ring 0 Hooks And Shared Release Helpers

Goal:

- relocate per-variant hooks into `ring0/hooks/`
- relocate shared helpers into `_shared/ring0/hooks/`

Required work:

- move variant files:
  - `build-release.sh`
  - `boot-release.sh`
  - `live-tools-release.sh`
- move shared files:
  - `_shared/build-release.sh`
  - `_shared/release-artifacts.sh`
- update shell-script relative repo-root calculations because the scripts will
  now live deeper in the tree

Why after PR 2:

- builder-side hook lookup will already be resolver-owned
- only the shell entrypoints and file locations change here

### PR 8. Move Ring 2 Overlay Assets

Goal:

- relocate variant-local overlay material from `profile/live-overlay` into
  `ring2/overlays/live`

Required work:

- move `acorn` and `iuppiter` overlay directories
- update ring2 manifest values
- update compile-time `include_str!` paths in:
  - `testing/install-tests/src/distro/acorn.rs`
  - `testing/install-tests/src/distro/iuppiter.rs`
- update any user-facing diagnostics that still mention `profile/live-overlay`

Why late:

- this is not loader-critical
- it has compile-time path fallout
- it is isolated to two variants

### PR 9. Remove Compatibility And Enforce Owner-Directory Layout Only

Goal:

- delete flat-layout fallback behavior

Required work:

- remove dual-path loader support
- fail on root-level owner files
- delete stale tests/docs/examples that still teach flat layout

Why last:

- compatibility exists only to reduce migration blast radius
- it should be removed as soon as all variants and consumers are converted

## Concrete Phase-to-PR Mapping

The existing six migration phases should map to the implementation stack like
this:

- Phase 1:
  - PR 1
- Phase 2:
  - PR 5
- Phase 3:
  - PR 6
- Phase 4:
  - PR 7
- Phase 5:
  - PR 8
- Phase 6:
  - PR 9

Cross-cutting consumer migration that should happen before any real file move:

- PR 2
- PR 3
- PR 4

Those three PRs are the main result of the source investigation.
They are the difference between a low-risk layout migration and a repo-wide
break/fix scramble.

## Compatibility Window

This track should use a short dual-path compatibility window.

Required behavior during migration:

- the loader accepts either:
  - old flat layout
  - or new owner-directory layout
- a variant may not publish both paths for the same owner file at once
- mixed duplicate ownership is a hard failure

This compatibility window exists only to permit the migration commits to land in
small slices.

It must not become permanent.

## Migration Phases

### Phase 1. Canonical Path Resolver

Goal:

- add one resolver API for variant-local owner/ring paths
- teach loaders to support both old and new locations temporarily

Scope:

- `distro-contract/src/variant.rs`
- any new shared resolver module if split out
- tests for old layout, new layout, and duplicate-path conflict failures

Acceptance:

- canonical resolver exists
- loader can read either layout
- duplicate old+new owner files fail loudly

### Phase 2. Manifest Family Relocation

Goal:

- move all owner/ring manifests into owner directories

Scope:

- all active variants:
  - `levitate`
  - `acorn`
  - `ralph`
  - `iuppiter`
- manifest discovery code
- manifest-focused tests

Acceptance:

- all variants load from owner directories
- variant root no longer contains free-floating owner manifests

### Phase 3. Build-Host Asset Relocation

Goal:

- move build-host support files under `build-host/`

Scope:

- `build-host/kernel/kconfig`
- `build-host/recipes/kernel.rhai`
- `build-host/evidence/build-capability.sh`
- validators/runtime checks/kernel build tooling

Acceptance:

- no code assumes root-level `kconfig`
- build-host support-file validation passes from new paths

### Phase 4. Ring 0 Hook Relocation

Goal:

- move release hooks under `ring0/hooks/`
- move shared release helpers under `_shared/ring0/hooks/`

Scope:

- per-variant release hooks
- shared release helpers
- release hook runner
- shell-script references

Acceptance:

- no variant release hook is root-level
- no shared release helper is flat under `_shared/`

### Phase 5. Ring 2 Overlay Relocation

Goal:

- move variant-local overlay material under `ring2/overlays/`

Scope:

- `acorn`
- `iuppiter`
- overlay declarations
- install-test hardcoded paths

Acceptance:

- `profile/live-overlay` is gone from canonical variant layout
- ring2 owns those overlay assets clearly

### Phase 6. Compatibility Removal

Goal:

- delete flat-layout compatibility

Scope:

- old-path fallbacks
- tests for root-level owner files
- stale docs/examples

Acceptance:

- only owner-directory layout is accepted
- path resolution is centralized
- no open-coded flat path joins remain in active codepaths

## Acceptance Criteria

This track is complete only when all of the following are true:

1. every active distro variant has the canonical owner-directory layout
2. variant root contains only `README.md` plus owner/ring directories
3. no active codepath relies on root-level owner manifests
4. no active codepath relies on root-level `kconfig`
5. no active codepath relies on root-level release hooks
6. no active codepath relies on root-level `profile/live-overlay`
7. shared scenario scripts remain globally owned under `testing/install-tests/test-scripts/`
8. variant path resolution is centralized in one canonical helper surface

## Recommended Order Relative To Other Tracks

Track 06 should start only after:

- Track 04 ownership is materially real on the canonical path
- Track 05 has stabilized the planner/control-plane entrypoints enough that
  path churn will not be conflated with orchestration churn

This is intentionally a later track.

It improves clarity and long-term maintainability, but it should not be allowed
to obscure ownership or orchestration bugs by mixing them into the same change
set.
