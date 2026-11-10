//! extends: rootfs-producer.rhai
// The APK backend of the rootfs producer: the locked Alpine APK set (the
// `apks` input), installed by the locked apk.static without the packages'
// scripts, so `package_effects` does what those scripts would have done. The
// desktop system (REC_FLAVOUR=desktop in the `os_release` input) also installs
// the desktop lock (the `desktop_apks` input) in the same apk run, so every
// step below and the installed-system policy run over base and desktop
// packages alike.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ apks: "directory", desktop_apks: "directory?", capabilities: "file", system_setup: "file", os_release: "file", admin_rule_file: "file", admin_rule: "file", display_manager: "file?", wallpaper_light: "file?", wallpaper_dark: "file?", logo_ink: "file?", logo_paper: "file?", logo_micro_ink: "file?", logo_micro_paper: "file?" }, outputs: #{ rootfs: "directory" }, memory_mib: 4096, storage_max_objects: 131072 };
import "sources/acorn-apk-lock.rhai" as lock;
import "sources/acorn-apk-desktop-lock.rhai" as desktop_lock;
import "installed-system.rhai" as system;
let ctx = #{
    name: "alpine-live-source-rootfs",
    description: "The system rootfs from the locked Alpine " + lock::release() + " APK set",
    init: "openrc",
    // [input, lock rows]: the base lock, and the desktop lock on the desktop system.
    sets: [["apks", lock::packages()]],
    apk_tools: lock::apk_tools(),
};
if system::flavour(RUNTIME.paths.inputs) == "desktop" {
    ctx.sets.push(["desktop_apks", desktop_lock::packages()]);
}

fn set_dir(ctx, set) { join_path(ctx.runtime.paths.inputs, set[0]) }
fn apk_file(ctx, set, row) { join_path(set_dir(ctx, set), basename(row[2])) }
// The names of the packages this tree installs.
fn installed_names(ctx) {
    let names = [];
    for set in ctx.sets { for row in set[1] { names.push(row[0]); } }
    names
}

fn verify_inputs(ctx) {
    let files = [[apk_file(ctx, ctx.sets[0], ctx.apk_tools), ctx.apk_tools]];
    for set in ctx.sets { for row in set[1] { files.push([apk_file(ctx, set, row), row]); } }
    for entry in files {
        let file = entry[0];
        if !is_file(file) { throw "the apk inputs lack " + basename(file) + "; run the rootfs source preseed"; }
        verify_sha256(file, entry[1][3]);
    }
}

fn unpack(ctx, root) {
    let build = ctx.runtime.paths.build;
    let tools = join_path(build, "apk-tools");
    if is_dir(tools) { rm(tools); }
    mkdir(tools);
    extract_with_format(apk_file(ctx, ctx.sets[0], ctx.apk_tools), tools, "apk");
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
    let dirs = [];
    for set in ctx.sets {
        dirs.push(set_dir(ctx, set));
        for row in set[1] { args.push(apk_file(ctx, set, row)); }
    }
    // APK 3 installs as a non-root user in usermode; the packages' owners come
    // from their metadata.
    exec_checked(apk, args, #{uid: 1000, gid: 1000});
    target_capture_apks(root, dirs + [cache]);
}

fn package_effects(ctx, root) {
    busybox_applets(root);
    // The accounts the packages' install scripts add (adduser -S): busybox's
    // klogd, chrony, dbus's messagebus; NetworkManager's plugdev group.
    for account in [["klogd", "/dev/null"], ["chrony", "/dev/null"], ["messagebus", "/dev/null"]] {
        system_account(root, account[0], account[1]);
    }
    system_group(root, "plugdev");
    // The desktop packages' accounts, where the tree has the package:
    // polkit's polkitd, sddm's sddm (in the video group, as its script makes
    // it) and pipewire's group.
    let names = installed_names(ctx);
    if names.contains("polkit-common") { system_account(root, "polkitd", "/var/empty"); }
    if names.contains("sddm") {
        system_group(root, "sddm");
        system_user(root, "sddm", "video", "/var/lib/sddm");
    }
    if names.contains("pipewire") { system_group(root, "pipewire"); }
    // Avahi's avahi (avahi-daemon drops to it), CUPS's lpadmin group (lp
    // comes with alpine-baselayout), libgphoto2's camera group, and SANE's
    // scanner group with the saned network scanning account in it.
    if names.contains("avahi") { system_account(root, "avahi", "/dev/null"); }
    if names.contains("cups") { system_group(root, "lpadmin"); }
    if names.contains("libgphoto2") { system_group(root, "camera"); }
    if names.contains("sane") { system_group(root, "scanner"); }
    if names.contains("sane-saned") { system_user(root, "saned", "scanner", "/run/saned"); }
    // pinentry's script points /usr/bin/pinentry at the curses one when no
    // graphical one is installed.
    if names.contains("pinentry") && !exists(join_path(root, "usr/bin/pinentry")) {
        system::link(root, "usr/bin/pinentry", "pinentry-curses");
    }
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

fn group_id(root, name) {
    for line in read_file(join_path(root, "etc/group")).split("\n") {
        if line.starts_with(name + ":") { return parse_int(line.split(":")[2]); }
    }
    throw "the rootfs has no group " + name;
}

// A system user with its own group (the same id where both are new), a home
// and no login.
fn system_account(root, name, home) {
    if has_entry(root, "etc/passwd", name) { return; }
    system_group(root, name);
    let gid = group_id(root, name);
    let gid_is_free_uid = !read_file(join_path(root, "etc/passwd")).split("\n")
        .some(|line| line.split(":").len() >= 3 && parse_int(line.split(":")[2]) == gid);
    let uid = if gid_is_free_uid { gid } else { free_id(root) };
    append(root, "etc/passwd", name + ":x:" + uid + ":" + gid + ":" + name + ":" + home + ":/sbin/nologin", 0o644);
    append(root, "etc/shadow", trim(system::shadow_line(name, "!")), 0o640);
}

// A system user in an existing group (adduser -S -G), a home and no login.
fn system_user(root, name, group, home) {
    if has_entry(root, "etc/passwd", name) { return; }
    let uid = free_id(root);
    append(root, "etc/passwd", name + ":x:" + uid + ":" + group_id(root, group) + "::" + home + ":/sbin/nologin", 0o644);
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
