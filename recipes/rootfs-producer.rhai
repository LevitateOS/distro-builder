// The rootfs producer: the lifecycle both package backends share. A backend
// recipe starts with `//! extends: rootfs-producer.rhai`, sets `ctx.init` and
// supplies three hooks:
//
//   verify_inputs(ctx)          check the locked packages it imports
//   unpack(ctx, root)           unpack them into `root`, without their scripts
//   package_effects(ctx, root)  do what those scripts would have done
//
// Then the installed-system policy (`installed-system.rhai`) makes the tree
// the installable system, and the tree must have every command in the
// `capabilities` input (distro/contract/capabilities.txt) before it counts as
// built. Recipe's state identity already covers this file, the backend, their
// imports and every input, so a change to any of them starts from an empty
// output; the stamp beside the tree only records that the build finished.
const RECIPE_API = 2;
import "installed-system.rhai" as installed;

fn root_of(ctx) { join_path(ctx.runtime.paths.output, "rootfs") }
fn stamp_of(ctx) { join_path(ctx.runtime.paths.output, "rootfs.built") }

// What a built tree must have: every shared command on PATH, and what the
// install tools need on the target (recpart's mkfs.fat, bootctl's EFI binary).
// The first one missing, or "".
fn missing(ctx, root) {
    let needed = [];
    for line in read_file(join_path(ctx.runtime.paths.inputs, "capabilities")).split("\n") {
        let line = trim(line);
        if line.starts_with("command ") { needed.push(trim(line.sub_string(8))); }
    }
    for command in needed + ["mkfs.fat"] {
        let found = false;
        for dir in ["usr/bin", "usr/sbin", "bin", "sbin"] {
            if exists(join_path(root, dir + "/" + command)) { found = true; break; }
        }
        if !found { return "the command " + command; }
    }
    let efi = "usr/lib/systemd/boot/efi/systemd-bootx64.efi";
    if !is_file(join_path(root, efi)) { return efi; }
    ""
}

fn is_acquired(ctx) {
    verify_inputs(ctx);
    ready(ctx)
}

fn acquire(ctx) {
    verify_inputs(ctx);
    ctx
}

fn is_built(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "rootfs not built"); }
    let gap = missing(ctx, root_of(ctx));
    if gap != "" { return needs_work(ctx, "the rootfs lacks " + gap); }
    ready(ctx)
}

fn build(ctx) {
    let root = root_of(ctx);
    if is_file(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    if is_dir(root) { rm(root); }
    mkdir(root);
    unpack(ctx, root);
    package_effects(ctx, root);
    // Resolve the packages' file owners against their accounts and make the
    // staging tree writable before cleanup; package modes stay in metadata.
    target_finalize(root);
    // No caches or logs from the build.
    for dir in ["var/cache", "var/log"] {
        let path = join_path(root, dir);
        if exists(path) { rm(path); }
        mkdir(path);
    }
    // auditd's log directory.
    installed::directory(root, "var/log/audit", 0o700);
    installed::apply(root, ctx.runtime.paths.inputs, ctx.init);
    target_finalize(root);
    let gap = missing(ctx, root);
    if gap != "" { throw "the built rootfs lacks " + gap; }
    write_file(stamp_of(ctx), "built\n");
    ctx
}

fn is_installed(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { return status; }
    ctx = status.ctx;
    ctx.outputs = #{rootfs: root_of(ctx)};
    ready(ctx)
}

fn install(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { throw status.reason; }
    ctx = status.ctx;
    ctx.outputs = #{rootfs: root_of(ctx)};
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" && is_file(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    ctx
}

fn remove(ctx) {
    let root = root_of(ctx);
    if is_dir(root) { rm(root); }
    if is_file(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    ctx.outputs = #{};
    ctx
}
