// The installed-system policy, and the init backend it shares with the live
// overlay (`live-overlay.rhai` imports this module). The policy states each
// feature once; the backend functions, keyed on the init system ("systemd" or
// "openrc"), supply only the on-disk mechanism. This tree is also what
// recstrap installs and recab deploys.

// ---------------------------------------------------------------------------
// Files. Every mode is recorded in Recipe's target metadata.
// ---------------------------------------------------------------------------

fn put(root, path, content, mode) {
    let dest = join_path(root, path);
    mkdir(dirname(dest));
    write_file(dest, content);
    target_mode(root, path, mode);
}
fn link(root, path, target) {
    let dest = join_path(root, path);
    mkdir(dirname(dest));
    ln_force(target, dest);
}
fn directory(root, path, mode) {
    mkdir(join_path(root, path));
    target_mode(root, path, mode);
}
// One account's shadow line; the aging fields are the same for every account.
fn shadow_line(name, password) { name + ":" + password + ":20000:0:99999:7:::\n" }
// `shadow` (the file's text) with root's password replaced.
fn root_password(shadow, password) {
    let result = "";
    let found = false;
    for line in shadow.split("\n") {
        if line.starts_with("root:") { result += shadow_line("root", password); found = true; }
        else if line != "" { result += line + "\n"; }
    }
    if !found { throw "the rootfs has no root shadow entry"; }
    result
}

// ---------------------------------------------------------------------------
// Init backend.
// ---------------------------------------------------------------------------

fn check_init(init) {
    if init != "systemd" && init != "openrc" { throw "unsupported init system: " + init; }
}

// The OpenRC runlevel a service belongs in: sysinit mounts /dev, /dev/pts
// and /dev/shm, then udev initializes devices for NetworkManager. The boot
// runlevel sets up the system, and shutdown stops it cleanly.
// Every other service goes in default.
fn openrc_runlevel(service) {
    let levels = #{
        devfs: "sysinit", dmesg: "sysinit", udev: "sysinit", "udev-trigger": "sysinit",
        "udev-settle": "sysinit",
        modules: "boot", sysctl: "boot", hostname: "boot", bootmisc: "boot", loopback: "boot", syslog: "boot",
        hwclock: "boot", seedrng: "boot", loadkeys: "boot",
        "mount-ro": "shutdown", killprocs: "shutdown", savecache: "shutdown"
    };
    let level = levels[service];
    if type_of(level) == "()" { "default" } else { level }
}

// Where the init keeps a service's definition, relative to the root.
fn service_file(init, service) {
    check_init(init);
    if init == "systemd" {
        if service.ends_with(".service") { "usr/lib/systemd/system/" + service }
        else { "usr/lib/systemd/system/" + service + ".service" }
    } else { "etc/init.d/" + service }
}

// Start `service` at boot.
fn enable(root, init, service) {
    let file = "/" + service_file(init, service);
    if init == "systemd" {
        link(root, "etc/systemd/system/multi-user.target.wants/" + basename(file), file);
    } else {
        link(root, "etc/runlevels/" + openrc_runlevel(service) + "/" + service, file);
    }
}

// The init system's own boot services. systemd brings up `lo` itself; OpenRC's
// `loopback` does (NetworkManager leaves lo alone), so it owns loopback there.
fn base_services(init) {
    check_init(init);
    if init == "systemd" { [] }
    else { ["devfs", "dmesg", "udev", "udev-trigger", "udev-settle", "modules", "sysctl", "hostname", "bootmisc", "loopback",
            "syslog", "hwclock", "seedrng", "mount-ro", "killprocs", "savecache"] }
}

// The service that brings up every network interface, wired and WiFi:
// NetworkManager (with wpa_supplicant for WiFi) on both distros. OpenRC also
// runs dbus (capability_services), which NetworkManager needs.
fn network_services(init) {
    check_init(init);
    if init == "systemd" { ["NetworkManager"] } else { ["networkmanager"] }
}

// What every system runs, by its service names: time sync (chrony), the
// audit daemon and the D-Bus system bus. systemd starts D-Bus from its socket,
// which the RPM backend links. sshd is installed but starts only when the
// user enables it.
fn capability_services(init) {
    check_init(init);
    if init == "systemd" { ["chronyd", "auditd"] } else { ["chronyd", "auditd", "dbus"] }
}

// The boot target: systemd's default.target. Busybox init's inittab (written
// by `consoles`) names OpenRC's runlevels itself.
fn boot_target(root, init) {
    check_init(init);
    if init == "systemd" {
        link(root, "etc/systemd/system/default.target", "/usr/lib/systemd/system/multi-user.target");
    }
}

// Boot to the graphical desktop: start the display manager and, on systemd,
// make graphical.target the default. OpenRC has no target; its display manager
// starts in the default runlevel after dbus and elogind.
fn graphical(root, init, display_manager) {
    enable(root, init, display_manager);
    if init == "systemd" {
        link(root, "etc/systemd/system/default.target", "/usr/lib/systemd/system/graphical.target");
    }
}

// The login on each console. `gettys` is a list of [console, login]: login ""
// is a login prompt, "root" logs root in without asking, and a path runs that
// program as root in place of login. A serial login prompt runs only when
// the boot names the port as a console: systemd's getty generator starts it
// then; busybox init's line runs `serial-login`, which checks the same.
fn consoles(root, init, gettys) {
    check_init(init);
    if init == "systemd" {
        for getty in gettys { systemd_getty(root, getty[0], getty[1]); }
    } else {
        openrc_inittab(root, gettys);
    }
}
fn systemd_getty(root, console, login) {
    let serial = console.starts_with("ttyS");
    if serial && login == "" { return; }
    let unit = if serial { "serial-getty@" } else { "getty@" };
    let instance = "etc/systemd/system/" + unit + console + ".service";
    link(root, "etc/systemd/system/getty.target.wants/" + unit + console + ".service",
        "/usr/lib/systemd/system/" + unit + ".service");
    if login == "" { return; }
    let agetty = "-/sbin/agetty --autologin root";
    if login != "root" { agetty += " --login-program " + login; }
    if serial { agetty += " --keep-baud 115200,57600,38400,9600 %I vt100"; }
    else { agetty += " --noclear %I $TERM"; }
    put(root, instance + ".d/zz-autologin.conf", "[Service]\nExecStart=\nExecStart=" + agetty + "\n", 0o644);
}
// Busybox init's inittab: OpenRC's runlevels in order, and one getty per console.
fn openrc_inittab(root, gettys) {
    let inittab = "::sysinit:/sbin/openrc sysinit\n::sysinit:/sbin/openrc boot\n";
    let root_login = "usr/local/libexec/root-login";
    let serial_login = "usr/local/libexec/serial-login";
    for getty in gettys {
        let console = getty[0];
        let program = getty[1];
        if program == "root" { program = "/" + root_login; }
        if console.starts_with("ttyS") && program == "" {
            inittab += console + "::respawn:/" + serial_login + " " + console + "\n";
            continue;
        }
        let line = console + "::respawn:/sbin/getty ";
        if console.starts_with("ttyS") { line += "-L "; }
        if program != "" { line += "-n -l " + program + " "; }
        if console.starts_with("ttyS") { line += "115200 " + console + " vt100\n"; }
        else { line += "38400 " + console + " linux\n"; }
        inittab += line;
    }
    inittab += "::wait:/sbin/openrc default\n::ctrlaltdel:/sbin/reboot\n::shutdown:/sbin/openrc shutdown\n";
    put(root, "etc/inittab", inittab, 0o644);
    if inittab.contains("/" + root_login + " ") {
        put(root, root_login,
            "#!/bin/sh\n# getty runs this in place of login where root logs in without asking.\nexec /bin/login -f root\n",
            0o755);
    }
    if inittab.contains("/" + serial_login + " ") {
        let script = "#!/bin/sh\n# A login prompt on serial port $1 when the boot names it as a console.\n";
        script += "for arg in $(cat /proc/cmdline); do\n";
        script += "    case \"$arg\" in console=\"$1\"|console=\"$1\",*) exec /sbin/getty -L 115200 \"$1\" vt100 ;; esac\n";
        script += "done\nexec sleep 2147483647\n";
        put(root, serial_login, script, 0o755);
    }
}

// Login shells get LANG from etc/locale.conf. systemd reads the file itself.
fn locale_env(root, init) {
    check_init(init);
    if init == "openrc" {
        put(root, "etc/profile.d/locale.sh",
            "[ -r /etc/locale.conf ] && . /etc/locale.conf && export LANG\n", 0o644);
    }
}

// sshd's runtime directory /run/sshd. OpenRC's sshd service creates it.
fn sshd_runtime_dir(root, init) {
    check_init(init);
    if init == "systemd" {
        put(root, "etc/tmpfiles.d/sshd-local.conf", "d /run/sshd 0755 root root -\n", 0o644);
    }
}

// Run `script` (the text of a shell script) at shutdown, before the init
// unmounts filesystems and before `mount` (a mount point) is unmounted.
fn shutdown_hook(root, init, name, script, mount) {
    check_init(init);
    if init == "systemd" {
        let program = "usr/local/sbin/" + name;
        put(root, program, script, 0o755);
        // The mount's unit name: the path with "-" escaped and "/" as "-".
        let mount_unit = mount.sub_string(1).replace("-", "\\x2d").replace("/", "-") + ".mount";
        // Stopping runs in reverse order: after the mount in start order is
        // before it at shutdown.
        let unit = "[Unit]\nDescription=" + name + "\nDefaultDependencies=no\n";
        unit += "After=multi-user.target " + mount_unit + "\n";
        unit += "Before=umount.target shutdown.target\nConflicts=shutdown.target umount.target\n\n";
        unit += "[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/bin/true\n";
        unit += "ExecStop=/" + program + "\n";
        put(root, "etc/systemd/system/" + name + ".service", unit, 0o644);
        link(root, "etc/systemd/system/multi-user.target.wants/" + name + ".service",
            "/etc/systemd/system/" + name + ".service");
    } else {
        // The local service runs local.d's *.stop scripts when it stops, which
        // is first at shutdown (it starts after every other service).
        put(root, "etc/local.d/" + name + ".stop", script, 0o755);
        enable(root, init, "local");
    }
}

// ---------------------------------------------------------------------------
// Installed-system policy.
// ---------------------------------------------------------------------------

// Every console the system has: six virtual consoles and the serial console.
fn all_consoles() { ["tty1", "tty2", "tty3", "tty4", "tty5", "tty6", "ttyS0"] }

// The system's identity and distro profile (the REC_* fields the install
// tools read), from the contract: the builder's `os_release` input. A packaged
// /etc/os-release keeps its packaged form and mode (Fedora's is a symlink to
// usr/lib/os-release, which then holds this text).
fn identity(root, inputs) {
    let text = read_file(join_path(inputs, "os_release"));
    let path = join_path(root, "etc/os-release");
    if exists(path) { write_file(path, text); } else { put(root, "etc/os-release", text, 0o644); }
}

// The OS's PRETTY_NAME from the builder's `os_release` input, for the login banner.
fn pretty_name(inputs) {
    for line in read_file(join_path(inputs, "os_release")).split("\n") {
        if line.starts_with("PRETTY_NAME=") {
            let name = line.sub_string(12);
            name.replace("\"", "");
            return name;
        }
    }
    throw "the os_release input has no PRETTY_NAME";
}

// The setup program reads the distro profile from /etc/os-release, so it is
// installed as the builder rendered it (its `system_setup` input).
fn setup_program(root, inputs) {
    put(root, "usr/libexec/recipe-system-setup", read_file(join_path(inputs, "system_setup")), 0o755);
}

// The wheel administrators' password-authenticated privilege rule, in the file
// the distro profile names (the builder's `admin_rule_file` and `admin_rule`
// inputs).
fn admin_rule(root, inputs) {
    put(root, read_file(join_path(inputs, "admin_rule_file")).trim(),
        read_file(join_path(inputs, "admin_rule")).trim() + "\n", 0o440);
}

// Root's login shell is bash, which recsetup also gives new users, and
// /etc/shells lists it (package scripts that would add it do not run).
fn bash_login(root) {
    let passwd = "";
    for line in read_file(join_path(root, "etc/passwd")).split("\n") {
        if line == "" { continue; }
        if line.starts_with("root:") {
            let fields = line.split(":");
            if fields.len() != 7 { throw "malformed root passwd entry: " + line; }
            fields[6] = "/bin/bash";
            line = fields[0];
            for i in 1..7 { line += ":" + fields[i]; }
        }
        passwd += line + "\n";
    }
    put(root, "etc/passwd", passwd, 0o644);
    let shells = read_file_or_empty(join_path(root, "etc/shells"));
    if !shells.split("\n").contains("/bin/bash") {
        if shells != "" && !shells.ends_with("\n") { shells += "\n"; }
        put(root, "etc/shells", shells + "/bin/bash\n", 0o644);
    }
}

// Home directories live under /var, which A/B installs mount from the shared
// STATE partition, so an update (which replaces the slot, not /var) keeps
// them. The image's /home is a symlink to var/home; accounts keep /home/<name>
// as their home, which resolves through it. /var/home is created here for
// mutable installs (/var on the root) and by recsetup for A/B installs.
fn homes(root) {
    rm(join_path(root, "home"));
    // Drop the package's directory metadata before replacing it with a link.
    target_finalize(root);
    link(root, "home", "var/home");
    directory(root, "var/home", 0o755);
}

// Release images have no machine password (personalization supplies it) and
// no machine identity (the first boot or recsetup makes one).
fn lock_root(root) {
    put(root, "etc/shadow", root_password(read_file(join_path(root, "etc/shadow")), "!"), 0o600);
    put(root, "etc/machine-id", "", 0o444);
}

// Apply the installed-system policy to the tree at `root`, whose init is `init`.
fn apply(root, inputs, init) {
    check_init(init);
    identity(root, inputs);
    put(root, "etc/issue", pretty_name(inputs) + " \\l\n", 0o644);
    put(root, "etc/locale.conf", "LANG=C.UTF-8\n", 0o644);
    locale_env(root, init);
    // sshd's privilege-separation directories.
    for path in ["var/empty/sshd", "usr/share/empty.sshd"] { directory(root, path, 0o755); }
    let gettys = [];
    for console in all_consoles() { gettys.push([console, ""]); }
    consoles(root, init, gettys);
    boot_target(root, init);
    for service in base_services(init) + network_services(init) + capability_services(init) {
        if !is_file(join_path(root, service_file(init, service))) {
            throw "installed system lacks the " + init + " service " + service;
        }
        enable(root, init, service);
    }
    bash_login(root);
    homes(root);
    lock_root(root);
    setup_program(root, inputs);
    admin_rule(root, inputs);
    if init == "openrc" {
        // D-Bus cannot initialize an existing empty machine-id, and /var/log
        // may be a fresh live tmpfs or the installed system's shared state.
        put(root, "etc/init.d/recipe-system-setup",
            "#!/sbin/openrc-run\ndescription=\"Initialize machine identity and runtime directories\"\n\ndepend() {\n    need localmount\n    before dbus auditd\n}\n\nstart() {\n    /usr/libexec/recipe-system-setup machine-id || return 1\n    checkpath -d -m 0700 -o root:root /var/log/audit\n}\n", 0o755);
        enable(root, init, "recipe-system-setup");
    }
}
