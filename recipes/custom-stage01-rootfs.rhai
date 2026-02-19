// Generic Stage 01 rootfs source producer for custom distro payloads.
//
// Produces:
// - BUILD_DIR/rootfs (copied from CUSTOM_ROOTFS_DIR)
//
// Required defines:
// - CUSTOM_ROOTFS_DIR (absolute or repo-relative path, resolved by caller)

let ctx = #{
    name: "custom-stage01-rootfs",
    description: "Custom rootfs producer for Stage 01",
    source_rootfs: CUSTOM_ROOTFS_DIR,
    rootfs_path: "",
};

fn _validate_source(path) {
    if !is_dir(path) {
        throw "custom rootfs source directory missing: " + path;
    }
    if !is_dir(join_path(path, "usr")) {
        throw "custom rootfs source must contain 'usr/' (missing under " + path + ")";
    }
}

fn is_acquired(ctx) {
    _validate_source(ctx.source_rootfs);
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs");
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);
    _validate_source(ctx.source_rootfs);
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs");
    ctx
}

fn is_built(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs");
    if !is_dir(join_path(rootfs, "usr")) {
        throw "rootfs not built";
    }
    ctx.rootfs_path = rootfs;
    ctx
}

fn build(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs");
    _validate_source(ctx.source_rootfs);

    shell("rm -rf " + rootfs);
    shell("mkdir -p " + rootfs);
    shell("cp -a " + ctx.source_rootfs + "/. " + rootfs + "/");

    if !is_dir(join_path(rootfs, "usr")) {
        throw "custom rootfs copy incomplete: missing " + join_path(rootfs, "usr");
    }

    ctx.rootfs_path = rootfs;
    ctx
}

fn is_installed(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs");
    if !is_dir(join_path(rootfs, "usr")) {
        throw "rootfs not installed";
    }
    ctx.rootfs_path = rootfs;
    ctx
}

fn install(ctx) {
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs");
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" {
        let rootfs = join_path(BUILD_DIR, "rootfs");
        if is_dir(rootfs) {
            rm(rootfs);
        }
    }
    ctx
}
