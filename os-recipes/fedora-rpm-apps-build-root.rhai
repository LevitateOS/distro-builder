// The root the desktop apps (gui/) are compiled in on a Fedora variant: the
// base lock's RPMs and the apps-build lock's, without firmware and without
// their scripts, composed by the builder from its unpack store into one tree
// (the `composed` input: `tree.tar`, the tree in `target_pack_tree`'s format,
// and `packages.txt`, one `<sha256> <id>` line per package in order). The
// builder makes a root image of the tree and runs `desktop-apps.rhai` in it,
// so the apps are compiled by the distro's own compiler against the distro's
// own C++ runtime and Qt, the ones the desktop system ships. The tree is
// never part of an image.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ composed: "directory" }, outputs: #{ root: "directory" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 7200 };
import "sources/levitate-rpm-lock.rhai" as lock;
import "sources/levitate-rpm-apps-build-lock.rhai" as build_lock;
let ctx = #{
    name: "fedora-rpm-apps-build-root",
    description: "The Fedora " + lock::release() + " root the desktop apps are compiled in",
    // [input, lock rows]
    sets: [["rpms", lock::packages()], ["apps_build_rpms", build_lock::packages()]],
};

fn root_of(ctx) { join_path(ctx.runtime.paths.output, "root") }
fn stamp_of(ctx) { join_path(ctx.runtime.paths.output, "root.built") }

// A compiler needs no firmware; it is most of the base lock's bytes, and the
// root is copied for every run in it. The builder composes only these
// packages (`apps_build_wanted` in src/compose.rs); verify_inputs checks it.
fn wanted(name) { !name.contains("firmware") && !name.contains("microcode") }

// The lock rows the root is made of, in order.
fn root_rows(ctx) {
    let rows = [];
    for set in ctx.sets { for rpm in set[1] { if wanted(rpm[0]) { rows.push(rpm); } } }
    rows
}

fn composed_file(ctx, name) { join_path(join_path(ctx.runtime.paths.inputs, "composed"), name) }

// Refuses a composed tree that is not made of exactly these packages, in this order.
fn verify_inputs(ctx) {
    if !is_file(composed_file(ctx, "tree.tar")) { throw "the composed input lacks tree.tar"; }
    let lines = [];
    for line in read_file(composed_file(ctx, "packages.txt")).split("\n") {
        let line = trim(line);
        if line != "" { lines.push(line); }
    }
    let expected = root_rows(ctx);
    if lines.len() != expected.len() {
        throw "the composed root is made of " + lines.len() + " packages, the locks name " + expected.len();
    }
    for i in 0..expected.len() {
        if lines[i].split(" ")[0] != expected[i][3].to_lower() {
            throw "package " + (i + 1) + " of the composed root is " + lines[i] + ", the locks name " + expected[i][1];
        }
    }
}

// What the tree must have to compile the apps: the first one missing, or "".
fn missing(root) {
    for path in ["usr/bin/g++", "usr/bin/ld", "usr/bin/make", "usr/bin/cmake", "usr/bin/sh",
                 "usr/lib64/cmake/Qt6Widgets/Qt6WidgetsConfig.cmake", "usr/lib64/cmake/Qt6Svg/Qt6SvgConfig.cmake"] {
        if !exists(join_path(root, path)) { return "/" + path; }
    }
    ""
}

fn is_acquired(ctx) { verify_inputs(ctx); ready(ctx) }
fn acquire(ctx) { verify_inputs(ctx); ctx }

fn is_built(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the root is not unpacked"); }
    let gap = missing(root_of(ctx));
    if gap != "" { return needs_work(ctx, "the root lacks " + gap); }
    ready(ctx)
}

fn build(ctx) {
    let root = root_of(ctx);
    if exists(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    if is_dir(root) { rm(root); }
    mkdir(root);
    log("Copying the composed Fedora " + lock::release() + " compiler root...");
    progress(0, 0, "copying the composed root");
    target_unpack_tree(composed_file(ctx, "tree.tar"), root);
    // binutils' install script chooses the linker through `alternatives`;
    // no script ran, so name the default one here.
    if !exists(join_path(root, "usr/bin/ld")) {
        if !is_file(join_path(root, "usr/bin/ld.bfd")) { throw "the root has no linker (/usr/bin/ld.bfd)"; }
        ln_force("ld.bfd", join_path(root, "usr/bin/ld"));
    }
    let gap = missing(root);
    if gap != "" { throw "the unpacked root lacks " + gap + ": add the package that has it to desktop.apps_build_packages and lock again"; }
    write_file(stamp_of(ctx), lock::locked_on() + " " + build_lock::locked_on() + "\n");
    ctx
}

fn with_outputs(ctx) { ctx.outputs = #{ root: root_of(ctx) }; ctx }
fn is_installed(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the root is not unpacked"); }
    ready(with_outputs(ctx))
}
fn install(ctx) { with_outputs(ctx) }

fn remove(ctx) {
    if exists(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    if is_dir(root_of(ctx)) { rm(root_of(ctx)); }
    ctx.outputs = #{};
    ctx
}
