const RECIPE_API = 1;
// The root the desktop apps (tools/gui/), the display server (xlibre.rhai) and
// the login screen (reclogin.rhai) are compiled in on an Alpine variant: the
// base lock's APKs (the `apks` input) and the apps-build lock's (the
// `apps_build_apks` input) installed into one
// tree by the locked apk.static, without their scripts. The builder makes a
// root image of the tree and runs `desktop-apps.rhai` and `xlibre.rhai` in
// it, so they are compiled by the distro's own compiler against the distro's
// own C library, C++ runtime, Qt and X libraries, the ones the desktop system
// ships. The tree is never part of an image.
const NEEDS = #{ inputs: #{ apks: "directory", apps_build_apks: "directory" }, outputs: #{ root: "directory" }, memory_mib: 4096, storage_max_objects: 196608, timeout_secs: 7200 };
const PACKAGE = #{ name: "alpine-apps-build-root", description: "The Alpine v3.24 root the desktop apps are compiled in" };
import "sources/acorn-apk-lock.rhai" as lock;
import "sources/acorn-apk-apps-build-lock.rhai" as build_lock;

// A compiler needs no firmware; it is most of the base lock's bytes, and the
// root is copied for every run in it. Nothing depends on a firmware package.
fn wanted(name) { !name.contains("firmware") && !name.contains("ucode") }

// The APK files the root is made of, in order, as [file, lock row]: the
// base lock's from the `apks` input, then the apps-build lock's from the
// `apps_build_apks` input.
fn root_apks() {
    let files = [];
    for set in [[INPUTS.apks, lock::packages()], [INPUTS.apps_build_apks, build_lock::packages()]] {
        for row in set[1] { if wanted(row[0]) { files.push([join_path(set[0], basename(row[2])), row]); } }
    }
    files
}

fn apk_tools_file() { join_path(INPUTS.apks, basename(lock::apk_tools()[2])) }

fn verify_inputs(apks) {
    for entry in [[apk_tools_file(), lock::apk_tools()]] + apks {
        if !is_file(entry[0]) { throw "the apk inputs lack " + basename(entry[0]) + "; run the rootfs source preseed for the desktop"; }
        verify_sha256(entry[0], entry[1][3]);
    }
}

// What the tree must have to compile the apps, XLibre and reclogin: the first
// one missing, or "".
fn missing(root) {
    for path in ["usr/bin/g++", "usr/bin/ld", "usr/bin/make", "usr/bin/cmake", "bin/sh",
                 "usr/lib/cmake/Qt6Widgets/Qt6WidgetsConfig.cmake", "usr/lib/cmake/Qt6Svg/Qt6SvgConfig.cmake",
                 "usr/bin/meson", "usr/bin/samu", "usr/bin/git", "usr/bin/pkg-config",
                 "usr/lib/pkgconfig/xfont2.pc", "usr/lib/pkgconfig/epoxy.pc", "usr/lib/pkgconfig/libinput.pc",
                 // reclogin (reclogin.rhai): Rust, and the libraries it links
                 "usr/bin/cargo", "usr/bin/rustc", "usr/bin/cc",
                 "usr/lib/libpam.so", "usr/lib/libinput.so", "usr/lib/libxkbcommon.so"] {
        if !exists(join_path(root, path)) { return "/" + path; }
    }
    ""
}

fn build(src, out) {
    let apks = root_apks();
    verify_inputs(apks);
    let tools = join_path(WORK, "apk-tools");
    if is_dir(tools) { rm(tools); }
    mkdir(tools);
    extract_with_format(apk_tools_file(), tools, "apk");
    let apk = join_path(tools, "sbin/apk.static");
    if !is_file(apk) { throw "the locked apk-tools-static has no sbin/apk.static"; }
    chmod(apk, 0o755);

    // apk checks every package's signature against the release key.
    let key = lock::signing_key();
    let keys = join_path(out, "etc/apk/keys");
    mkdir(keys);
    write_file(join_path(keys, key[0]), key[1]);
    chmod(join_path(keys, key[0]), 0o644);
    let cache = join_path(WORK, "apk-cache");
    mkdir(cache);
    log("Installing the Alpine " + lock::release() + " compiler root...");
    let args = ["--root", out, "--usermode", "--initdb", "--no-progress", "--no-scripts",
        "--force-non-repository", "--no-network", "--cache-dir", cache, "add"];
    for entry in apks { args.push(entry[0]); }
    let total = apks.len();
    // APK 3 installs as a non-root user in usermode. One apk run installs
    // them all: Recipe sees none done until it ends.
    progress(0, total, "packages (one apk add)");
    exec_checked(apk, args, #{uid: 1000, gid: 1000});
    progress(total, total, "packages (one apk add)");
    // BusyBox's install script links its commands; no script ran, and the
    // build runs its commands through /bin/sh.
    if !exists(join_path(out, "bin/sh")) {
        if !is_file(join_path(out, "bin/busybox")) { throw "the root has no shell (/bin/busybox)"; }
        ln_force("busybox", join_path(out, "bin/sh"));
    }
}

fn verify(out) {
    let gap = missing(out);
    if gap != "" { throw "the installed root lacks " + gap + ": add the package that has it to desktop.apps_build_packages and lock again"; }
}
