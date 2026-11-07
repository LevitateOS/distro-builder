// The live session is a separate product. Never modify the installed rootfs.
const RECIPE_API = 2;
let config = parse_json(read_file("/inputs/config"));
let ctx = #{name: "live-overlay", config: config,
    root: join_path(RUNTIME.paths.output, "overlay")};

fn put(root, path, content, mode) {
    let dest = join_path(root, path);
    mkdir(dirname(dest)); write_file(dest, content); chmod(dest, mode);
}
fn link(root, path, target) {
    let dest = join_path(root, path);
    mkdir(dirname(dest)); ln_force(target, dest);
}
fn asset(name, config) {
    let text = read_file(join_path("/inputs/assets/live", name));
    text = text.replace("@OS_NAME@", config.os_name);
    text = text.replace("@SHELL_READY_MARKER@", config.shell_ready_marker);
    if text.contains("@OS_NAME@") { throw "unrendered OS name in live asset " + name; }
    if text.contains("@SHELL_READY_MARKER@") { throw "unrendered serial marker in live asset " + name; }
    text
}
fn openrc(root, config) {
    let inittab = "::sysinit:/sbin/openrc sysinit\n::sysinit:/sbin/openrc boot\n";
    for tty in ["tty1", "tty2", "tty3", "tty4", "tty5", "tty6"] {
        let login = if config.consoles.contains(tty) { "-n -l " + config.launcher + " " } else { "" };
        inittab += tty + "::respawn:/sbin/getty " + login + "38400 " + tty + " linux\n";
    }
    let serial = if config.consoles.contains("ttyS0") { config.launcher }
        else { "/usr/local/bin/serial-autologin" };
    inittab += "ttyS0::respawn:/sbin/getty -L -n -l " + serial + " 115200 ttyS0 vt100\n" +
        "::wait:/sbin/openrc default\n::ctrlaltdel:/sbin/reboot\n::shutdown:/sbin/openrc shutdown\n";
    put(root, "etc/inittab", inittab, 0o644);
    put(root, "usr/local/bin/serial-autologin", asset("serial-autologin.rootfs.sh", config), 0o755);
    put(root, "etc/fstab", "tmpfs /var/log tmpfs nosuid,nodev,noexec,size=64M,mode=0755 0 0\n", 0o644);
    put(root, "etc/local.d/01-efivarfs.start", asset("efivarfs.start", config), 0o755);
    put(root, "etc/sysctl.d/50-live-sysrq.conf", "kernel.sysrq = 1\n", 0o644);
    // QEMU-specific network defaults are live-only.
    put(root, "etc/network/interfaces", "auto lo\niface lo inet loopback\n\nauto eth0\niface eth0 inet dhcp\n", 0o644);
}
fn systemd(root, config) {
    let system = "etc/systemd/system/";
    let vt = "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --noclear %I $TERM\n";
    let serial = "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --keep-baud 115200,57600,38400,9600 %I vt100\n";
    put(root, system + "getty@.service.d/zz-autologin.conf", vt, 0o644);
    put(root, system + "serial-getty@.service.d/zz-autologin.conf", serial, 0o644);
    for console in config.consoles {
        let unit = if console == "ttyS0" { "serial-getty@" } else { "getty@" };
        let command = if console == "ttyS0" { serial } else { vt };
        command = command.replace(" %I", " --login-program " + config.launcher + " %I");
        put(root, system + unit + console + ".service.d/zz-autologin.conf", command, 0o644);
    }
    link(root, system + "getty.target.wants/getty@tty1.service", "/usr/lib/systemd/system/getty@.service");
    for target in ["basic.target.wants", "getty.target.wants"] {
        link(root, system + target + "/serial-getty@ttyS0.service", "/usr/lib/systemd/system/serial-getty@.service");
    }
    put(root, "usr/local/sbin/live-net-setup", asset("live-net-setup.sh", config), 0o755);
    put(root, system + "live-net-setup.service",
        "[Unit]\nDescription=Live network setup\nDefaultDependencies=no\nAfter=basic.target local-fs.target\nBefore=network.target network-online.target sshd.service\nWants=network.target\n\n" +
        "[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/usr/local/sbin/live-net-setup\n\n[Install]\nWantedBy=multi-user.target\n", 0o644);
    link(root, system + "multi-user.target.wants/live-net-setup.service", "/etc/systemd/system/live-net-setup.service");
    put(root, "usr/local/sbin/live-shutdown-cleanup", asset("live-shutdown-cleanup.sh", config), 0o755);
    put(root, system + "live-shutdown-cleanup.service",
        "[Unit]\nDescription=Live ISO shutdown cleanup\nDefaultDependencies=no\nAfter=multi-user.target\nBefore=run-live-media.mount umount.target shutdown.target\nConflicts=shutdown.target umount.target\n\n" +
        "[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/bin/true\nExecStop=/usr/local/sbin/live-shutdown-cleanup\n\n[Install]\nWantedBy=multi-user.target\n", 0o644);
    link(root, system + "multi-user.target.wants/live-shutdown-cleanup.service", "/etc/systemd/system/live-shutdown-cleanup.service");
    put(root, "etc/tmpfiles.d/sshd-local.conf", "d /run/sshd 0755 root root -\n", 0o644);
    put(root, "etc/machine-id", "", 0o444);
}
fn validate_service(service) {
    if service == "" { throw "empty live service name"; }
    for ch in service.chars() {
        let allowed = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.@".contains(ch);
        if !allowed { throw "invalid live service name"; }
    }
}
fn services(root, c) {
    for service in c.services {
        validate_service(service);
        if c.init == "systemd" {
            let unit = service;
            if !service.ends_with(".service") { unit += ".service"; }
            link(root, "etc/systemd/system/multi-user.target.wants/" + unit, "/usr/lib/systemd/system/" + unit);
        } else {
            if !["sshd", "networking", "dhcpcd"].contains(service) { throw "unsupported live OpenRC service " + service; }
            let level = if service == "networking" { "boot" } else { "default" };
            link(root, "etc/runlevels/" + level + "/" + service, "/etc/init.d/" + service);
        }
    }
}

fn is_built(ctx) { ready(ctx) }
fn is_installed(ctx) {
    if !is_file(join_path(ctx.root, ".recipe-live-overlay")) {
        return needs_work(ctx, "live overlay has not been prepared");
    }
    ctx.outputs = #{overlay: ctx.root}; ready(ctx)
}
fn install(ctx) {
    let root = ctx.root;
    if exists(root) { rm(root); }
    mkdir(root);
    let c = ctx.config;
    if c.init != "systemd" && c.init != "openrc" { throw "unsupported live init system"; }
    for console in c.consoles {
        let valid = ["tty1", "ttyS0"];
        if c.init == "openrc" { valid = ["tty1", "tty2", "tty3", "tty4", "tty5", "tty6", "ttyS0"]; }
        if !valid.contains(console) { throw "unsupported install console " + console; }
    }
    put(root, "etc/issue", c.issue, 0o644);
    // Keep package system accounts; override only root for the live session.
    let shadow = "";
    for line in read_file("/inputs/shadow").split("\n") {
        if line.starts_with("root:") { shadow += "root::20000:0:99999:7:::\n"; }
        else if line != "" { shadow += line + "\n"; }
    }
    put(root, "etc/shadow", shadow, 0o600);
    put(root, c.role_file, c.role + "\n", 0o644);
    put(root, "etc/profile.d/20-live-shell.sh", asset("shell-ux.sh", c), 0o755);
    put(root, "etc/profile.d/99-serial-test.sh", read_file("/inputs/serial_profile"), 0o644);
    put(root, "usr/local/bin/live-help", asset("live-help.sh", c), 0o755);
    if c.init == "openrc" { openrc(root, c); } else { systemd(root, c); }
    services(root, c);
    put(root, ".recipe-live-overlay", "1\n", 0o644);
    ctx.outputs = #{overlay: root}; ctx
}
fn remove(ctx) { if exists(ctx.root) { rm(ctx.root); } ctx.outputs = #{}; ctx }
