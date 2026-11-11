// The rootfs producer: the system recipe both package backends share, the
// second half of making a system tree. A backend recipe starts with
// `//! extends: rootfs-producer.rhai` and sets `ctx.init`.
//
// The `system` input is the package-effects step's tree (`package-effects.rhai`
// and its backends: the locked packages composed, their effects applied, the
// target metadata finalized), one archive in `target_pack_tree`'s format. This
// recipe copies it, the installed-system policy (`installed-system.rhai`)
// makes it the installable system, and the tree must have every command in
// the `capabilities` input (distro/contract/capabilities.txt) before it counts
// as built. Recipe's state identity covers this file, the backend, their
// imports and every input, so a change to any of them starts from an empty
// output; a configuration change reruns this step only, not the packages'.
// The stamp beside the tree only records that the build finished.
const RECIPE_API = 2;
const NEEDS = #{};
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

fn system_archive(ctx) { join_path(ctx.runtime.paths.inputs, "system") }

fn verify_inputs(ctx) {
    if !is_file(system_archive(ctx)) { throw "the system input (the package-effects step's tree) is missing"; }
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
    // The package tree, as the package-effects step left it: packages
    // composed, their effects applied, metadata finalized, caches and logs
    // dropped, auditd's log directory made.
    progress(0, 0, "copying the package tree");
    target_unpack_tree(system_archive(ctx), root);
    progress(0, 0, "system configuration");
    installed::apply(root, ctx.runtime.paths.inputs, ctx.init);
    progress(0, 0, "finalizing the configured tree");
    target_finalize(root);
    progress(0, 0, "capability check");
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
