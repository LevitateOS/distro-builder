// The static busybox the live and the installed initramfs run (recinit): the
// prebuilt musl binary busybox.net publishes, pinned by its sha256.
const RECIPE_API = 2;
let ctx = #{
    name: "busybox-static",
    description: "Static busybox for the initramfs",
    version: "1.35.0",
    url: "https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox",
    sha256: "6e123e7f3202a8c1e9b1f94d8941580a25135382b99e8d3e34fb858bba311348",
    download: join_path(RUNTIME.paths.build, "busybox-1.35.0"),
    program: join_path(RUNTIME.paths.output, "bin/busybox"),
};

fn is_acquired(ctx) {
    if !is_file(ctx.download) { return needs_work(ctx, "busybox " + ctx.version + " is not downloaded"); }
    verify_sha256(ctx.download, ctx.sha256);
    ready(ctx)
}

fn acquire(ctx) {
    mkdir(dirname(ctx.download));
    download(ctx.url, ctx.download, ctx.sha256);
    ctx
}

fn is_built(ctx) { is_acquired(ctx) }
fn build(ctx) { ctx }

fn is_installed(ctx) {
    if !is_file(ctx.program) { return needs_work(ctx, "busybox is not installed"); }
    verify_sha256(ctx.program, ctx.sha256);
    ctx.outputs.busybox = ctx.program;
    ready(ctx)
}

fn install(ctx) {
    mkdir(dirname(ctx.program));
    copy_file(ctx.download, ctx.program);
    chmod(ctx.program, 0o755);
    ctx.outputs.busybox = ctx.program;
    ctx
}

fn remove(ctx) {
    if is_file(ctx.program) { rm(ctx.program); }
    ctx.outputs = #{};
    ctx
}
