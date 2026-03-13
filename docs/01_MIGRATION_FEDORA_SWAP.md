# 01 Fedora Swap Migration Plan

Status: active; Levitate Fedora flip landed, compatibility cleanup still pending

## Goal

Replace the current Rocky Linux Stage 01 DVD/rootfs source path with Fedora Server DVD sourcing for the Levitate/Ralph family without adding another long-lived distro-specific branch to the default builder path.

## What This Migration Covers

- Stage 01 source metadata for Levitate and Ralph.
- Builder-side Stage 01 source policy parsing and materialization.
- Recipe-side rootfs source preparation and ISO preseeding for the current RPM/DVD path.
- Rocky-branded CLI and diagnostics that would become wrong once Fedora is the canonical source.

## What This Migration Does Not Cover

- `bootc`
- A/B or runtime update policy changes
- removal of the stage model
- broad package-source changes outside the Stage 01 Fedora/Rocky source path

## Current Canonical Owners

- `distro-variants/levitate/01Boot.toml`
- `distro-variants/ralph/01Boot.toml`
- `distro-builder/src/pipeline/config.rs`
- `distro-builder/src/pipeline/source.rs`
- `distro-builder/src/stages/s01_boot_inputs.rs`
- `distro-builder/src/recipe/stage01_source.rs`
- `distro-builder/src/bin/workflows/artifacts.rs`
- `distro-builder/src/bin/workflows/commands.rs`
- `distro-builder/src/bin/workflows/mod.rs`
- `distro-builder/src/bin/distro-builder.rs`
- `distro-builder/recipes/stage01-dvd-deps.rhai`
- `distro-builder/recipes/fedora-stage01-rootfs.rhai`
- `distro-builder/recipes/fedora-preseed-iso.rhai`
- `distro-builder/recipes/rocky-stage01-rootfs.rhai`
- `distro-builder/recipes/rocky-preseed-iso.rhai`

## Current Repo Reality

- Levitate Stage 01 now uses `kind = "recipe_rpm_dvd"` in `distro-variants/levitate/01Boot.toml`.
- Levitate now points at the Fedora Stage 01 source recipes in `distro-variants/levitate/01Boot.toml`.
- Ralph now declares an explicit Fedora-backed `rootfs_source` in `distro-variants/ralph/01Boot.toml`.
- The current Stage 01 source parser in `distro-builder/src/pipeline/source.rs` supports the neutral `recipe_rpm_dvd` path plus legacy `recipe_rocky` compatibility and `recipe_custom`.
- The Stage 01 recipe path is now self-contained for the canonical case: the Rocky, Fedora, and Alpine Stage 01 recipes no longer require large injected metadata define maps.
- The Stage 01 source adapter now lives in `distro-builder/src/recipe/stage01_source.rs` and exposes neutral rootfs materialization and preseed APIs.
- The default artifact CLI path is now generic (`artifact preseed-stage01-source <distro>`); the Rocky/Alpine command names remain only as compatibility aliases.
- The canonical RPM/DVD Stage 01 dependency owner is now `distro-builder/recipes/stage01-dvd-deps.rhai`.
- There are repo-side Rocky references outside the immediate source-policy path, but they are not all part of this migration. Example: `distro-builder/recipes/qemu-deps.rhai`.

## Progress Snapshot

Already landed:

- Stage 01 recipes are self-contained for the canonical case and no longer depend on Rust to inject ISO/checksum/torrent metadata.
- The default public Stage 01 preseed command is now `artifact preseed-stage01-source <distro> [--refresh]`.
- Stage 01 source materialization work dirs are now recipe-derived rather than hardcoded to Rocky-specific subpaths.
- Kernel source metadata ownership was also corrected in the same cleanup pass, so Rust no longer injects kernel version/SHA/localversion facts into the shared kernel recipe.
- Levitate now points at the Fedora Stage 01 rootfs and preseed recipes by default.
- The default RPM/DVD Stage 01 dependency recipe is now neutralized as `stage01-dvd-deps`.

Still blocking the actual Fedora swap:

- A real preseeded Fedora ISO run has not been executed yet through the canonical builder path.
- The legacy `recipe_rocky` parser branch and Rocky compatibility recipes still exist.
- Ralph source ownership is now explicit.

## Migration Strategy

Use a generic RPM/DVD Stage 01 source boundary as the compatibility bridge, then flip Levitate to Fedora Server DVD metadata.

Do not add `recipe_fedora` as a second permanent distro-specific default kind unless there is a very strong reason to keep separate source-policy branches.

The intended shape is:

- `recipe_rocky` current state
- temporary bridge: generic RPM/DVD source kind and adapter
- canonical near-term use: Fedora Server DVD through that generic path

## Current Chosen Fedora Artifact

- ISO: `Fedora-Server-dvd-x86_64-43-1.6.iso`
- SHA256: `aca06983bef83da9b43144c1a2ff4c8483e4745167c17f53725c16a16742e643`
- CHECKSUM: `https://download.fedoraproject.org/pub/fedora/linux/releases/43/Server/x86_64/iso/Fedora-Server-43-1.6-x86_64-CHECKSUM`
- Torrent: `https://torrent.fedoraproject.org/torrents/Fedora-Server-dvd-x86_64-43.torrent`

This is the current concrete migration target for the first Fedora-backed preseed/source recipe.

## Decision Log To Resolve First

- [x] Decide the new source kind name.
  Recommended: `recipe_rpm_dvd`.
- [x] Decide whether Ralph becomes explicit in `distro-variants/ralph/01Boot.toml`.
  Chosen: yes, keep Ralph explicit so the source policy is inspectable and not inherited implicitly.
- [x] Decide whether the Fedora path should keep the current preseed concept or rename it to neutral DVD/source preparation language.
  Recommended: keep the behavior, neutralize the naming.

## Phase 0: Prepare the Abstraction Boundary

- [x] Replace `S01RootfsSourcePolicy::RecipeRocky` in `distro-builder/src/pipeline/source.rs` with a generic RPM/DVD source variant.
- [x] Rename or replace `RockyStage01RecipeSpec` in `distro-builder/src/recipe/stage01_source.rs` with a neutral Stage 01 DVD/rootfs source spec.
- [x] Rename or replace `RockyIsoPreseedSpec` in `distro-builder/src/recipe/stage01_source.rs` with a neutral DVD preseed/source-preparation spec.
- [x] Replace `materialize_rootfs(...)` with a neutral materializer API name.
- [x] Replace `preseed_rocky_iso(...)` with a neutral preseed/source-preparation API name.
- [x] Remove Rocky-specific metadata define injection from the canonical Rust execution path.
- [x] Replace remaining Rocky-specific helper/env names such as `ROCKY_FORCE_REFRESH` if Rocky compatibility remains.
  Canonical path now uses neutral `STAGE01_SOURCE_FORCE_REFRESH`; legacy Rocky/Fedora envs still work as compatibility inputs.

Exit criteria:

- no default builder path requires the word `rocky` in the Rust type or enum names for the RPM/DVD Stage 01 source path
- the source-policy parser can represent Fedora metadata without pretending it is Rocky

## Phase 1: Replace Rocky-Specific Recipe Inputs

- [x] Create a Fedora-backed rootfs source recipe to replace `distro-builder/recipes/rocky-stage01-rootfs.rhai`.
  Current owner: `distro-builder/recipes/fedora-stage01-rootfs.rhai`
- [x] Create a Fedora-backed ISO/source-preparation recipe to replace `distro-builder/recipes/rocky-preseed-iso.rhai`.
  Current owner: `distro-builder/recipes/fedora-preseed-iso.rhai`
- [x] Remove Rocky-only marker naming from the canonical Fedora recipe path.
  Rocky compatibility recipes still retain their original marker names.
- [x] Ensure the new recipe names and outputs are not Rocky-branded on disk.
- [x] Replace the Rocky-named Stage 01 dependency owner in the canonical path with `distro-builder/recipes/stage01-dvd-deps.rhai`.
- [x] Keep the same reproducibility model: explicit ISO name, checksum, checksum URL, and torrent/download source.

Current known package-name deltas for Fedora Server 43:

- drop `basesystem`
- drop the `brotli` CLI package from the initial Stage 01 extraction list
- keep `libbrotli`, which is present on the Fedora Server media
- search Fedora package payloads under `Packages/` or `os/Packages` instead of Rocky `BaseOS/Packages` and `AppStream/Packages`

Exit criteria:

- the Stage 01 source recipe path no longer depends on Rocky-specific helper defines or trust-marker names
- the source recipe can materialize a Fedora-backed rootfs source reproducibly

## Phase 2: Flip Variant Metadata

- [x] Update `distro-variants/levitate/01Boot.toml` from Rocky ownership to Fedora-backed Stage 01 recipes.
- [x] Replace `kind = "recipe_rocky"` with the new generic source kind.
- [x] Replace the Stage 01 recipe script path with the Fedora-backed RPM/DVD recipe path.
- [x] Move the canonical Fedora ISO filename, SHA256, checksum URL, and torrent/download metadata into the Fedora recipes rather than variant TOML.
- [x] Decide whether `distro-variants/ralph/01Boot.toml` should gain its own explicit `rootfs_source` block.

Recommended Ralph decision:

- make Ralph explicit once the Fedora path works for Levitate, so the source policy is inspectable and not inherited implicitly

Exit criteria:

- Levitate no longer points at Rocky metadata
- Ralph's ownership is explicit, either as an intentional explicit source block or an intentional documented inheritance choice

## Phase 3: Remove Rocky-Specific CLI and Diagnostics

- [x] Replace the default public Stage 01 preseed command with `artifact preseed-stage01-source <distro> [--refresh]`.
- [x] Replace `preseed-rocky-iso` command names in:
  - `distro-builder/src/bin/workflows/commands.rs`
  - `distro-builder/src/bin/workflows/mod.rs`
  - `distro-builder/src/bin/workflows/artifacts.rs`
  - `distro-builder/src/bin/distro-builder.rs`
- [x] Replace Rocky-only help text in the default builder CLI usage surface.
- [x] Replace Rocky-only wording in default Stage 01 preseed diagnostics emitted by the builder path.
- [x] Audit the default builder path so operators are not taught Rocky/Alpine-specific preseed commands.
- [ ] Decide whether the explicit `preseed-rocky-iso` and `preseed-alpine-stage01-assets` aliases should be retained temporarily as compatibility commands or removed entirely in the next cleanup pass.

Recommended command target:

- use a neutral command such as `artifact preseed-stage01-source <distro>`

Exit criteria:

- a user can operate the Stage 01 source path through `artifact preseed-stage01-source <distro>` without invoking Rocky-branded commands

## Phase 4: Cleanup and Audit

- [x] Remove or retire `distro-builder/src/recipe/rocky_stage01.rs` once the neutral owner exists.
- [x] Remove Rocky-only Stage 01 recipe files from the canonical builder path once they are replaced.
- [x] Audit repo references to `stage01-rootfs-provider/rocky` and either update or explicitly classify them as out of scope.
- [ ] Audit docs and tests for stale Rocky wording.
- [ ] Decide whether `distro-builder/recipes/qemu-deps.rhai` is intentionally Rocky-based and separate from this migration.

Out-of-scope unless explicitly chosen:

- switching every Rocky RPM URL in the repo
- redesigning Stage 01 itself
- changing update/runtime policy

## Validation Checklist

- [x] `cargo xtask policy audit-legacy-bindings`
- [x] Levitate Stage 01 config parses with the new Fedora metadata.
- [ ] Stage 01 source preparation succeeds through the canonical builder command path.
- [ ] Stage 01 source output is non-legacy and policy-clean.
- [x] The builder no longer requires Rocky-branded CLI commands for the default Fedora path.
- [x] Any Ralph source-path choice is explicit and documented.
- [x] Docs and migration index point at the new canonical Fedora document.

## Risks

- Fedora media layout may not match Rocky assumptions embedded in the current Rhai recipes.
- Stage 01 CLI and recipe naming currently encode Rocky as if it were the product, not just the current source.
- Ralph may accidentally continue inheriting behavior in a way that obscures source ownership if it is left implicit.

## Recommended Execution Order

1. Run a real Fedora preseed/build validation through the canonical builder path.
2. Decide whether to keep or remove the legacy `recipe_rocky` parser branch and Rocky CLI aliases.
3. Audit remaining docs/tests for stale Rocky wording.

## Definition of Done

This migration is done when all of the following are true:

- Levitate Stage 01 uses Fedora Server DVD metadata through a neutral source-policy path.
- The canonical builder path no longer requires Rocky-branded Rust types, recipe defines, commands, or diagnostics.
- Ralph's source-path ownership is explicit.
- The default Stage 01 source path is reproducible and policy-clean.
