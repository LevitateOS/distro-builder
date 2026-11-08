//! extends: rootfs-producer.rhai
// The APK backend of the rootfs producer: the locked Alpine APK set (the
// `apks` input), installed by the locked apk.static without the packages'
// scripts, so `package_effects` does what those scripts would have done.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ apks: "directory", capabilities: "file", system_setup: "file", os_release: "file" }, outputs: #{ rootfs: "directory" }, memory_mib: 4096, storage_max_objects: 131072 };
import "sources/acorn-apk-lock.rhai" as lock;
import "installed-system.rhai" as system;
let ctx = #{
    name: "alpine-live-source-rootfs",
    description: "The system rootfs from the locked Alpine " + lock::release() + " APK set",
    init: "openrc",
    apks_path: join_path(RUNTIME.paths.inputs, "apks"),
    packages: lock::packages(),
    apk_tools: lock::apk_tools(),
};

fn apk_file(ctx, row) { join_path(ctx.apks_path, basename(row[2])) }

fn verify_inputs(ctx) {
    for row in ctx.packages + [ctx.apk_tools] {
        let file = apk_file(ctx, row);
        if !is_file(file) { throw "the apks input lacks " + basename(file) + "; run the rootfs source preseed"; }
        verify_sha256(file, row[3]);
    }
}

fn unpack(ctx, root) {
    let build = ctx.runtime.paths.build;
    let tools = join_path(build, "apk-tools");
    if is_dir(tools) { rm(tools); }
    mkdir(tools);
    extract_with_format(apk_file(ctx, ctx.apk_tools), tools, "apk");
    let apk = join_path(tools, "sbin/apk.static");
    if !is_file(apk) { throw "the locked apk-tools-static has no sbin/apk.static"; }
    chmod(apk, 0o755);

    // apk checks every package's signature against the release key, where
    // alpine-keys keeps it.
    let key = lock::signing_key();
    system::put(root, "etc/apk/keys/" + key[0], key[1], 0o644);
    let cache = join_path(build, "apk-cache");
    mkdir(cache);
    log("Installing the locked Alpine " + lock::release() + " APK set...");
    let args = ["--root", root, "--usermode", "--initdb", "--no-progress", "--no-scripts",
        "--force-non-repository", "--no-network", "--cache-dir", cache, "add"];
    for row in ctx.packages { args.push(apk_file(ctx, row)); }
    // APK 3 installs as a non-root user in usermode; the packages' owners come
    // from their metadata.
    exec_checked(apk, args, #{uid: 1000, gid: 1000});
    target_capture_apks(root, [ctx.apks_path, cache]);
}

fn package_effects(ctx, root) {
    busybox_applets(root);
    // The accounts the packages' install scripts add (adduser -S): busybox's
    // klogd, chrony, dbus's messagebus, dhcpcd; NetworkManager's plugdev group.
    for account in [["klogd", "/dev/null"], ["chrony", "/dev/null"], ["messagebus", "/dev/null"],
                    ["dhcpcd", "/var/lib/dhcpcd"]] {
        system_account(root, account[0], account[1]);
    }
    system_group(root, "plugdev");
    // Password-authenticated doas for wheel administrators.
    system::put(root, "etc/doas.conf", "permit persist :wheel\n", 0o600);
    // The network stack ifupdown and dhcpcd bring up (installed-system.rhai
    // network_services): loopback here, dhcpcd for every other interface.
    system::put(root, "etc/network/interfaces",
        "# Installed loopback; dhcpcd discovers wired interfaces.\nauto lo\niface lo inet loopback\n", 0o644);
    system::put(root, "etc/conf.d/dhcpcd", "dhcpcd_args=\"--quiet\"\n", 0o644);
}

// The lowest free system id (adduser -S/addgroup -S: from 100).
fn free_id(root) {
    let used = [];
    for file in ["etc/passwd", "etc/group"] {
        for line in read_file(join_path(root, file)).split("\n") {
            let fields = line.split(":");
            if fields.len() >= 3 { used.push(parse_int(fields[2])); }
        }
    }
    let id = 100;
    while used.contains(id) { id += 1; }
    id
}

fn has_entry(root, file, name) {
    read_file(join_path(root, file)).split("\n").some(|line| line.starts_with(name + ":"))
}

fn append(root, file, line, mode) {
    let text = read_file_or_empty(join_path(root, file));
    if text != "" && !text.ends_with("\n") { text += "\n"; }
    system::put(root, file, text + line + "\n", mode);
}

fn system_group(root, name) {
    if has_entry(root, "etc/group", name) { return; }
    append(root, "etc/group", name + ":x:" + free_id(root) + ":", 0o644);
}

// A system user with its own group (the same id where both are new), a home
// and no login.
fn system_account(root, name, home) {
    if has_entry(root, "etc/passwd", name) { return; }
    system_group(root, name);
    let gid = 0;
    for line in read_file(join_path(root, "etc/group")).split("\n") {
        if line.starts_with(name + ":") { gid = parse_int(line.split(":")[2]); }
    }
    let gid_is_free_uid = !read_file(join_path(root, "etc/passwd")).split("\n")
        .some(|line| line.split(":").len() >= 3 && parse_int(line.split(":")[2]) == gid);
    let uid = if gid_is_free_uid { gid } else { free_id(root) };
    append(root, "etc/passwd", name + ":x:" + uid + ":" + gid + ":" + name + ":" + home + ":/sbin/nologin", 0o644);
    append(root, "etc/shadow", trim(system::shadow_line(name, "!")), 0o640);
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
