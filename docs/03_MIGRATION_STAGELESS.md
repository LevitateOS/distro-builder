# 03 Stageless Migration

Status: stopped

## Purpose

This document slot is reserved for the future migration away from the current stage-numbered build model and toward direct filesystem products plus release-engineering outputs.

## Why It Is Stopped

- This is the largest architecture change of the three migration tracks.
- The current builder, contract, and install-test layers still treat stage numbers as the composition model, not just UI labels.
- Starting it before the Fedora swap would create churn in the same Stage 01 source path twice.

## What This Track Will Eventually Cover

- replacing stage-numbered build targets with product-oriented build targets
- removing parent-stage extraction as the main composition mechanism
- replacing stage-numbered contract structures with product/release-oriented contracts
- moving install tests from stage-owned artifact lookup to scenario-based validation
- retiring stage-numbered CLI wrappers once compatibility shims are no longer needed

## Current Canonical Owners

- `distro-builder/src/bin/workflows/parse.rs`
- `distro-builder/src/bin/workflows/build.rs`
- `distro-builder/src/bin/distro-builder.rs`
- `distro-builder/src/stages/s01_boot_inputs.rs`
- `distro-builder/src/stages/s02_live_tools_inputs.rs`
- `distro-builder/src/pipeline/plan.rs`
- `distro-contract/src/schema.rs`
- `distro-contract/src/variant.rs`
- `distro-contract/src/validate.rs`
- `distro-contract/src/runtime.rs`
- `testing/install-tests/src/stages/mod.rs`
- `testing/install-tests/src/preflight.rs`
- `xtask/src/cli/types.rs`
- `xtask/src/tasks/testing/stages.rs`
- `justfile`

## Entry Criteria

- Fedora swap is complete.
- The repo is ready to replace stage-numbered contracts and test lookups with product-oriented ownership.

## Start Point When Resumed

When this track resumes, the first concrete action should be to define the replacement product model in `distro-contract` before changing the builder CLI.
