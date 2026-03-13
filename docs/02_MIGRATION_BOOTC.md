# 02 bootc Migration

Status: cancelled

## X

X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X
X                                                                         X
X  THIS TRACK IS CANCELLED.                                               X
X                                                                         X
X  The repository is keeping the current A/B model instead of migrating   X
X  LevitateOS/RalphOS/AcornOS/IuppiterOS to `bootc`.                      X
X                                                                         X
X  Do not start new `bootc` implementation work from this document.       X
X                                                                         X
X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X X

## Decision

The repo will keep its current A/B runtime/update model as the canonical direction.

Reason:

- `bootc` may fit the Levitate/Ralph family, but it is not justified as the universal backend for all variants.
- AcornOS and IuppiterOS are Alpine/OpenRC/musl/EROFS-shaped and should not be forced into a Fedora/systemd-shaped migration by default.
- A repo-wide runtime/update model matters more than switching to `bootc` for only part of the variant set.

## What This Means

- keep the current A/B implementation as the foundation
- improve and harden the existing A/B contract/install/test model instead of replacing it with `bootc`
- do not treat `bootc` as the default future path in migration planning

## Follow-up Direction

If runtime/update work resumes, it should happen under an A/B-focused track, not under `bootc`.

That future work should focus on:

- making the runtime/update contract explicit
- removing legacy mutability inference
- cleaning up installer/backend ownership
- preserving one coherent model across all variants
