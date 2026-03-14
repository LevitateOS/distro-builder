# 04 Ring-Model Ownership Migration

Status: in_progress

## Purpose

Replace the remaining mixed-owner manifest and contract model with a ring-native ownership model.

This track starts where Track 03 stops:

- Track 03 made execution semantics product/scenario-native.
- Track 04 makes manifest ownership and file layout ring-native.

This is not a naming-only migration.
It is an ownership redistribution migration.

## Core Mental Model

The canonical artifact DAG is:

- `Ring 0`
  - final release outputs
- `Ring 1`
  - shippable artifacts directly consumed by Ring 0
- `Ring 2`
  - canonical composed products and payload trees
- `Ring 3`
  - acquired/generated source inputs and provenance roots

Orthogonal owners:

- `identity`
  - distro identity and branding facts
- `build_host`
  - host-tool and kernel-build policy
- `scenarios`
  - live boot, install, installed boot, automated login, runtime

The final model is therefore:

- `identity`
- `build_host`
- `ring3_sources`
- `ring2_products`
- `ring1_transforms`
- `ring0_release`
- `scenarios`

Nothing is allowed to remain in a mixed “old stage file” just because the file was renamed.

## Why This Track Exists

The repo still has a real ownership bug:

- old manifest families such as `00Build.toml`, `01Boot.toml`, and `02LiveTools.toml` are mixed-owner files
- the contents are still partly grouped by historical stage origin rather than by true owner
- Phase 9 in Track 03 exposed that a naming purge alone would be a lie

Examples of the current mixed-owner problem:

- `00Build.toml` mixes:
  - identity
  - build-host capability
  - transform outputs
  - release naming
  - evidence
- `01Boot.toml` mixes:
  - Ring 3 source ownership
  - scenario runtime requirements
- `02LiveTools.toml` mixes:
  - Ring 2 product ownership
  - scenario/install UX behavior

So the remaining work is not “remove stage words”.
It is “move each fact to the correct owner”.

## Ring Ownership Rules

- `identity`
  - owns `os_name`, `os_id`, `iso_label`, versioning, hostname defaults, branding facts
- `build_host`
  - owns host executables, kernel build policy, evidence hooks, host-side prerequisite declarations
- `ring3_sources`
  - owns upstream source selection, source acquisition policy, rootfs source policy, source recipe selection
- `ring2_products`
  - owns canonical trees and payload composition
  - examples:
    - base rootfs tree
    - live overlay tree
    - live boot payload tree
    - installed boot payload tree
    - live tools payload tree
- `ring1_transforms`
  - owns transforms from canonical trees/payloads into shippable artifacts
  - examples:
    - `filesystem.erofs`
    - `overlayfs.erofs`
    - initramfs images
    - UKIs
- `ring0_release`
  - owns final release assembly and publishable outputs
  - examples:
    - ISO
    - disk image
    - checksums
    - release manifests
- `scenarios`
  - owns runtime/test/install behavior and scenario-specific requirements

## Design Rules

- Every field must have exactly one canonical owner.
- No manifest may mix facts from multiple owner families unless the file is explicitly a generated compatibility view.
- File renames are allowed only together with ownership correction.
- The default path must stay single-owner and single-intent.
- Compatibility views, if temporarily needed, must be generated/derived, not canonical.
- Track 04 is complete only when stage-era manifest families are gone from canonical ownership.

## Even Phase Split

This track is intentionally split into 9 even phases:

- Phase 1 establishes the split scaffold.
- Phases 2 through 8 are balanced migration slices, not strict one-owner-per-phase buckets.
- Large rings are split across multiple phases.
- Small owner families are combined where needed to keep the work even.
- Phase 9 removes the old stage-era residue and naming.

That keeps the work balanced and prevents another giant mixed migration.

### Phase 1. Ownership Census And Split Scaffold

Goal:
- inventory every field currently living in stage-era manifests and contracts
- assign each field to exactly one future owner family
- create the new manifest skeletons and loader scaffolding

Scope:
- field-by-field ownership table
- new manifest family layout
- `variant.rs` scaffold that can load the new owners in parallel with legacy manifests

Acceptance:
- every currently loaded field is mapped to one owner family
- no field remains “temporary owner unknown”
- new manifest skeletons exist for all owner families
- no behavior changes are required yet

#### Phase 1 Baseline Census: Shared Fields

| Current source | Current field | Canonical owner | Pilot target |
|---|---|---|---|
| `00Build.toml` | `identity.*` | `identity` | `identity.toml` |
| `00Build.toml` | `stage_00.required_build_tools` | `build_host` | `build-host.toml` |
| `00Build.toml` | `stage_00.kernel_*` | `build_host` | `build-host.toml` |
| `00Build.toml` | `stage_00.evidence.*` | `build_host` | `build-host.toml` |
| `00Build.toml` | `artifacts.rootfs_name` | `ring1_transforms` | `ring1-transforms.toml` |
| `00Build.toml` | `artifacts.initramfs_live_output` | `ring1_transforms` | `ring1-transforms.toml` |
| `00Build.toml` | `artifacts.initramfs_installed_output` | `ring1_transforms` | `ring1-transforms.toml` |
| `00Build.toml` | `artifacts.installed_uki_outputs` | `ring1_transforms` | `ring1-transforms.toml` |
| `00Build.toml` | `artifacts.iso_filename` | `ring0_release` | `ring0-release.toml` |
| `00Build.toml` | `stage_00.iso_assembly.*` | `ring1_transforms` | `ring1-transforms.toml` |
| `00Build.toml` | `stage_00.non_kernel_inputs.*` | derived compatibility view | not stored canonically |
| `00Build.toml` | `stage_01.required_kernel_cmdline` | `scenarios` | `scenarios.toml` |
| `00Build.toml` | `stage_01.required_live_services` | `scenarios` | `scenarios.toml` |
| `01Boot.toml` | `stage_01.boot_inputs.os_name` | compatibility duplication of `identity` | delete after migration |
| `01Boot.toml` | `stage_01.boot_inputs.overlay_kind` | `ring2_products` | `ring2-products.toml` |
| `01Boot.toml` | `stage_01.boot_inputs.required_services` | `scenarios` | `scenarios.toml` |
| `01Boot.toml` | `stage_01.boot_inputs.rootfs_source.*` | `ring3_sources` | `ring3-sources.toml` |
| `02LiveTools.toml` | `stage_02.live_tools.os_name` | compatibility duplication of `identity` | delete after migration |
| `02LiveTools.toml` | `stage_02.live_tools.install_experience` | `scenarios` | `scenarios.toml` |

#### Phase 1 Repo-Wide Variant Deltas

The shared table above covers the common manifest surface.
These variant deltas are the remaining currently loaded fields that differ by distro:

| Variant | Current source | Current field | Canonical owner | Target owner file |
|---|---|---|---|---|
| `ralph` | `00Build.toml` | no `initramfs_installed_output` / no `installed_uki_outputs` | not present for this variant | none |
| `ralph` | `02LiveTools.toml` | `install_experience = "automated_ssh"` | `scenarios` | `scenarios.toml` |
| `acorn` | `00Build.toml` | `artifacts.installed_uki_outputs` | `ring1_transforms` | `ring1-transforms.toml` |
| `acorn` | `01Boot.toml` | `rootfs_source.defines.*` | `ring3_sources` | `ring3-sources.toml` |
| `acorn` | `01Boot.toml` | `openrc_inittab`, `profile_overlay` | `ring2_products` | `ring2-products.toml` |
| `iuppiter` | `00Build.toml` | `artifacts.installed_uki_outputs` | `ring1_transforms` | `ring1-transforms.toml` |
| `iuppiter` | `00Build.toml` | `artifacts.disk_image_output` | `ring0_release` | `ring0-release.toml` |
| `iuppiter` | `01Boot.toml` | `rootfs_source.defines.*` | `ring3_sources` | `ring3-sources.toml` |
| `iuppiter` | `01Boot.toml` | `openrc_inittab`, `profile_overlay` | `ring2_products` | `ring2-products.toml` |
| `acorn`, `iuppiter` | `02LiveTools.toml` | variant-specific `install_experience` | `scenarios` | `scenarios.toml` |

Notes:
- the ring family remains additive during the migration window
- canonical contract loading is no longer `levitate`-only for scaffold/parity detection
- the new ring files now exist for all four variants so owner-scoped layout is proved repo-wide before the later destructive cleanup phases

Current reality:
- all four variants now have a complete ring-manifest scaffold
- `variant.rs` parses the full ring family all-or-none and rejects partial scaffold sets
- `distro-contract` has a workspace test proving the ring scaffold set is complete and parseable for every variant

Honest completion estimate:
- repo-wide: `100%`
- `levitate` pilot only: `100%`

Remaining work before this phase is truly done:
- [x] extend the ownership census beyond `levitate` to `ralph`, `acorn`, and `iuppiter`
- [x] add complete ring-manifest scaffold files for the remaining variants
- [x] prove the field census covers every currently loaded manifest field repo-wide, not just the `levitate` slice
- [ ] keep the census current if new manifest fields are introduced later in Track 04

### Phase 2. Identity And Build-Host Ownership Migration

Goal:
- move the small non-ring owners into canonical `identity` and `build_host` ownership

Scope:
- distro identity fields
- branding/version/default host naming
- required host executables
- kernel build policy boundaries
- evidence script ownership
- host-side prerequisite validation

Acceptance:
- identity facts load from one canonical owner only
- no identity fact remains in ring or scenario manifests
- no build-host fact remains in ring or scenario manifests
- host/build policy is loadable independently of artifact rings

Current reality:
- when a complete ring family is present, `identity` and `build_host` now load canonically from `identity.toml` and `build-host.toml` for all four variants
- `00Build.toml` copies of those owners are still validated in parallel and must stay byte-for-byte equivalent at the canonical contract level
- parity drift is a hard failure during the migration window

Honest completion estimate:
- repo-wide: `80%`
- `levitate` pilot only: `85%`

Remaining work before this phase is truly done:
- [x] add `identity.toml` and `build-host.toml` for `ralph`, `acorn`, and `iuppiter`
- [ ] move any remaining identity/build-host consumers outside `distro-contract` onto the new owner files where appropriate
- [ ] stop treating `00Build.toml` as the long-term canonical home for these owners once parity coverage exists for all variants
- [ ] reduce `00Build.toml` copies of identity/build-host facts to temporary compatibility only, then delete them in the later cleanup phase

### Phase 3. Ring 3 Source Ownership Migration

Goal:
- move all source acquisition and provenance ownership into `ring3_sources`

Scope:
- rootfs source policy
- source recipe selection
- upstream source acquisition policy
- preseed/source preparation facts

Acceptance:
- no source/provenance fact remains in product, transform, or scenario manifests
- Ring 3 can be loaded and validated independently

Current reality:
- all four variants now provide `ring3-sources.toml`, and `distro-builder` loads `rootfs_source.*` canonically from Ring 3 when present
- legacy `01Boot.toml` source fields are still loaded in parallel and must stay semantically identical during the migration window
- source-owner drift is a hard failure in `distro-builder`

Honest completion estimate:
- repo-wide: `65%`
- `levitate` pilot only: `70%`

Remaining work before this phase is truly done:
- [x] add `ring3-sources.toml` for `ralph`, `acorn`, and `iuppiter`
- [ ] move the rest of the source/provenance surface, not just `rootfs_source.*`, into Ring 3 ownership
- [ ] teach `distro-contract` to consume Ring 3 facts canonically instead of only validating the files in parallel
- [ ] remove `01Boot.toml` as the canonical source owner once all Ring 3 facts are migrated

### Phase 4. Ring 2 Base Product Ownership Migration

Goal:
- move the base/foundation product layer into `ring2_products`

Scope:
- base rootfs
- live overlay
- product composition declarations

Acceptance:
- Ring 2 owns the base/foundation trees exclusively
- no transform or release facts remain in the migrated Ring 2 base manifests

Current reality:
- all four variants now provide `ring2-products.toml`
- `distro-contract` now loads the canonical `ProductContract` from `ring2-products.toml` when the ring family is present
- `distro-builder` now loads the base live-overlay policy from `ring2-products.toml`
- legacy `00Build.toml` and `01Boot.toml` copies of those base-product facts are still loaded in parallel and must stay semantically identical during the migration window

Honest completion estimate:
- repo-wide: `75%`
- `levitate` pilot only: `80%`

Remaining work before this phase is truly done:
- [x] add `ring2-products.toml` for `ralph`, `acorn`, and `iuppiter`
- [x] move the remaining base-product facts out of `01Boot.toml`, not just `overlay_kind`
- [ ] move builder/runtime consumers of base-product composition onto Ring 2 ownership instead of stage-era manifests
- [ ] remove `00Build.toml` and `01Boot.toml` as canonical sources of base-product facts once parity coverage exists for all variants

Phase 5 gate decision:
- Phase 5 must start only after repo-wide Ring 2 base parity exists.
- A second `levitate`-only pilot slice is explicitly rejected for this track.

Pre-Phase-5 gate status:
- [x] add `identity.toml`, `build-host.toml`, `ring3-sources.toml`, and `ring2-products.toml` for `ralph`, `acorn`, and `iuppiter`
- [x] make the new owner files parse cleanly for all four variants under `distro-contract`
- [x] extend the Phase 1 ownership census beyond `levitate` so every currently loaded field is mapped repo-wide
- [x] move the remaining Ring 2 base-product facts out of `01Boot.toml`
  - [x] `os_name` duplication
  - [x] `issue_message`
  - [x] `openrc_inittab`
  - [x] `profile_overlay`
- [x] make `distro-builder` base-product loading succeed without needing `01Boot.toml` for `levitate`
- [x] add parity tests for the non-`levitate` variants so their new owner files cannot silently drift from the legacy manifests
- [x] decide whether Phase 5 will start only after repo-wide Ring 2 base parity exists, or whether it will proceed as a second `levitate`-only pilot slice
- [x] because the decision is repo-wide, keep Phase 5 blocked until the repo-wide parity work above is finished

Result:
- the pre-Phase-5 gate is satisfied for the Phase 1-4 scope
- Phase 5 can now start without another `levitate`-only exception

### Phase 5. Ring 2 Runtime Product Ownership Migration

Goal:
- move the runtime-facing product layer into `ring2_products`

Scope:
- live tools payload
- live boot payload trees
- installed boot payload trees
- runtime payload composition declarations

Acceptance:
- Ring 2 owns runtime/live/install payload trees exclusively
- runtime-facing product ownership is separate from Ring 1 transforms and Ring 0 release outputs

### Phase 6. Ring 1 Filesystem Transform Ownership Migration

Goal:
- move the filesystem-image transform layer into `ring1_transforms`

Scope:
- EROFS output declarations
- rootfs/overlay transform input-output graph

Acceptance:
- filesystem image transforms are owned only by Ring 1
- no final release outputs are declared in this slice

### Phase 7. Ring 1 Boot Transform Ownership Migration

Goal:
- move the boot artifact transform layer into `ring1_transforms`

Scope:
- initramfs outputs
- UKI outputs

Acceptance:
- boot transforms are owned only by Ring 1
- Ring 1 is now complete across filesystem and boot artifacts

### Phase 8. Ring 0 Release And Scenario Ownership Migration

Goal:
- move the final output and runtime-behavior owners into their canonical homes
- combine them intentionally because each is smaller than the Ring 2 / Ring 1 slices

Scope:
- ISO outputs
- disk image outputs
- checksum/release metadata outputs
- publishable release bundle declarations
- live boot requirements
- install/runtime scenario requirements
- scenario-specific validation requirements

Acceptance:
- Ring 0 exclusively owns final release outputs
- ISO/disk/checksum naming no longer lives in lower rings
- no scenario behavior remains in ring manifests
- scenario ownership is independent of artifact-ring ownership

### Phase 9. Delete Stage-Era Manifest Families And Purge Stage Numbering

Goal:
- remove the old stage-era manifest/file families after all ownership has been re-homed

Scope:
- delete or retire canonical use of:
  - `00Build.toml`
  - `01Boot.toml`
  - `02LiveTools.toml`
  - `00Build-build.sh`
  - `01Boot-build.sh`
  - `02LiveTools-build.sh`
  - `00Build-build-capability.sh`
- remove literal `stage`
- remove numbered stage artifact families like:
  - `00Build`
  - `01Boot`
  - `02LiveTools`
  - `03Install`
  - `s00`
  - `s01`
  - `s02`
  - numeric aliases like `0`, `1`, `2`

Acceptance:
- no canonical manifest family is stage-era
- no tracked active path contains `stage`
- no tracked active path contains `00Build`, `01Boot`, `02LiveTools`, `s00`, `s01`, or `s02`
- no canonical command/help/doc surface uses stage-era naming

## Proposed Manifest Family

The target manifest family should be role/ring-based, not stage-based.

Minimum canonical set:

- `identity.toml`
- `build-host.toml`
- `ring3-sources.toml`
- `ring2-products.toml`
- `ring1-transforms.toml`
- `ring0-release.toml`
- `scenarios.toml`

If some variants need more granularity, split within a ring, but do not reintroduce stage-era grouping.

## Recommended First Implementation Cut

Start with Phase 1 only.

That means:

- add the owner-family skeletons
- write the field ownership table
- make `variant.rs` capable of loading the new owner-family manifests alongside the old inputs
- do not rename files or commands yet

This is the correct first cut because it fixes the ownership map before any destructive rename work starts.

## Definition Of Done

This track is complete only when:

- every currently tracked fact belongs to exactly one owner family
- ring manifests own only ring facts
- scenario manifests own only scenario facts
- identity/build-host are separate from ring ownership
- stage-era manifest families are gone from canonical ownership
- literal `stage` and numbered stage artifact families are gone from the active repo surface
