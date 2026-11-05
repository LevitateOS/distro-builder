// Explicit pinned HTTPS acquisition. Successful named outputs are reusable
// immutable inputs to the separate rootfs materializer.
const RECIPE_API = 2;
import "sources/alpine.rhai" as source;
let assets = source::facts();
let ctx = #{
    name: "alpine-preseed-source-assets",
    description: "Pinned Alpine rootfs source assets",
    iso_path: join_path(RUNTIME.paths.output, assets.iso_name),
    iso_url: assets.iso_url,
    iso_sha256: assets.iso_sha256,
    apk_tools_path: join_path(RUNTIME.paths.output, assets.apk_tools_name),
    apk_tools_url: assets.apk_tools_url,
    apk_tools_sha256: assets.apk_tools_sha256,
};

fn is_acquired(ctx) {
    if !is_file(ctx.iso_path) || !is_file(ctx.apk_tools_path) {
        return needs_work(ctx, "preseed assets are missing");
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    ctx.outputs = #{iso: ctx.iso_path, apk_tools: ctx.apk_tools_path};
    ready(ctx)
}

fn acquire(ctx) {
    if ctx.runtime.options.force_acquire || !is_file(ctx.iso_path) {
        download(ctx.iso_url, ctx.iso_path, ctx.iso_sha256);
    } else { verify_sha256(ctx.iso_path, ctx.iso_sha256); }
    if ctx.runtime.options.force_acquire || !is_file(ctx.apk_tools_path) {
        download(ctx.apk_tools_url, ctx.apk_tools_path, ctx.apk_tools_sha256);
    } else { verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256); }
    ctx.outputs = #{iso: ctx.iso_path, apk_tools: ctx.apk_tools_path};
    ctx
}

fn is_built(ctx) { is_acquired(ctx) }
fn build(ctx) { ctx }
fn is_installed(ctx) { is_acquired(ctx) }
fn install(ctx) { ctx.outputs = #{iso: ctx.iso_path, apk_tools: ctx.apk_tools_path}; ctx }

fn remove(ctx) {
    for path in [ctx.iso_path, ctx.apk_tools_path] { if is_file(path) { rm(path); } }
    ctx.outputs = #{};
    ctx
}
