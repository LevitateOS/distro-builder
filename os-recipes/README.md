# OS build recipes

This collection produces artifacts for the LevitateOS and AcornOS builder:
source root filesystems, configured installed systems, live overlays, kernels,
toolchains and tools. `sources/` holds package locks and `kernel/` holds shared
kernel facts. The builder selects this directory explicitly as its recipe
search path; variant declarations select the rootfs and preseed producers.

Every executable recipe uses the shared Recipe API 2 Rhai format and engine in
`tools/recipe`, with its own `NEEDS`. Its `install` hook writes an owned output
tree inside Ginux. Recipe publishes the declared files and directories, and the
builder consumes them to assemble images. recab owns activation and updates of
installed system images.

Base and desktop packages, including applications bundled into a desktop image,
are image-owned here. The workspace's top-level
[`recipes/`](../../../recipes/README.md) is the separate catalog for software
users manage independently of the image. Both collections share one format and
engine; their consumers and output lifecycles differ. User package activation is
not implemented by these OS producers.
