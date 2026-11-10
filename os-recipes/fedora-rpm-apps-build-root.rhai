// The root the desktop apps (gui/) are compiled in on a Fedora variant: the
// base lock's RPMs (the `rpms` input) and the apps-build lock's (the
// `apps_build_rpms` input) unpacked into one tree, without their scripts. The
// builder makes a root image of the tree and runs `desktop-apps.rhai` in it,
// so the apps are compiled by the distro's own compiler against the distro's
// own C++ runtime and Qt, the ones the desktop system ships. The tree is
// never part of an image.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ rpms: "directory", apps_build_rpms: "directory" }, outputs: #{ root: "directory" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 3600 };
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
// root is copied for every run in it.
fn wanted(name) { !name.contains("firmware") && !name.contains("microcode") }

// Each RPM the root gets, with its lock row: [file, row].
fn rpm_files(ctx) {
    let files = [];
    for set in ctx.sets {
        for rpm in set[1] {
            if wanted(rpm[0]) { files.push([join_path(join_path(ctx.runtime.paths.inputs, set[0]), basename(rpm[2])), rpm]); }
        }
    }
    files
}

fn verify_inputs(ctx) {
    for entry in rpm_files(ctx) {
        if !is_file(entry[0]) { throw "the rpm inputs lack " + basename(entry[0]) + "; run the rootfs source preseed for the desktop"; }
        verify_sha256(entry[0], entry[1][3]);
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
    log("Extracting the Fedora " + lock::release() + " compiler root...");
    for entry in rpm_files(ctx) { target_extract_rpm(entry[0], root); }
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
