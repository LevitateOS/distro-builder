//! extends: rootfs-producer.rhai
// The APK backend of the rootfs producer: Alpine packages from the pinned ISO
// and the pinned extra APKs (the `iso`, `apk_tools` and `extra_apks`
// inputs), installed by apk.static without their scripts, so
// `package_effects` does what those scripts would have done.
const RECIPE_API = 2;
import "sources/alpine.rhai" as source;
import "installed-system.rhai" as system;
let assets = source::facts();
let ctx = #{
    name: "alpine-live-source-rootfs",
    description: "The system rootfs from Alpine " + assets.iso_version + " packages",
    init: "openrc",
    apk_tools_path: join_path(RUNTIME.paths.inputs, "apk_tools"),
    apk_tools_sha256: assets.apk_tools_sha256,
    iso_path: join_path(RUNTIME.paths.inputs, "iso"),
    iso_sha256: assets.iso_sha256,
    extra_apks_path: join_path(RUNTIME.paths.inputs, "extra_apks"),
    extra_apks: assets.extra_apks,
    mirror: assets.mirror,
};

// The packages the rootfs installs from the ISO; the pinned extra APKs come
// beside them.
fn packages() {
    ["alpine-base", "busybox", "coreutils", "openrc", "doas", "openssh", "openssh-server",
     "util-linux", "e2fsprogs", "dosfstools", "iproute2", "iputils", "dhcpcd", "curl",
     "pciutils", "htop", "tzdata", "kbd-bkeymaps", "wpa_supplicant", "wpa_supplicant-openrc", "iw"]
}

fn verify_inputs(ctx) {
    if !is_file(ctx.iso_path) || !is_file(ctx.apk_tools_path) {
        throw "the iso and apk_tools inputs are missing; run the rootfs source preseed";
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    for apk in ctx.extra_apks {
        let file = join_path(ctx.extra_apks_path, apk[0]);
        if !is_file(file) { throw "the extra_apks input lacks " + apk[0] + "; run the rootfs source preseed"; }
        verify_sha256(file, apk[1]);
    }
}

fn unpack(ctx, root) {
    let build = ctx.runtime.paths.build;
    let tools = join_path(build, "apk-tools");
    if is_dir(tools) { rm(tools); }
    mkdir(tools);
    extract_with_format(ctx.apk_tools_path, tools, "apk");
    let apk = join_path(tools, "sbin/apk.static");
    if !is_file(apk) { throw "the pinned apk-tools-static has no sbin/apk.static"; }
    chmod(apk, 0o755);

    let iso = join_path(build, "iso-contents");
    if is_dir(iso) { rm(iso); }
    mkdir(iso);
    log("Extracting the Alpine ISO...");
    // An immutable ISO file, not a writable optical drive.
    exec_checked("xorriso", ["-drive_access", "shared:readonly", "-osirrox", "on",
        "-indev", "stdio:" + ctx.iso_path, "-extract", "/", iso + "/"]);
    let apks = join_path(iso, "apks");
    if !is_dir(apks) { throw "the Alpine ISO has no apks directory"; }

    system::put(root, "etc/apk/repositories", ctx.mirror + "/main\n" + ctx.mirror + "/community\n", 0o644);
    mkdir(join_path(root, "etc/apk/keys"));
    let cache = join_path(build, "apk-cache");
    mkdir(cache);
    log("Installing the Alpine packages...");
    let args = ["--root", root, "--usermode", "--initdb", "--no-progress", "--no-scripts",
        "--allow-untrusted", "--force-non-repository", "--no-network", "--repository", apks,
        "--cache-dir", cache, "add"];
    args += packages();
    for extra in ctx.extra_apks { args.push(join_path(ctx.extra_apks_path, extra[0])); }
    // APK 3 installs as a non-root user in usermode; the packages' owners come
    // from their metadata.
    exec_checked(apk, args, #{uid: 1000, gid: 1000});
    target_capture_apks(root, [apks, ctx.extra_apks_path, cache]);
}

fn package_effects(ctx, root) {
    busybox_applets(root);
    // Password-authenticated doas for wheel administrators.
    system::put(root, "etc/doas.conf", "permit persist :wheel\n", 0o600);
    // The network stack ifupdown and dhcpcd bring up (installed-system.rhai
    // network_services): loopback here, dhcpcd for every other interface.
    system::put(root, "etc/network/interfaces",
        "# Installed loopback; dhcpcd discovers wired interfaces.\nauto lo\niface lo inet loopback\n", 0o644);
    system::put(root, "etc/conf.d/dhcpcd", "dhcpcd_args=\"--quiet\"\n", 0o644);
}

// BusyBox's applet links, from its own packaged inventory; files the
// full-size tools ship stay.
fn busybox_applets(root) {
    let privileged = ["bin/mount", "bin/umount", "bin/su", "usr/bin/crontab",
        "usr/bin/passwd", "usr/bin/traceroute", "usr/bin/traceroute6", "usr/bin/vlock"];
    for path in read_file(join_path(root, "etc/busybox-paths.d/busybox")).split("\n") {
        path = trim(path);
        if path == "" { continue; }
        if !(path.starts_with("bin/") || path.starts_with("sbin/") ||
             path.starts_with("usr/bin/") || path.starts_with("usr/sbin/")) ||
             path.contains("..") {
            throw "invalid packaged BusyBox applet path: " + path;
        }
        let dest = join_path(root, path);
        if exists(dest) { continue; }
        let program = if privileged.contains(path) { "bbsuid" } else { "busybox" };
        if !is_file(join_path(root, "bin/" + program)) { throw "missing BusyBox provider: " + program; }
        mkdir(dirname(dest));
        let prefix = if path.starts_with("usr/") { "../../bin/" }
                     else if path.starts_with("sbin/") { "../bin/" } else { "" };
        ln_force(prefix + program, dest);
    }
}
