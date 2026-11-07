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
    extra_apks_dir: join_path(RUNTIME.paths.output, "extra-apks"),
    extra_apks: assets.extra_apks,
};

fn _outputs(ctx) {
    #{iso: ctx.iso_path, apk_tools: ctx.apk_tools_path, extra_apks: ctx.extra_apks_dir}
}

fn _missing(ctx) {
    if !is_file(ctx.iso_path) || !is_file(ctx.apk_tools_path) { return true; }
    for apk in ctx.extra_apks {
        if !is_file(join_path(ctx.extra_apks_dir, apk[0])) { return true; }
    }
    false
}

fn _fetch(url, path, sha256, force) {
    if force || !is_file(path) {
        download(url, path, sha256);
    } else { verify_sha256(path, sha256); }
}

fn is_acquired(ctx) {
    if _missing(ctx) {
        return needs_work(ctx, "preseed assets are missing");
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    for apk in ctx.extra_apks {
        verify_sha256(join_path(ctx.extra_apks_dir, apk[0]), apk[1]);
    }
    ctx.outputs = _outputs(ctx);
    ready(ctx)
}

fn acquire(ctx) {
    let force = ctx.runtime.options.force_acquire;
    _fetch(ctx.iso_url, ctx.iso_path, ctx.iso_sha256, force);
    _fetch(ctx.apk_tools_url, ctx.apk_tools_path, ctx.apk_tools_sha256, force);
    mkdir(ctx.extra_apks_dir);
    for apk in ctx.extra_apks {
        _fetch(apk[2] + apk[0], join_path(ctx.extra_apks_dir, apk[0]), apk[1], force);
    }
    ctx.outputs = _outputs(ctx);
    ctx
}

fn is_built(ctx) { is_acquired(ctx) }
fn build(ctx) { ctx }
fn is_installed(ctx) { is_acquired(ctx) }
fn install(ctx) { ctx.outputs = _outputs(ctx); ctx }

fn remove(ctx) {
    for path in [ctx.iso_path, ctx.apk_tools_path] { if is_file(path) { rm(path); } }
    if is_dir(ctx.extra_apks_dir) { rm(ctx.extra_apks_dir); }
    ctx.outputs = #{};
    ctx
}
