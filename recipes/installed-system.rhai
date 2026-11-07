// Installed-system policy shared by the source producers. Live-session policy
// belongs to the live overlay; this tree is also deployed by recstrap/recab.
fn put(root, path, content, mode) {
    let dest = join_path(root, path);
    mkdir(dirname(dest));
    write_file(dest, content);
    chmod(dest, mode);
}

fn setup_program(root, init, libc) {
    let script = read_file("/inputs/system_setup");
    script = script.replace("@INIT@", init);
    script = script.replace("@LIBC@", libc);
    if script.contains("@INIT@") { throw "unrendered setup init system"; }
    if script.contains("@LIBC@") { throw "unrendered setup libc"; }
    put(root, "usr/libexec/recipe-system-setup", script, 0o755);
}

fn lock_root(root) {
    let path = join_path(root, "etc/shadow");
    let lines = read_file(path).split("\n");
    let result = "";
    let found = false;
    for line in lines {
        if line.starts_with("root:") {
            // Release images have no machine password. Personalization supplies it.
            result += "root:!:20000:0:99999:7:::\n";
            found = true;
        } else if line != "" { result += line + "\n"; }
    }
    if !found { throw "installed rootfs has no root shadow entry"; }
    put(root, "etc/shadow", result, 0o600);
    put(root, "etc/machine-id", "", 0o444);
}

fn systemd(root) {
    put(root, "etc/issue", "Linux \\l\n", 0o644);
    mkdir(join_path(root, "etc/systemd/system"));
    ln_force("/usr/lib/systemd/system/multi-user.target",
        join_path(root, "etc/systemd/system/default.target"));
    for path in ["var/empty/sshd", "usr/share/empty.sshd"] {
        mkdir(join_path(root, path));
        chmod(join_path(root, path), 0o755);
    }
    let config = join_path(root, "etc/ssh/sshd_config");
    if !is_file(config) {
        let fallback = join_path(root, "etc/ssh/sshd_config.anaconda");
        if is_file(fallback) { copy_file(fallback, config); }
        else { put(root, "etc/ssh/sshd_config", "UsePAM yes\nInclude /etc/ssh/sshd_config.d/*.conf\n", 0o600); }
    }
    let has_locale = false;
    for path in ["lib/locale/C.utf8/LC_CTYPE", "usr/lib/locale/C.utf8/LC_CTYPE",
                 "lib64/locale/C.utf8/LC_CTYPE", "usr/lib64/locale/C.utf8/LC_CTYPE"] {
        if is_file(join_path(root, path)) { has_locale = true; }
    }
    if !has_locale { throw "installed system lacks the C.UTF-8 locale payload"; }
    if is_dir(join_path(root, "lib/locale")) && !exists(join_path(root, "usr/lib/locale")) {
        ln_force("/lib/locale", join_path(root, "usr/lib/locale"));
    }
    put(root, "etc/locale.conf", "LANG=C.UTF-8\n", 0o644);
    mkdir(join_path(root, "etc/systemd/system/multi-user.target.wants"));
    let network = join_path(root, "usr/lib/systemd/system/NetworkManager.service");
    if !is_file(network) { throw "installed system lacks NetworkManager.service"; }
    ln_force("/usr/lib/systemd/system/NetworkManager.service",
        join_path(root, "etc/systemd/system/multi-user.target.wants/NetworkManager.service"));
    lock_root(root);
    setup_program(root, "systemd", "glibc");
}

fn openrc(root) {
    // Busybox init reads this in the installed system. No alternate login program.
    let inittab = "::sysinit:/sbin/openrc sysinit\n::sysinit:/sbin/openrc boot\n";
    for tty in ["tty1", "tty2", "tty3", "tty4", "tty5", "tty6"] {
        inittab += tty + "::respawn:/sbin/getty 38400 " + tty + "\n";
    }
    inittab += "ttyS0::respawn:/sbin/getty -L 115200 ttyS0 vt100\n" +
        "::wait:/sbin/openrc default\n::ctrlaltdel:/sbin/reboot\n::shutdown:/sbin/openrc shutdown\n";
    put(root, "etc/inittab", inittab, 0o644);
    put(root, "etc/issue", "Linux \\l\n", 0o644);
    put(root, "etc/profile.d/locale.sh",
        "[ -r /etc/locale.conf ] && . /etc/locale.conf && export LANG\n", 0o644);
    for service in ["networking", "dhcpcd"] {
        if !is_file(join_path(root, "etc/init.d/" + service)) {
            throw "installed system lacks OpenRC service " + service;
        }
        let level = if service == "networking" { "boot" } else { "default" };
        let dir = join_path(root, "etc/runlevels/" + level);
        mkdir(dir);
        ln_force("/etc/init.d/" + service, join_path(dir, service));
    }
    lock_root(root);
    setup_program(root, "openrc", "musl");
}
