// Explicit pinned HTTPS acquisition. Successful named outputs are reusable
// immutable inputs to the separate rootfs materializer.
const RECIPE_API = 2;
import "sources/fedora.rhai" as source;
let assets = source::facts();
let ctx = #{
    name: "fedora-preseed-iso",
    description: "Pinned Fedora rootfs source assets",
    iso_path: join_path(RUNTIME.paths.output, assets.iso_name),
    iso_url: assets.iso_url,
    iso_sha256: assets.sha256,
};

fn is_acquired(ctx) {
    if !is_file(ctx.iso_path) { return needs_work(ctx, "preseed ISO is missing"); }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    ctx.outputs = #{iso: ctx.iso_path};
    ready(ctx)
}

fn acquire(ctx) {
    if ctx.runtime.options.force_acquire || !is_file(ctx.iso_path) {
        download(ctx.iso_url, ctx.iso_path, ctx.iso_sha256);
    } else { verify_sha256(ctx.iso_path, ctx.iso_sha256); }
    ctx.outputs = #{iso: ctx.iso_path};
    ctx
}

fn is_built(ctx) { is_acquired(ctx) }
fn build(ctx) { ctx }
fn is_installed(ctx) { is_acquired(ctx) }
fn install(ctx) { ctx.outputs = #{iso: ctx.iso_path}; ctx }

fn remove(ctx) {
    if is_file(ctx.iso_path) { rm(ctx.iso_path); }
    ctx.outputs = #{};
    ctx
}
