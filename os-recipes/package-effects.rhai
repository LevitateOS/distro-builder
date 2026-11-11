// The package-effects step: the lifecycle both package backends share for
// the first half of making a system tree. A backend recipe starts with
// `//! extends: package-effects.rhai` and supplies three hooks:
//
//   verify_inputs(ctx)          check its inputs against the locks it imports
//   unpack(ctx, root)           put the locked packages' files into `root`:
//                               the composed tree (`copy_composed`), plus
//                               whatever the backend layers on it
//   package_effects(ctx, root)  do what the packages' scripts would have done
//
// Inputs every backend takes: `composed`, the tree the builder composed from
// the locked packages (`tree.tar`, the tree with its target metadata manifest,
// as `target_pack_tree` writes it; `packages.txt`, one `<sha256> <id>` line
// per package in the order they were composed), and `flavour`, `base` or
// `desktop`: which locks the tree is made of.
//
// Then the tree's target metadata is finalized, the build's caches and logs
// are dropped, and the step publishes the tree as one archive (the `tree`
// output, `target_pack_tree`'s format; only the archive crosses to the next
// step), which the system recipe (`rootfs-producer.rhai`) copies and
// configures. This step imports neither the installed-system policy nor
// any configuration input, so a configuration change never reruns it.
const RECIPE_API = 2;
const NEEDS = #{};
import "system-files.rhai" as files;

fn root_of(ctx) { join_path(ctx.runtime.paths.output, "tree") }
fn archive_of(ctx) { join_path(ctx.runtime.paths.output, "tree.tar") }
fn stamp_of(ctx) { join_path(ctx.runtime.paths.output, "tree.built") }

fn composed_file(ctx, name) {
    join_path(join_path(ctx.runtime.paths.inputs, "composed"), name)
}

// The `flavour` input: "base" or "desktop".
fn flavour(ctx) {
    let value = trim(read_file(join_path(ctx.runtime.paths.inputs, "flavour")));
    if value != "base" && value != "desktop" { throw "the flavour input is " + value + ", not base or desktop"; }
    value
}

// Refuses a composed tree that is not made of exactly the lock rows `rows`
// ([name, id, URL, SHA256]), in that order.
fn verify_composed(ctx, rows) {
    if !is_file(composed_file(ctx, "tree.tar")) { throw "the composed input lacks tree.tar"; }
    let lines = [];
    for line in read_file(composed_file(ctx, "packages.txt")).split("\n") {
        let line = trim(line);
        if line != "" { lines.push(line); }
    }
    if lines.len() != rows.len() {
        throw "the composed tree is made of " + lines.len() + " packages, the locks name " + rows.len();
    }
    for i in 0..rows.len() {
        let digest = lines[i].split(" ")[0];
        if digest != rows[i][3].to_lower() {
            throw "package " + (i + 1) + " of the composed tree is " + lines[i] + ", the locks name " + rows[i][1];
        }
    }
}

// The composed tree, its files, links, staging modes and target metadata
// manifest, into the empty `root`.
fn copy_composed(ctx, root) {
    progress(0, 0, "copying the composed tree");
    target_unpack_tree(composed_file(ctx, "tree.tar"), root);
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
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the package tree is not built"); }
    if !is_file(archive_of(ctx)) { return needs_work(ctx, "the package tree is not archived"); }
    ready(ctx)
}

fn build(ctx) {
    let root = root_of(ctx);
    for file in [stamp_of(ctx), archive_of(ctx)] { if is_file(file) { rm(file); } }
    if is_dir(root) { rm(root); }
    mkdir(root);
    progress(0, 0, "unpacking packages");
    unpack(ctx, root);
    progress(0, 0, "package effects");
    package_effects(ctx, root);
    progress(0, 0, "finalizing the tree");
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
    files::directory(root, "var/log/audit", 0o700);
    progress(0, 0, "archiving the tree");
    // The archive takes the tree's place (target_pack_tree removes it), so
    // only the archive is published and the domain never holds two trees.
    target_pack_tree(root, archive_of(ctx));
    write_file(stamp_of(ctx), "built\n");
    ctx
}

fn with_outputs(ctx) { ctx.outputs = #{ tree: archive_of(ctx) }; ctx }

fn is_installed(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { return status; }
    ready(with_outputs(status.ctx))
}

fn install(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { throw status.reason; }
    with_outputs(status.ctx)
}

fn cleanup(ctx, reason) {
    if reason == "manual" && is_file(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    ctx
}

fn remove(ctx) {
    let root = root_of(ctx);
    if is_dir(root) { rm(root); }
    for file in [stamp_of(ctx), archive_of(ctx)] { if is_file(file) { rm(file); } }
    ctx.outputs = #{};
    ctx
}
