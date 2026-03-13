# 01 Fedora Swap Migration Plan

Status: active

## Goal

Replace the current Rocky Linux Stage 01 DVD/rootfs source path with Fedora Server DVD sourcing for the Levitate/Ralph family without adding another long-lived distro-specific branch to the default builder path.

## What This Migration Covers

- Stage 01 source metadata for Levitate, and possibly Ralph if Ralph becomes explicit.
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
- `distro-builder/src/recipe/rocky_stage01.rs`
- `distro-builder/src/bin/workflows/artifacts.rs`
- `distro-builder/src/bin/workflows/commands.rs`
- `distro-builder/src/bin/workflows/mod.rs`
- `distro-builder/src/bin/distro-builder.rs`
- `distro-builder/recipes/rocky-stage01-rootfs.rhai`
- `distro-builder/recipes/rocky-preseed-iso.rhai`

## Current Repo Reality

- Levitate Stage 01 explicitly uses `kind = "recipe_rocky"` in `distro-variants/levitate/01Boot.toml`.
- Ralph does not currently declare its own `rootfs_source`; it only defines Stage 01 overlay basics in `distro-variants/ralph/01Boot.toml`.
- The current Stage 01 source parser in `distro-builder/src/pipeline/source.rs` only supports `recipe_rocky` and `recipe_custom`.
- The current Stage 01 source adapter in `distro-builder/src/recipe/rocky_stage01.rs` bakes Rocky naming into both the Rust API and the recipe define names.
- The current artifact CLI exposes Rocky-specific preseed commands and error strings.
- There are repo-side Rocky references outside the immediate source-policy path, but they are not all part of this migration. Example: `distro-builder/recipes/qemu-deps.rhai`.

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

- [ ] Decide the new source kind name.
  Recommended: `recipe_rpm_dvd`.
- [ ] Decide whether Ralph becomes explicit in `distro-variants/ralph/01Boot.toml`.
  Recommended: yes, make Ralph explicit once the Fedora path is ready so the source policy is not hidden.
- [ ] Decide whether the Fedora path should keep the current preseed concept or rename it to neutral DVD/source preparation language.
  Recommended: keep the behavior, neutralize the naming.

## Phase 0: Prepare the Abstraction Boundary

- [ ] Replace `S01RootfsSourcePolicy::RecipeRocky` in `distro-builder/src/pipeline/source.rs` with a generic RPM/DVD source variant.
- [ ] Rename or replace `RockyStage01RecipeSpec` in `distro-builder/src/recipe/rocky_stage01.rs` with a neutral Stage 01 DVD/rootfs source spec.
- [ ] Rename or replace `RockyIsoPreseedSpec` in `distro-builder/src/recipe/rocky_stage01.rs` with a neutral DVD preseed/source-preparation spec.
- [ ] Replace `materialize_rootfs(...)` with a neutral materializer API name.
- [ ] Replace `preseed_rocky_iso(...)` with a neutral preseed/source-preparation API name.
- [ ] Replace Rocky-specific define names such as `ROCKY_ISO_NAME`, `ROCKY_SHA256`, `ROCKY_SHA256_URL`, `ROCKY_TORRENT_URL`, `ROCKY_PRESEED_ISO`, and `ROCKY_TRUST_DIR`.
  Recommended: use neutral `STAGE01_*` or `RPM_DVD_*` names.

Exit criteria:

- no default builder path requires the word `rocky` in the Rust type or enum names for the RPM/DVD Stage 01 source path
- the source-policy parser can represent Fedora metadata without pretending it is Rocky

## Phase 1: Replace Rocky-Specific Recipe Inputs

- [x] Create a Fedora-backed rootfs source recipe to replace `distro-builder/recipes/rocky-stage01-rootfs.rhai`.
  Current owner: `distro-builder/recipes/fedora-stage01-rootfs.rhai`
- [x] Create a Fedora-backed ISO/source-preparation recipe to replace `distro-builder/recipes/rocky-preseed-iso.rhai`.
  Current owner: `distro-builder/recipes/fedora-preseed-iso.rhai`
- [ ] Remove Rocky-only marker naming in the recipe layer such as `.rocky-iso-trust.marker` and `.rocky-iso-verified.marker`.
- [ ] Ensure the new recipe names and outputs are not Rocky-branded on disk.
- [ ] Keep the same reproducibility model: explicit ISO name, checksum, checksum URL, and torrent/download source.

Current known package-name deltas for Fedora Server 43:

- drop `basesystem`
- drop the `brotli` CLI package from the initial Stage 01 extraction list
- keep `libbrotli`, which is present on the Fedora Server media
- search Fedora package payloads under `Packages/` or `os/Packages` instead of Rocky `BaseOS/Packages` and `AppStream/Packages`

Exit criteria:

- the Stage 01 source recipe path no longer depends on Rocky-specific helper defines or trust-marker names
- the source recipe can materialize a Fedora-backed rootfs source reproducibly

## Phase 2: Flip Variant Metadata

- [ ] Update `distro-variants/levitate/01Boot.toml` from Rocky metadata to Fedora Server DVD metadata.
- [ ] Replace `kind = "recipe_rocky"` with the new generic source kind.
- [ ] Replace the Stage 01 recipe script path with the Fedora or generic RPM/DVD recipe path.
- [ ] Replace the ISO filename, SHA256, checksum URL, and torrent/download metadata with Fedora values.
- [ ] Decide whether `distro-variants/ralph/01Boot.toml` should gain its own explicit `rootfs_source` block.

Recommended Ralph decision:

- make Ralph explicit once the Fedora path works for Levitate, so the source policy is inspectable and not inherited implicitly

Exit criteria:

- Levitate no longer points at Rocky metadata
- Ralph's ownership is explicit, either as an intentional explicit source block or an intentional documented inheritance choice

## Phase 3: Remove Rocky-Specific CLI and Diagnostics

- [ ] Replace `preseed-rocky-iso` command names in:
  - `distro-builder/src/bin/workflows/commands.rs`
  - `distro-builder/src/bin/workflows/mod.rs`
  - `distro-builder/src/bin/workflows/artifacts.rs`
  - `distro-builder/src/bin/distro-builder.rs`
- [ ] Replace Rocky-only help text and error messages in the builder CLI.
- [ ] Replace Rocky-only wording in Stage 01 diagnostics emitted during source preparation and preseed execution.
- [ ] Audit any remaining `Rocky`-named user-facing output in the default builder path.

Recommended command target:

- use a neutral command such as `artifact preseed-stage01-dvd` or `artifact prepare-stage01-source`

Exit criteria:

- a user can operate the Fedora-backed path without invoking Rocky-branded commands or reading Rocky-branded errors

## Phase 4: Cleanup and Audit

- [ ] Remove or retire `distro-builder/src/recipe/rocky_stage01.rs` once the neutral owner exists.
- [ ] Remove Rocky-only Stage 01 recipe files from the canonical builder path once they are replaced.
- [ ] Audit repo references to `stage01-rootfs-provider/rocky` and either update or explicitly classify them as out of scope.
- [ ] Audit docs and tests for stale Rocky wording.
- [ ] Decide whether `distro-builder/recipes/qemu-deps.rhai` is intentionally Rocky-based and separate from this migration.

Out-of-scope unless explicitly chosen:

- switching every Rocky RPM URL in the repo
- redesigning Stage 01 itself
- changing update/runtime policy

## Validation Checklist

- [ ] `cargo xtask policy audit-legacy-bindings`
- [ ] Levitate Stage 01 config parses with the new Fedora metadata.
- [ ] Stage 01 source preparation succeeds through the canonical builder command path.
- [ ] Stage 01 source output is non-legacy and policy-clean.
- [ ] The builder no longer requires Rocky-branded CLI commands for the default Fedora path.
- [ ] Any Ralph source-path choice is explicit and documented.
- [ ] Docs and migration index point at the new canonical Fedora document.

## Risks

- Fedora media layout may not match Rocky assumptions embedded in the current Rhai recipes.
- Stage 01 CLI and recipe naming currently encode Rocky as if it were the product, not just the current source.
- Ralph may accidentally continue inheriting behavior in a way that obscures source ownership if it is left implicit.

## Recommended Execution Order

1. Land the neutral source-policy and adapter rename first.
2. Land the Fedora-backed recipes second.
3. Flip Levitate metadata third.
4. Make Ralph explicit fourth if that remains the chosen policy.
5. Remove Rocky-specific CLI naming and old adapter leftovers last.

## Definition of Done

This migration is done when all of the following are true:

- Levitate Stage 01 uses Fedora Server DVD metadata through a neutral source-policy path.
- The canonical builder path no longer requires Rocky-branded Rust types, recipe defines, commands, or diagnostics.
- Ralph's source-path ownership is explicit.
- The default Stage 01 source path is reproducible and policy-clean.
