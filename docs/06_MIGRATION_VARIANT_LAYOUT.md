# 06 Variant-Directory Layout Migration

Status: ready

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

Today each active variant directory is mostly flat:

- root-level owner manifests:
  - `identity.toml`
  - `build-host.toml`
  - `ring3-sources.toml`
  - `ring2-products.toml`
  - `ring1-transforms.toml`
  - `ring0-release.toml`
  - `scenarios.toml`
- root-level build/release support files:
  - `kconfig`
  - `build-capability.sh`
  - `build-release.sh`
  - `boot-release.sh`
  - `live-tools-release.sh`
- root-level support folders:
  - `recipes/`
  - `profile/` for some variants

Examples in the current tree:

- `distro-variants/levitate/*`
- `distro-variants/acorn/profile/live-overlay`
- `distro-variants/iuppiter/profile/live-overlay`
- `distro-variants/_shared/build-release.sh`
- `distro-variants/_shared/release-artifacts.sh`

This is workable, but it leaves variant root as a mixed-owner dumping ground.

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
      ring3-sources.toml
      assets/
        rootfs-source/

    ring2/
      ring2-products.toml
      overlays/
        live/
        installed/

    ring1/
      ring1-transforms.toml

    ring0/
      ring0-release.toml
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

### 3. Preserve existing manifest basenames

To minimize loader churn, this track keeps the current manifest filenames:

- `identity/identity.toml`
- `build-host/build-host.toml`
- `ring3/ring3-sources.toml`
- `ring2/ring2-products.toml`
- `ring1/ring1-transforms.toml`
- `ring0/ring0-release.toml`
- `scenarios/scenarios.toml`

This track is about directory structure first, not file-renaming churn.

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
| `<variant>/ring3-sources.toml` | `<variant>/ring3/ring3-sources.toml` |
| `<variant>/ring2-products.toml` | `<variant>/ring2/ring2-products.toml` |
| `<variant>/ring1-transforms.toml` | `<variant>/ring1/ring1-transforms.toml` |
| `<variant>/ring0-release.toml` | `<variant>/ring0/ring0-release.toml` |
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
- `variant_dir.join("ring3-sources.toml")`
- `variant_dir.join("ring2-products.toml")`
- `variant_dir.join("ring1-transforms.toml")`
- `variant_dir.join("ring0-release.toml")`
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

- `kconfig`
- `recipes/kernel.rhai`
- `build-capability.sh`
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
