const RECIPE_API = 1;
// The root the desktop apps (tools/gui/) and the login screen (reclogin.rhai)
// are compiled in on a Fedora variant: the base lock's RPMs and the apps-build
// lock's, without firmware and without
// their scripts, composed by the builder from its unpack store into one tree
// (the `composed` input: `tree.tar`, the tree in `target_pack_tree`'s format,
// and `packages.txt`, one `<sha256> <id>` line per package in order). The
// builder makes a root image of the tree and runs `desktop-apps.rhai` in it,
// so the apps are compiled by the distro's own compiler against the distro's
// own C++ runtime and Qt, the ones the desktop system ships. The tree is
// never part of an image.
const NEEDS = #{ inputs: #{ composed: "directory" }, outputs: #{ root: "directory" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 7200, runtime: "minimal" };
const PACKAGE = #{ name: "fedora-rpm-apps-build-root", description: "The Fedora 45 root the desktop apps are compiled in" };
import "sources/levitate-rpm-lock.rhai" as lock;
import "sources/levitate-rpm-apps-build-lock.rhai" as build_lock;

// A compiler needs no firmware; it is most of the base lock's bytes, and the
// root is copied for every run in it. The builder composes only these
// packages (`apps_build_wanted` in src/compose.rs); verify_inputs checks it.
fn wanted(name) { !name.contains("firmware") && !name.contains("microcode") }

// The lock rows the root is made of, in order: the base lock's, then the
// apps-build lock's.
fn root_rows() {
    let rows = [];
    for rpms in [lock::packages(), build_lock::packages()] { for rpm in rpms { if wanted(rpm[0]) { rows.push(rpm); } } }
    rows
}

fn composed_file(name) { join_path(INPUTS.composed, name) }

// Refuses a composed tree that is not made of exactly these packages, in this order.
fn verify_inputs() {
    if !is_file(composed_file("tree.tar")) { throw "the composed input lacks tree.tar"; }
    let lines = [];
    for line in read_file(composed_file("packages.txt")).split("\n") {
        let line = trim(line);
        if line != "" { lines.push(line); }
    }
    let expected = root_rows();
    if lines.len() != expected.len() {
        throw "the composed root is made of " + lines.len() + " packages, the locks name " + expected.len();
    }
    for i in 0..expected.len() {
        if lines[i].split(" ")[0] != expected[i][3].to_lower() {
            throw "package " + (i + 1) + " of the composed root is " + lines[i] + ", the locks name " + expected[i][1];
        }
    }
}

// What the tree must have to compile the apps and reclogin: the first one
// missing, or "".
fn missing(root) {
    for path in ["usr/bin/g++", "usr/bin/ld", "usr/bin/make", "usr/bin/cmake", "usr/bin/sh",
                 "usr/lib64/cmake/Qt6Widgets/Qt6WidgetsConfig.cmake", "usr/lib64/cmake/Qt6Svg/Qt6SvgConfig.cmake",
                 // reclogin (reclogin.rhai): Rust, and the libraries it links
                 "usr/bin/cargo", "usr/bin/rustc", "usr/bin/cc",
                 "usr/lib64/libpam.so", "usr/lib64/libinput.so", "usr/lib64/libxkbcommon.so"] {
        if !exists(join_path(root, path)) { return "/" + path; }
    }
    ""
}

fn build(src, out) {
    verify_inputs();
    log("Copying the composed Fedora " + lock::release() + " compiler root...");
    progress(0, 0, "copying the composed root");
    target_unpack_tree(composed_file("tree.tar"), out);
    // binutils' install script chooses the linker through `alternatives`;
    // no script ran, so name the default one here.
    if !exists(join_path(out, "usr/bin/ld")) {
        if !is_file(join_path(out, "usr/bin/ld.bfd")) { throw "the root has no linker (/usr/bin/ld.bfd)"; }
        ln_force("ld.bfd", join_path(out, "usr/bin/ld"));
    }
}

fn verify(out) {
    let gap = missing(out);
    if gap != "" { throw "the unpacked root lacks " + gap + ": add the package that has it to desktop.apps_build_packages and lock again"; }
}
