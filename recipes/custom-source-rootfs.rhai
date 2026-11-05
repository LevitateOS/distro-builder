// Produce a rootfs from one explicit immutable source tree:
//   --input rootfs=PATH
// The coordinator publishes ctx.outputs.rootfs after successful installation.
const RECIPE_API = 2;
let ctx = #{
    name: "custom-source-rootfs",
    description: "Custom rootfs source producer",
    source_rootfs: join_path(RUNTIME.paths.inputs, "rootfs"),
    rootfs_path: join_path(RUNTIME.paths.output, "rootfs"),
};

fn _validate_source(path) {
    if !is_dir(path) { throw "custom rootfs input directory missing: " + path; }
    if !is_dir(join_path(path, "usr")) {
        throw "custom rootfs input must contain usr/: " + path;
    }
}

fn is_acquired(ctx) {
    if !is_dir(ctx.source_rootfs) { return needs_work(ctx, "rootfs input is missing"); }
    _validate_source(ctx.source_rootfs);
    ready(ctx)
}

fn acquire(ctx) { _validate_source(ctx.source_rootfs); ctx }

fn is_built(ctx) {
    if !is_dir(join_path(ctx.rootfs_path, "usr")) {
        return needs_work(ctx, "rootfs has not been copied");
    }
    ready(ctx)
}

fn build(ctx) {
    _validate_source(ctx.source_rootfs);
    if is_dir(ctx.rootfs_path) { rm(ctx.rootfs_path); }
    mkdir(ctx.rootfs_path);
    copy_tree_contents(ctx.source_rootfs, ctx.rootfs_path);
    _validate_source(ctx.rootfs_path);
    ctx
}

fn is_installed(ctx) {
    if !is_dir(join_path(ctx.rootfs_path, "usr")) {
        return needs_work(ctx, "rootfs output is missing");
    }
    ctx.outputs.rootfs = ctx.rootfs_path;
    ready(ctx)
}

fn install(ctx) {
    _validate_source(ctx.rootfs_path);
    ctx.outputs.rootfs = ctx.rootfs_path;
    ctx
}

fn remove(ctx) {
    if is_dir(ctx.rootfs_path) { rm(ctx.rootfs_path); }
    ctx.outputs = #{};
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" || reason == "auto.build.failure" {
        if is_dir(ctx.rootfs_path) { rm(ctx.rootfs_path); }
        ctx.outputs = #{};
    }
    ctx
}
