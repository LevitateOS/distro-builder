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
   Scope: active filesystem-first migration away from stage-numbered composition toward explicit products, artifact transforms, and release-engineering outputs.

## Recommended Order

1. Keep the current A/B runtime/update model and improve its contract/install/test ownership as part of the product-model transition.
2. Start the filesystem-first migration from `distro-contract`, then move builder/test routing after product ownership is real.

## Why This Split Exists

- The Fedora swap was the concrete near-term migration that unblocked the later tracks.
- The `bootc` track was evaluated and cancelled in favor of keeping the current A/B model.
- The filesystem-first/product-model migration is now the primary architecture track and should start at contract ownership instead of surface-level CLI renames.
