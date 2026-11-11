// The root the desktop apps (gui/) are compiled in on an Alpine variant: the
// base lock's APKs (the `apks` input) and the apps-build lock's (the
// `apps_build_apks` input) installed into one tree by the locked apk.static,
// without their scripts. The builder makes a root image of the tree and runs
// `desktop-apps.rhai` in it, so the apps are compiled by the distro's own
// compiler against the distro's own C library, C++ runtime and Qt, the ones
// the desktop system ships. The tree is never part of an image.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ apks: "directory", apps_build_apks: "directory" }, outputs: #{ root: "directory" }, memory_mib: 4096, storage_max_objects: 196608, timeout_secs: 7200 };
import "sources/acorn-apk-lock.rhai" as lock;
import "sources/acorn-apk-apps-build-lock.rhai" as build_lock;
let ctx = #{
    name: "alpine-apps-build-root",
    description: "The Alpine " + lock::release() + " root the desktop apps are compiled in",
    // [input, lock rows]
    sets: [["apks", lock::packages()], ["apps_build_apks", build_lock::packages()]],
    apk_tools: lock::apk_tools(),
};

fn root_of(ctx) { join_path(ctx.runtime.paths.output, "root") }
fn stamp_of(ctx) { join_path(ctx.runtime.paths.output, "root.built") }
fn set_dir(ctx, set) { join_path(ctx.runtime.paths.inputs, set[0]) }
fn apk_file(ctx, set, row) { join_path(set_dir(ctx, set), basename(row[2])) }

// A compiler needs no firmware; it is most of the base lock's bytes, and the
// root is copied for every run in it. Nothing depends on a firmware package.
fn wanted(name) { !name.contains("firmware") && !name.contains("ucode") }

fn verify_inputs(ctx) {
    let files = [[apk_file(ctx, ctx.sets[0], ctx.apk_tools), ctx.apk_tools]];
    for set in ctx.sets { for row in set[1] { if wanted(row[0]) { files.push([apk_file(ctx, set, row), row]); } } }
    for entry in files {
        if !is_file(entry[0]) { throw "the apk inputs lack " + basename(entry[0]) + "; run the rootfs source preseed for the desktop"; }
        verify_sha256(entry[0], entry[1][3]);
    }
}

// What the tree must have to compile the apps: the first one missing, or "".
fn missing(root) {
    for path in ["usr/bin/g++", "usr/bin/ld", "usr/bin/make", "usr/bin/cmake", "bin/sh",
                 "usr/lib/cmake/Qt6Widgets/Qt6WidgetsConfig.cmake", "usr/lib/cmake/Qt6Svg/Qt6SvgConfig.cmake"] {
        if !exists(join_path(root, path)) { return "/" + path; }
    }
    ""
}

fn is_acquired(ctx) { verify_inputs(ctx); ready(ctx) }
fn acquire(ctx) { verify_inputs(ctx); ctx }

fn is_built(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the root is not installed"); }
    let gap = missing(root_of(ctx));
    if gap != "" { return needs_work(ctx, "the root lacks " + gap); }
    ready(ctx)
}

fn build(ctx) {
    let build = ctx.runtime.paths.build;
    let root = root_of(ctx);
    if exists(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    if is_dir(root) { rm(root); }
    mkdir(root);
    let tools = join_path(build, "apk-tools");
    if is_dir(tools) { rm(tools); }
    mkdir(tools);
    extract_with_format(apk_file(ctx, ctx.sets[0], ctx.apk_tools), tools, "apk");
    let apk = join_path(tools, "sbin/apk.static");
    if !is_file(apk) { throw "the locked apk-tools-static has no sbin/apk.static"; }
    chmod(apk, 0o755);

    // apk checks every package's signature against the release key.
    let key = lock::signing_key();
    let keys = join_path(root, "etc/apk/keys");
    mkdir(keys);
    write_file(join_path(keys, key[0]), key[1]);
    chmod(join_path(keys, key[0]), 0o644);
    let cache = join_path(build, "apk-cache");
    mkdir(cache);
    log("Installing the Alpine " + lock::release() + " compiler root...");
    let args = ["--root", root, "--usermode", "--initdb", "--no-progress", "--no-scripts",
        "--force-non-repository", "--no-network", "--cache-dir", cache, "add"];
    for set in ctx.sets { for row in set[1] { if wanted(row[0]) { args.push(apk_file(ctx, set, row)); } } }
    // APK 3 installs as a non-root user in usermode.
    exec_checked(apk, args, #{uid: 1000, gid: 1000});
    // BusyBox's install script links its commands; no script ran, and the
    // build runs its commands through /bin/sh.
    if !exists(join_path(root, "bin/sh")) {
        if !is_file(join_path(root, "bin/busybox")) { throw "the root has no shell (/bin/busybox)"; }
        ln_force("busybox", join_path(root, "bin/sh"));
    }
    let gap = missing(root);
    if gap != "" { throw "the installed root lacks " + gap + ": add the package that has it to desktop.apps_build_packages and lock again"; }
    write_file(stamp_of(ctx), lock::locked_on() + " " + build_lock::locked_on() + "\n");
    ctx
}

fn with_outputs(ctx) { ctx.outputs = #{ root: root_of(ctx) }; ctx }
fn is_installed(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the root is not installed"); }
    ready(with_outputs(ctx))
}
fn install(ctx) { with_outputs(ctx) }

fn remove(ctx) {
    if exists(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    if is_dir(root_of(ctx)) { rm(root_of(ctx)); }
    ctx.outputs = #{};
    ctx
}
