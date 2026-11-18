// The installed-system policy, and the init backend it shares with the live
// overlay (`live-overlay.rhai` imports this module). The policy states each
// feature once; the backend functions, keyed on the init system ("systemd" or
// "openrc"), supply only the on-disk mechanism. This tree is also what
// recstrap installs and recab deploys.

// ---------------------------------------------------------------------------
// Files. Every mode is recorded in Recipe's target metadata. `put` is the
// engine's. link, directory and shadow_line have copies in `system-files.rhai` (the
// package-effects step's); change both together.
// ---------------------------------------------------------------------------

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
// and /dev/shm, then udev initializes devices for the services after it. The boot
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
// `loopback` does (NetworkManager, when the `network` group is installed, leaves
// lo alone), so it owns loopback there.
fn base_services(init) {
    check_init(init);
    if init == "systemd" { [] }
    else { ["devfs", "dmesg", "udev", "udev-trigger", "udev-settle", "modules", "sysctl", "hostname", "bootmisc", "loopback",
            "syslog", "hwclock", "seedrng", "mount-ro", "killprocs", "savecache"] }
}

// What every system runs, by its service names: time sync (chrony), the
// audit daemon and the D-Bus system bus. systemd starts D-Bus from its socket,
// which the RPM backend links. On OpenRC also BusyBox's crond, which runs
// root's crontab and with it /etc/periodic (the daily release notice,
// `recupdate-notice`, is a periodic job there; systemd runs it from a timer).
// sshd is installed but starts only when the user enables it.
fn capability_services(init) {
    check_init(init);
    if init == "systemd" { ["chronyd", "auditd"] } else { ["chronyd", "auditd", "dbus", "crond"] }
}

// The boot target: systemd's graphical.target, whatever software the system
// gets (without a display manager it is multi-user.target with nothing more;
// a desktop group recipe adds its display manager's display-manager.service
// alias, which graphical.target wants). Busybox init's inittab (written by
// `consoles`) names OpenRC's runlevels itself.
fn boot_target(root, init) {
    check_init(init);
    if init == "systemd" {
        link(root, "etc/systemd/system/default.target", "/usr/lib/systemd/system/graphical.target");
    }
}

// The seat and session services any login needs: systemd's logind is part of
// systemd and polkit starts on D-Bus demand; OpenRC runs elogind (seats and
// sessions) and polkit as services.
fn seat_services(init) {
    check_init(init);
    if init == "systemd" { [] } else { ["elogind", "polkit"] }
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
// Busybox init's inittab: OpenRC's runlevels in order, and one getty per
// console.
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
// Installed-system policy: the base system. What a group recipe installs
// (recipes/, through Recipe) carries its own configuration; nothing here
// configures software above the base.
// ---------------------------------------------------------------------------

// Every console the system has: six virtual consoles and the serial console.
// Each has a login, on every system: a display manager a desktop group
// installs starts on a virtual console of its own.
fn all_consoles() { ["tty1", "tty2", "tty3", "tty4", "tty5", "tty6", "ttyS0"] }

// The value of `field` in the builder's `os_release` input (the REC_* fields
// are unquoted words).
fn os_release_field(field) {
    for line in read_file(INPUTS.os_release).split("\n") {
        if line.starts_with(field + "=") { return trim(line.sub_string(field.len() + 1)); }
    }
    throw "the os_release input has no " + field;
}

// The system's identity and distro profile (the REC_* fields the install
// tools read), from the contract: the builder's `os_release` input. The text
// goes to usr/lib/os-release and etc/os-release links to it, as os-release(5)
// lays out. LevitateOS has no packaged os-release (system-release is
// provided_elsewhere); AcornOS's packaged one has this same shape.
fn identity(root) {
    put(root, "usr/lib/os-release", read_file(INPUTS.os_release), 0o644);
    link(root, "etc/os-release", "../usr/lib/os-release");
}

// The OS's PRETTY_NAME from the builder's `os_release` input, for the login banner.
fn pretty_name() {
    for line in read_file(INPUTS.os_release).split("\n") {
        if line.starts_with("PRETTY_NAME=") {
            let name = line.sub_string(12);
            name = name.replace("\"", "");
            return name;
        }
    }
    throw "the os_release input has no PRETTY_NAME";
}

// The setup program reads the distro profile from /etc/os-release, so it is
// installed as the builder rendered it (its `system_setup` input).
fn setup_program(root) {
    put(root, "usr/libexec/recipe-system-setup", read_file(INPUTS.system_setup), 0o755);
}

// The root effects recipe (the builder's `root_effects` input,
// os-recipes/root-effects.rhai): Recipe runs it after every system change to
// this root and writes what it makes as the reserved package system-effects
// (the whole-root caches package scripts would have rebuilt).
fn root_effects(root) {
    put(root, "usr/lib/recipe/effects.rhai", read_file(INPUTS.root_effects), 0o644);
}

// The wheel administrators' password-authenticated privilege rule, in the file
// the distro profile names (the builder's `admin_rule_file` and `admin_rule`
// inputs).
fn admin_rule(root) {
    put(root, read_file(INPUTS.admin_rule_file).trim(),
        read_file(INPUTS.admin_rule).trim() + "\n", 0o440);
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
// STATE partition, so a change made in the other root (which replaces the
// root, not /var) keeps them. The image's /home is a symlink to var/home; accounts keep /home/<name>
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

// No hibernation: recinit's initramfs does not resume from a hibernation
// image, so logind (systemd) or elogind (OpenRC) does not offer it, and the
// desktops hide it. Suspend and lid close keep their defaults (suspend).
fn no_hibernation(root, init) {
    check_init(init);
    let dir = if init == "systemd" { "etc/systemd/sleep.conf.d/" } else { "etc/elogind/sleep.conf.d/" };
    put(root, dir + "10-no-hibernation.conf",
        "[Sleep]\nAllowHibernation=no\nAllowSuspendThenHibernate=no\nAllowHybridSleep=no\n", 0o644);
}

// Our QEMU guest agent, recguest (a rootfs tool the builder adds to every
// system), starts only when the machine has its virtio port: a systemd unit
// with that condition, and on OpenRC the `vm-guest-agents` wrapper, so real
// hardware shows no failed service. (spice-vdagent is the vm-guest group's.)
fn guest_agent(root, init) {
    check_init(init);
    if init == "openrc" {
        let agents = "#!/sbin/openrc-run\ndescription=\"Start the VM guest agent when the machine is a VM with its port\"\n\n";
        agents += "depend() {\n    need localmount\n    after udev-settle\n}\n\nstart() {\n";
        agents += "    [ -e /dev/virtio-ports/org.qemu.guest_agent.0 ] && start-stop-daemon --start --background --exec /usr/bin/recguest\n";
        agents += "    return 0\n}\n";
        put(root, "etc/init.d/vm-guest-agents", agents, 0o755);
        enable(root, init, "vm-guest-agents");
        return;
    }
    put(root, "usr/lib/systemd/system/recguest.service",
        "[Unit]\nDescription=QEMU guest agent (recguest)\nConditionPathExists=/dev/virtio-ports/org.qemu.guest_agent.0\n\n[Service]\nExecStart=/usr/bin/recguest\n\n[Install]\nWantedBy=multi-user.target\n", 0o644);
    enable(root, init, "recguest");
}

// The variant's mark (the builder's `logo_*` inputs) as the icon
// distributor-logo-<os id> (os-release LOGO) of the fallback icon theme, in
// the tone of the mode the system starts in (REC_APPEARANCE: the ink mark on
// light, the paper mark on dark), and its wallpapers (`wallpaper_light`,
// `wallpaper_dark`, 3840x2160 JPEGs) as a wallpaper package under the OS's
// name. A desktop group recipe points its start button, About page, desktop
// and login screen at them.
fn branding(root) {
    let mode = os_release_field("REC_APPEARANCE");
    if mode != "light" && mode != "dark" { throw "unknown appearance " + mode + " (light or dark)"; }
    let tone = if mode == "dark" { "paper" } else { "ink" };
    let icons = "usr/share/icons/hicolor/";
    let name = "distributor-logo-" + os_release_field("ID");
    let files = [
        [icons + "scalable/apps/" + name + ".svg", "logo_" + tone],
        [icons + "16x16/apps/" + name + ".svg", "logo_micro_" + tone],
    ];
    // Plasma reads an image's size from its file name, and shows the
    // images_dark one under a dark colour scheme, the images one under a
    // light one.
    let wallpapers = "usr/share/wallpapers/" + pretty_name();
    files.push([wallpapers + "/contents/images/3840x2160.jpg", "wallpaper_light"]);
    files.push([wallpapers + "/contents/images_dark/3840x2160.jpg", "wallpaper_dark"]);
    for file in files {
        mkdir(dirname(join_path(root, file[0])));
        copy_file(INPUTS[file[1]], join_path(root, file[0]));
        target_mode(root, file[0], 0o644);
    }
    put(root, wallpapers + "/metadata.json",
        "{\"KPlugin\": {\"Id\": \"" + pretty_name() + "\", \"Name\": \"" + pretty_name() + "\"}}\n", 0o644);
}

// Apply the installed-system policy to the tree at `root`, whose init is `init`.
fn apply(root, init) {
    check_init(init);
    identity(root);
    put(root, "etc/issue", pretty_name() + " \\l\n", 0o644);
    // A package's own message of the day (Alpine's names its wiki and
    // setup-alpine) is not this system's.
    if is_file(join_path(root, "etc/motd")) { put(root, "etc/motd", "Welcome to " + pretty_name() + ".\n", 0o644); }
    put(root, "etc/locale.conf", "LANG=C.UTF-8\n", 0o644);
    locale_env(root, init);
    // sshd's privilege-separation directories.
    for path in ["var/empty/sshd", "usr/share/empty.sshd"] { directory(root, path, 0o755); }
    let gettys = [];
    for console in all_consoles() { gettys.push([console, ""]); }
    consoles(root, init, gettys);
    boot_target(root, init);
    for service in base_services(init) + capability_services(init) + seat_services(init) {
        if !is_file(join_path(root, service_file(init, service))) {
            throw "installed system lacks the " + init + " service " + service;
        }
        enable(root, init, service);
    }
    bash_login(root);
    homes(root);
    lock_root(root);
    setup_program(root);
    root_effects(root);
    admin_rule(root);
    no_hibernation(root, init);
    guest_agent(root, init);
    branding(root);
    if init == "openrc" {
        // D-Bus cannot initialize an existing empty machine-id, and /var/log
        // may be a fresh live tmpfs or the installed system's shared state.
        put(root, "etc/init.d/recipe-system-setup",
            "#!/sbin/openrc-run\ndescription=\"Initialize machine identity and runtime directories\"\n\ndepend() {\n    need localmount\n    before dbus auditd\n}\n\nstart() {\n    /usr/libexec/recipe-system-setup machine-id || return 1\n    checkpath -d -m 0700 -o root:root /var/log/audit\n}\n", 0o755);
        enable(root, init, "recipe-system-setup");
    }
}
