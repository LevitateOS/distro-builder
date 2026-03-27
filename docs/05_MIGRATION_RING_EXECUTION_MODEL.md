# 05 Ring-Execution Orchestration Migration

Status: ready

## Purpose

Make the ring model real in orchestration, not only in ownership.

Track 04 fixes where canonical facts live.
This track fixes how the builder plans and executes work from those facts.

This document exists to address one specific failure mode:

- a repo can rename manifests into `ring3_sources`, `ring2_products`, `ring1_transforms`, and `ring0_release`
- and still behave like a stage-driven system underneath
- which would make the ring model cosmetic instead of architectural

That outcome is explicitly rejected.

## Exact Concern

The concern is not "are stage words still present somewhere?"
The real concern is:

- does the operator target an outer product/output and let the system resolve inward dependencies automatically
- or does the operator still have to manually walk a historical ladder to make the build succeed

If the latter is still true, then the repo is not yet ring-process-native.
It is only ring-owned.

## Core Distinction

There are three different things here:

1. ownership
   - where canonical facts live
2. targeting
   - what the operator asks for
3. execution
   - what the planner builds first, next, and last

Track 04 solves ownership.
This track solves targeting and execution.

## Ring-Execution Rule

The correct ring-native rule is:

1. the operator targets the outermost desired result
   - usually a Ring 0 release output
   - sometimes a Ring 1 artifact or Ring 2 prepared product for debugging
2. the planner resolves the full dependency closure inward from that target
3. the executor materializes missing dependencies from the innermost missing nodes outward
4. stages do not participate in default build planning

Important clarification:

- "outer ring first" is correct for target selection
- it is wrong for execution order
- execution must build inward dependencies first, then materialize outward

Example:

- user requests: `release build iso levitate live-tools`
- planner resolves:
  - Ring 0: release ISO output for `live-tools`
  - Ring 1: rootfs image, overlay image, live initramfs, live UKI outputs
  - Ring 2: `product.payload.live_tools`, `product.payload.boot.live`, `product.payload.live_overlay`, `product.rootfs.base`, `product.kernel.staging`
  - Ring 3: rootfs source acquisition/provenance inputs
- executor then builds missing nodes in the opposite direction:
  - Ring 3 -> Ring 2 -> Ring 1 -> Ring 0

If the operator must manually prebuild `base-rootfs` or `live-boot` before `live-tools`, then the system is still exposing dependency-walking burden that belongs in the planner.

## What Must Be True In The Final Model

### 1. Outer-target entrypoints are canonical

Canonical user-facing build entrypoints must target products or release outputs, not stages.

Allowed examples:

- `distro-builder release build iso <distro> <product>`
- `distro-builder product prepare <product> <distro> <output_dir>`
- explicit lower-ring debug entrypoints where the ring intent is obvious

Forbidden as the default public model:

- requiring `00Build`, `01Boot`, `02LiveTools`, or similar labels to make progress
- requiring users to know historical stage aliases to satisfy dependencies

### 2. Dependency closure is planner-owned

The planner must own dependency traversal.

Forbidden:

- "build parent first yourself" as the normal success path
- hidden stage fallback logic
- manual chaining through compatibility wrappers as the primary orchestration story

### 3. Stages are validation-only in the canonical path

Stages may remain for:

- install/runtime checkpoints
- scenario/test progression
- conformance attribution
- compatibility wrappers during migration

Stages must not remain the canonical owners of:

- composition order
- build prerequisites
- release dependency planning
- artifact identity

### 4. Ring identities must be first-class planner nodes

The planner should reason in terms of canonical logical identities such as:

- Ring 3 source owners
- Ring 2 product logical names
- Ring 1 transform logical names
- Ring 0 release outputs

It should not internally fall back to stage buckets for normal planning.

### 5. Missing dependencies are normal planner work, not operator work

A missing parent product or transform should trigger planner action when possible, not an instruction to the operator to manually walk a legacy ladder.

If a dependency cannot yet be auto-built, the failure should be treated as orchestration debt and called out explicitly.

## Half-Migrations To Reject

The following are considered false-completion patterns:

- ring-named manifests with stage-driven build sequencing
- product-named CLI entrypoints that still require manual parent-product prebuild order
- stage aliases still being the easiest or default operator path
- ring-native ownership in `distro-contract` but stage-native orchestration in `distro-builder`
- documenting rings as architecture while continuing to teach stages as the normal execution model

## Current Repo Reality

This section is intentionally blunt.

### What is already true

- canonical ownership is ring-native
- canonical manifests now live under:
  - `identity.toml`
  - `build-host.toml`
  - `ring3-sources.toml`
  - `ring2-products.toml`
  - `ring1-transforms.toml`
  - `ring0-release.toml`
  - `scenarios.toml`
- canonical release parsing is product-native
- canonical product preparation entrypoints exist

### What is not yet fully true

- release builds still treat some parent-product availability as a precondition instead of recursively materializing the full dependency closure
- compatibility stage wrappers remain visible in the repo-level `justfile`
- stage identifiers still exist in validation/reporting surfaces

That means:

- the repo has largely completed ring-native ownership
- the repo has partially completed ring-native execution
- the repo has not yet fully completed ring-process-native orchestration

## Source Audit

This section records the current source-code reality by owner so Track 05 can
be implemented against the actual codepaths.

### A. Ownership is already ring-native on the canonical path

- `distro-contract/src/variant.rs`
  - canonical owner family is now `identity.toml`, `build-host.toml`,
    `ring3-sources.toml`, `ring2-products.toml`, `ring1-transforms.toml`,
    `ring0-release.toml`, and `scenarios.toml`
- `distro-builder/src/pipeline/source.rs`
  - Ring 3 rootfs-source policy is loaded from the canonical contract
- `distro-builder/src/pipeline/config.rs`
  - Ring 2 overlay, boot-payload, and live-tools runtime policy are loaded from
    the canonical contract
- `distro-builder/src/bin/workflows/parse.rs`
  - canonical CLI parsing accepts product names and rejects stage aliases as
    products
- `distro-builder/src/bin/workflows/release_hook.rs`
  - release hooks consume Ring 1 and Ring 0 identities, not stage manifests
- `distro-variants/_shared/build-release.sh`
  - shared release path prepares product inputs through
    `distro-builder product prepare ...`

This means the ownership migration is not the main blocker anymore.

### B. The biggest remaining blocker is planner ownership of dependency closure

The current canonical build path still leaks parent-product sequencing to the
operator.

- `distro-builder/src/bin/workflows/build.rs`
  - `preflight_iso_build` resolves the immediate parent product and fails with
    "build the parent release first" instead of recursively materializing the
    dependency closure
- `distro-builder/src/pipeline/io.rs`
  - `resolve_parent_product_rootfs_image_for_distro` looks only for the latest
    successful parent release artifact under `.artifacts/out/<distro>/releases/*`
- `distro-builder/src/pipeline/products.rs`
  - derived product preparation calls
    `resolve_parent_product_rootfs_image_for_distro(...)` internally, so product
    preparation is coupled to existing release artifacts on disk instead of
    planner-provided resolved inputs
- `distro-builder/src/bin/workflows/artifacts.rs`
  - `prepare_product_inputs` already knows the canonical Ring 2 `extends` edge,
    but still delegates to product preparers that perform parent lookup as a
    side effect

This is the primary Track 05 execution debt.

### C. Scenario/test execution is product-aware, but intentionally consume-only

The testing path already resolves canonical release products, but it still
expects those products to exist before the scenario runner starts.

- `testing/install-tests/src/scenarios/mod.rs`
  - `resolve_iso_artifact_for_scenario` maps scenarios to canonical release
    products and looks up the latest successful release run
  - missing release products still fail with "build it first"
- `xtask/src/tasks/testing/scenarios.rs`
  - interactive and automated scenario commands consume existing artifacts via
    canonical scenario/product resolution
- `xtask/src/app.rs`
  - policy guard placement is already enforced at executable entrypoints

This is correct with respect to the repo policy boundary:

- build commands may produce artifacts
- boot/test/scenario commands must consume existing artifacts

Track 05 should therefore make release/product build orchestration recursive,
but it should not add hidden build side effects to scenario runners.

### D. The wrapper/doc layer still teaches stage-shaped workflows

- `justfile`
  - compatibility aliases remain prominent: `_boot_stage`, `stage`, `stage-ssh`,
    `test`, `test-up-to`, `test-status`, `test-reset`
  - `build` still special-cases `03Install`
  - `build-up-to` still loops `00Build`, `01Boot`, `02LiveTools`, `03Install`
- `xtask/README.md`
  - usage examples still teach `stages boot`, `stages test`, and
    `stages test-up-to`
- `testing/install-tests/src/bin/install-tests.rs`
  - removal message still tells users to use SSH-based "stage workflows"

This is UX debt, not architecture debt, but it keeps the old process model
visible in day-to-day usage.

### E. Validation/reporting still uses stage attribution as the main vocabulary

- `distro-contract/src/error.rs`
  - violation attribution is still `StageId`
- `distro-contract/src/validate.rs`
  - many canonical validation surfaces still report under `Stage00` / `Stage01`
- `distro-contract/src/runtime.rs`
  - canonical runtime validation exists, but the compatibility entrypoint
    `validate_live_boot_runtime_with_stage_dir` remains
- `distro-builder/src/bin/artifact_paths.rs`
  - `stage_output_dir_for` is still present as a compatibility path helper

This residue is acceptable in the short term if it remains explicitly
validation-only and does not control build orchestration.

### F. Canonical scenario-script installation still carries a migration bug

There is one concrete builder-side inconsistency that should be fixed during
Track 05 proof/hardening:

- `testing/install-tests/test-scripts/` contains canonical scenario scripts such
  as `live-boot.sh`, `live-tools.sh`, `install.sh`, `installed-boot.sh`,
  `automated-login.sh`, and `installed-tools.sh`
- `distro-builder/src/pipeline/scripts.rs` currently installs only files whose
  names start with `stage-`
- the canonical scenario evidence in `distro-variants/*/scenarios.toml` points
  at `live-boot.sh`, `live-tools.sh`, and related script names

That means the canonical builder path does not currently own scenario-script
installation correctly. This should be fixed as part of Track 05, not left to
legacy crate behavior.

## Acceptance Criteria

This track is complete only when all of the following are true:

1. requesting a Ring 0 release target causes the planner to resolve and materialize its transitive missing dependencies automatically
2. requesting an explicit Ring 1 or Ring 2 target causes the planner to resolve only the lower-ring dependencies required for that target
3. no canonical build success path requires the operator to manually sequence historical stage aliases
4. canonical help text and primary docs teach product/release/ring entrypoints first
5. remaining stage references are limited to validation, scenarios, conformance attribution, or explicit compatibility surfaces

## Recommended Upgrade Order

The best upgrade path is:

1. fix planner ownership first
2. then decouple product preparation from on-disk parent release lookup
3. then clean up wrapper/docs surfaces
4. then harden validation/script compatibility residue
5. then prove the model with tests

Do not start by deleting stage words.
That produces a cosmetic migration and leaves the operator-facing dependency
problem untouched.

## Concrete Implementation Plan

### Phase 1. Introduce a real orchestration planner

Goal:

- make dependency closure an explicit subsystem instead of scattered ad hoc
  parent lookups

Recommended file ownership:

- `distro-builder/src/pipeline/mod.rs`
  - add a dedicated planner module
- `distro-builder/src/pipeline/plan.rs`
  - rename or narrow this module because it currently owns producer-plan logic,
    not orchestration planning
  - recommended rename target: `producers.rs`
- new file:
  - `distro-builder/src/pipeline/planner.rs`

Planner responsibilities:

- represent canonical node identities for:
  - Ring 2 product nodes
  - Ring 1 transform nodes
  - Ring 0 release targets
- resolve closure from a requested target using:
  - `contract.products.*.extends`
  - `contract.transforms.*.dependencies`
  - `contract.release.*.dependencies`
- distinguish:
  - cache hit
  - build required
  - impossible/missing owner declaration

Important rule:

- planner nodes must use canonical logical identities from the contract
- do not reintroduce stage buckets as internal planner keys

Acceptance proof for Phase 1:

- planner unit tests in `distro-builder` proving:
  - `live-tools` release target closes over `live-boot` and `base-rootfs`
  - `live-boot` release target closes over `base-rootfs`
  - impossible or missing edges fail with explicit contract-owner diagnostics

### Phase 2. Make product preparation consume resolved inputs, not global repo lookup

Goal:

- derived product preparation must consume explicit resolved parent artifacts
  from the planner, not discover "latest successful parent release" by itself

Recommended file ownership:

- `distro-builder/src/pipeline/products.rs`
- `distro-builder/src/pipeline/io.rs`
- `distro-builder/src/bin/workflows/artifacts.rs`

Required changes:

- move parent-rootfs lookup responsibility out of:
  - `prepare_live_boot_product`
  - `prepare_live_tools_product`
  - `prepare_installed_boot_product`
- replace internal repo-root parent discovery with explicit resolved inputs, for
  example:
  - parent rootfs image path
  - source product identity
  - source run metadata when needed for reproducibility
- downgrade `resolve_parent_product_rootfs_image_for_distro` into a lower-level
  cache/artifact lookup helper or replace it entirely with planner-owned
  resolution
- make `prepare_product_inputs` in `artifacts.rs` ask the planner for the
  required lower-ring inputs before calling product preparers

Why this phase matters:

- until product preparation consumes planner-resolved inputs, the system cannot
  be ring-process-native even if the release entrypoint becomes recursive

Acceptance proof for Phase 2:

- a unit/integration test that prepares `live-tools` inputs from an otherwise
  empty prepared-output dir when only lower-ring sources are available
- no product preparer should emit "build parent product first" anymore

### Phase 3. Replace release preflight failure with recursive target realization

Goal:

- `release build iso <distro> <product>` should realize the transitive closure
  of buildable prerequisites instead of aborting on the first missing parent

Recommended file ownership:

- `distro-builder/src/bin/workflows/build.rs`
- `distro-builder/src/bin/workflows/commands.rs`
- `distro-builder/src/bin/workflows/release_hook.rs`
- `distro-builder/src/bin/distro-builder.rs`

Required changes:

- replace `preflight_iso_build` as the canonical release dependency gate
- add a release-realization path that:
  - resolves target closure through the planner
  - materializes missing lower-ring prerequisites in inner-to-outer order
  - records release runs only for the requested release target, not as a fake
    stage ladder
- keep release hooks product-native
- keep the current policy guard placement

Explicit non-goal for this phase:

- do not add implicit build side effects to scenario boot/test wrappers

Acceptance proof for Phase 3:

- from an empty release-product run root for a distro, this succeeds:
  - `distro-builder release build iso <distro> live-tools`
- and it succeeds without requiring the user to manually build:
  - `base-rootfs`
  - `live-boot`

### Phase 4. Normalize artifact resolution for consume-only test paths

Goal:

- keep test/boot paths consume-only, but make their product resolution clearly
  canonical and shared

Recommended file ownership:

- `testing/install-tests/src/scenarios/mod.rs`
- `xtask/src/tasks/testing/scenarios.rs`
- optional shared helper:
  - `distro-builder/src/bin/workflows/artifact_resolve.rs`
  - or a testing-local resolver if cross-crate reuse is not worth it yet

Required changes:

- keep `resolve_iso_artifact_for_scenario` consume-only
- keep missing artifact failures explicit
- make error messages teach canonical product/release commands, not stage
  walkthroughs
- if a shared artifact resolver is introduced, ensure it does not trigger
  builds from test paths

Acceptance proof for Phase 4:

- scenario boot/test still fails fast when the required release product is
  absent
- failure text names the exact missing canonical product
- no hidden build side effects are introduced into scenario commands

### Phase 5. Demote stage wrappers from default UX to explicit compatibility

Goal:

- keep shims if needed, but stop teaching them as the primary operator model

Recommended file ownership:

- `justfile`
- `xtask/README.md`
- `testing/install-tests/src/bin/install-tests.rs`
- `distro-builder/docs/03_MIGRATION_STAGELESS.md`

Required changes:

- in `justfile`:
  - keep compatibility aliases only where still necessary
  - clearly mark `stage*`, `test*`, and `build-up-to` as migration shims
  - remove the special `03Install` shortcut from the default mental model
- in `xtask/README.md`:
  - replace stage examples with scenario examples
- in `install-tests.rs`:
  - replace "stage workflows" wording with "scenario workflows" or explicit
    `xtask scenarios ...` commands
- in `03_MIGRATION_STAGELESS.md`:
  - update stale repo-reality text so docs no longer argue against the current
    codebase

Acceptance proof for Phase 5:

- a new operator reading repo docs encounters products/releases/scenarios first
- compatibility stage aliases remain possible but are visibly non-canonical

### Phase 6. Isolate remaining validation compatibility residue

Goal:

- make the remaining stage vocabulary explicitly compatibility/validation-only

Recommended file ownership:

- `distro-contract/src/error.rs`
- `distro-contract/src/validate.rs`
- `distro-contract/src/runtime.rs`
- `distro-builder/src/bin/artifact_paths.rs`

Recommended approach:

- do not start by deleting `StageId`
- first ensure every remaining stage-attributed surface is genuinely
  validation-only
- then, if needed, introduce a broader attribution model later

Short-term required changes:

- mark stage-dir/path helpers as compatibility shims in docs/comments
- avoid expanding stage-oriented helpers into new orchestration code
- keep canonical build/runtime validators product/ring/scenario aware where
  possible

Acceptance proof for Phase 6:

- remaining stage references are limited to:
  - validation
  - scenarios/checkpoints
  - explicit compatibility helpers

### Phase 7. Fix canonical scenario-script installation and prove the full path

Goal:

- remove a real migration regression and use it as part of Track 05 proof

Recommended file ownership:

- `distro-builder/src/pipeline/scripts.rs`
- `distro-builder/src/pipeline/products.rs`
- `testing/install-tests/test-scripts/README.md`

Required changes:

- make canonical builder-side script installation match canonical scenario names
- stop filtering only `stage-*.sh` when the canonical scripts are:
  - `live-boot.sh`
  - `live-tools.sh`
  - `install.sh`
  - `installed-boot.sh`
  - `automated-login.sh`
  - `installed-tools.sh`
- keep shared library installation working
- add tests proving canonical product preparation installs the expected scenario
  scripts into the prepared rootfs

Acceptance proof for Phase 7:

- canonical prepared live products contain the scenario evidence scripts named in
  `scenarios.toml`
- scenario SSH execution no longer depends on legacy crate behavior

## First Implementation Cut

The smallest honest cut for this track is:

1. define planner behavior in terms of logical dependency closure from a requested target
2. make `release build iso <distro> <product>` auto-materialize missing parent product prerequisites instead of failing early
3. keep compatibility stage wrappers, but mark them as explicit migration shims rather than default workflow
4. add tests that prove a top-level release request succeeds from an empty product-cache state when lower-ring inputs are buildable

## Recommended First PR Stack

If this work is split into reviewable changes, the best stack is:

1. planner scaffolding only
   - introduce planner types/tests without changing public behavior
2. product-preparation decoupling
   - make derived products consume explicit resolved inputs
3. recursive release realization
   - replace parent-product preflight failure
4. scenario-script installation fix
   - align canonical installed scripts with canonical scenario evidence names
5. wrapper/doc cleanup
   - demote stage aliases and update stale docs

This order minimizes churn and keeps behavior changes reviewable.

## Non-Goals

- deleting stages from checkpoint docs
- removing scenario ladders from install/runtime verification
- pretending the DAG is a strict four-step linear pipeline
- forbidding all compatibility aliases immediately

Stages may remain.
They just may not remain the canonical build orchestration model.

## Relationship To Other Tracks

- Track 03 says stages are no longer the architecture and products/transforms/releases are the canonical model
- Track 04 says ownership must move into the ring/owner family
- This track says orchestration must also obey that model

Without this track, Track 04 can still succeed while the operator experience remains historically stage-driven.
That is exactly the failure mode this document exists to prevent.
