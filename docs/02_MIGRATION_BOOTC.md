# 02 bootc Migration

Status: stopped

## Purpose

This document slot is reserved for the future migration from the current install/runtime/update assumptions toward a `bootc`-oriented model.

## Why It Is Stopped

- The Fedora swap has landed, so the Stage 01 source path is no longer Rocky-specific.
- The current repo still routes important install/runtime decisions through stage-numbered tests and legacy mutability compatibility paths.
- Starting `bootc` now would risk mixing runtime-policy work with source-media migration and stage-model cleanup.

## What This Track Will Eventually Cover

- replacing `ImmutableAb`-style install assumptions with a `bootc` runtime/update contract
- moving Levitate/Ralph runtime policy away from legacy `rootfs_mutability` inference
- auditing `bootctl` assumptions in install tests and distro boot specs
- defining the release/product model needed for `bootc` outputs

## Current Canonical Owners

- `distro-contract/src/schema.rs`
- `distro-spec/src/conformance.rs`
- `testing/install-tests/src/stages/mod.rs`
- `testing/install-tests/src/steps/phase5_boot.rs`
- `testing/install-tests/src/distro/levitate.rs`
- `testing/install-tests/src/distro/ralph.rs`
- `distro-spec/src/shared/boot.rs`

## Entry Criteria

- Fedora swap is complete enough that the default source-media path is no longer Rocky-specific.
- The repo is ready to make runtime/update policy explicit instead of inferring it from legacy mutability fields.

## Start Point When Resumed

When this track resumes, the first concrete action should be to replace install-layout inference in `testing/install-tests/src/stages/mod.rs` with an explicit runtime/update contract field.
