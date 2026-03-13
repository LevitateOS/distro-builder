# Migration Index

This directory tracks the current high-level distro-builder migration work as numbered design tracks.

## Tracks

1. [01_MIGRATION_FEDORA_SWAP.md](01_MIGRATION_FEDORA_SWAP.md)
   Status: active
   Scope: replace the current Rocky Stage 01 DVD/rootfs source path with Fedora Server DVD sourcing for the Levitate/Ralph family.

2. [02_MIGRATION_BOOTC.md](02_MIGRATION_BOOTC.md)
   Status: stopped
   Scope: future runtime/update migration to `bootc`.

3. [03_MIGRATION_STAGELESS.md](03_MIGRATION_STAGELESS.md)
   Status: stopped
   Scope: future removal of the current stage-numbered build model in favor of direct filesystem products and release engineering outputs.

## Recommended Order

1. Finish the Fedora swap first.
2. Start `bootc` only after the Fedora source path is no longer Rocky-specific.
3. Start the stageless/product-model migration only when the repo is ready to replace stage-numbered contracts and tests.

## Why This Split Exists

- The Fedora swap is a concrete near-term migration with clear current owners.
- The `bootc` work is real, but should not start from stale Rocky- or A/B-framed assumptions.
- The stageless/product-model migration is the largest architecture change and should not be half-started while Fedora source plumbing is still in flux.
