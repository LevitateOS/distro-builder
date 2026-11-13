// The installed-system policy, and the init backend it shares with the live
// overlay (`live-overlay.rhai` imports this module). The policy states each
// feature once; the backend functions, keyed on the init system ("systemd" or
// "openrc"), supply only the on-disk mechanism. This tree is also what
// recstrap installs and recab deploys.

// ---------------------------------------------------------------------------
// Files. Every mode is recorded in Recipe's target metadata. put, link,
// directory and shadow_line have copies in `system-files.rhai` (the
// package-effects step's); change both together.
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

// The services a desktop seat needs besides the display manager. systemd's
// logind is part of systemd and polkit starts on D-Bus demand; OpenRC runs
// elogind (seats and sessions) and polkit as services.
fn seat_services(init) {
    check_init(init);
    if init == "systemd" { [] } else { ["elogind", "polkit"] }
}

// Boot to the graphical desktop: start the display manager and the seat's
// services. systemd starts the display manager through its
// display-manager.service alias, which graphical.target (made the default)
// wants. OpenRC has no target: the display manager starts in the default
// runlevel after dbus and elogind. Alpine's sddm service waits in start_post
// for the greeter's runtime directory, which a boot that logs a user in by
// itself (autologin, as the live desktop does) never makes, so OpenRC would
// mark the running display manager failed; the service's conf.d, which
// OpenRC sources before the service script, makes that wait succeed (its
// shell function shadows the ewaitfile helper for this service only).
fn graphical(root, init, display_manager) {
    check_init(init);
    for service in seat_services(init) + [display_manager] {
        if !is_file(join_path(root, service_file(init, service))) {
            throw "the desktop system lacks the " + init + " service " + service;
        }
    }
    if init == "systemd" {
        link(root, "etc/systemd/system/display-manager.service", "/" + service_file(init, display_manager));
        link(root, "etc/systemd/system/default.target", "/usr/lib/systemd/system/graphical.target");
    } else {
        for service in seat_services(init) + [display_manager] { enable(root, init, service); }
        let conf = "etc/conf.d/" + display_manager;
        let text = read_file_or_empty(join_path(root, conf));
        if text != "" && !text.ends_with("\n") { text += "\n"; }
        text += "# The desktop system (installed-system.rhai): start_post's wait for the\n";
        text += "# greeter's runtime directory succeeds, since an autologin starts no greeter.\n";
        text += "ewaitfile() { return 0; }\n";
        put(root, conf, text, 0o644);
    }
}

// The user session's own services: the session bus and PipeWire's audio.
// systemd starts them as user units, enabled for every user as the packages'
// user presets would (`systemctl --global enable`), with the Unity GTK module
// masked (nothing provides it). OpenRC has no user services: the session bus
// comes with the session (dbus-x11) and PipeWire from its XDG autostart entry.
fn session_services(root, init) {
    check_init(init);
    if init != "systemd" { return; }
    let units = "/usr/lib/systemd/user/";
    for unit in ["dbus-broker.service", "dbus.socket", "pipewire.socket", "pipewire-pulse.socket", "wireplumber.service"] {
        if !is_file(join_path(root, units.sub_string(1) + unit)) {
            throw "the desktop system lacks the systemd user unit " + unit;
        }
    }
    let user = "etc/systemd/user/";
    link(root, user + "dbus.service", units + "dbus-broker.service");
    link(root, user + "sockets.target.wants/dbus.socket", units + "dbus.socket");
    link(root, user + "sockets.target.wants/pipewire.socket", units + "pipewire.socket");
    link(root, user + "sockets.target.wants/pipewire-pulse.socket", units + "pipewire-pulse.socket");
    link(root, user + "pipewire.service.wants/wireplumber.service", units + "wireplumber.service");
    link(root, user + "pipewire-session-manager.service", units + "wireplumber.service");
    link(root, user + "unity-gtk-module.service", "/dev/null");
}

// Make `path` at every boot, with `mode` and owned by the account `owner`
// (for a directory a layer cannot own: the live desktop's home). systemd's
// tmpfiles makes it; OpenRC runs a service named `name` in the boot runlevel,
// before every default service.
fn boot_directory(root, init, name, path, mode, owner) {
    check_init(init);
    if init == "systemd" {
        put(root, "etc/tmpfiles.d/" + name + ".conf", "d " + path + " " + mode + " " + owner + " " + owner + " -\n", 0o644);
    } else {
        put(root, "etc/init.d/" + name,
            "#!/sbin/openrc-run\ndescription=\"Make " + path + "\"\n\ndepend() {\n    need localmount\n}\n\nstart() {\n    checkpath -d -m " + mode + " -o " + owner + ":" + owner + " " + path + "\n}\n",
            0o755);
        link(root, "etc/runlevels/boot/" + name, "/etc/init.d/" + name);
    }
}

// The login on each console. `gettys` is a list of [console, login]: login ""
// is a login prompt, "root" logs root in without asking, and a path runs that
// program as root in place of login. A serial login prompt runs only when
// the boot names the port as a console: systemd's getty generator starts it
// then; busybox init's line runs `serial-login`, which checks the same.
fn consoles(root, init, gettys) { consoles_in_runlevel(root, init, gettys, "default") }
// `consoles`, with OpenRC's last runlevel named `runlevel` (the live desktop
// installer boots its own, without the display manager); systemd ignores it.
fn consoles_in_runlevel(root, init, gettys, runlevel) {
    check_init(init);
    if init == "systemd" {
        for getty in gettys { systemd_getty(root, getty[0], getty[1]); }
    } else {
        openrc_inittab(root, gettys, runlevel);
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
// Busybox init's inittab: OpenRC's runlevels in order (the last one
// `runlevel`), and one getty per console.
fn openrc_inittab(root, gettys, runlevel) {
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
    inittab += "::wait:/sbin/openrc " + runlevel + "\n::ctrlaltdel:/sbin/reboot\n::shutdown:/sbin/openrc shutdown\n";
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
// The consoles with a login: on a desktop system the display manager takes
// the first virtual console, so every console but that one.
fn login_consoles(desktop) {
    let consoles = [];
    for console in all_consoles() {
        if !(desktop && console == "tty1") { consoles.push(console); }
    }
    consoles
}

// The value of `field` in the builder's `os_release` input (the REC_* fields
// are unquoted words).
fn os_release_field(inputs, field) {
    for line in read_file(join_path(inputs, "os_release")).split("\n") {
        if line.starts_with(field + "=") { return trim(line.sub_string(field.len() + 1)); }
    }
    throw "the os_release input has no " + field;
}
// The value of `field` in the builder's `os_release` input without its
// quotes, or "" when it has none (HOME_URL is there only for an OS with a
// website).
fn os_release_optional(inputs, field) {
    for line in read_file(join_path(inputs, "os_release")).split("\n") {
        if line.starts_with(field + "=") {
            let value = trim(line.sub_string(field.len() + 1));
            value = value.replace("\"", "");
            return value;
        }
    }
    ""
}
// Which system the tree is (REC_FLAVOUR): "base", or "desktop", the base
// system with the variant's desktop packages.
fn flavour(inputs) { os_release_field(inputs, "REC_FLAVOUR") }

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
            name = name.replace("\"", "");
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
    let desktop = flavour(inputs) == "desktop";
    let gettys = [];
    for console in login_consoles(desktop) { gettys.push([console, ""]); }
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
    if desktop {
        desktop_system(root, inputs, init, trim(read_file(join_path(inputs, "display_manager"))));
    }
    if init == "openrc" {
        // D-Bus cannot initialize an existing empty machine-id, and /var/log
        // may be a fresh live tmpfs or the installed system's shared state.
        put(root, "etc/init.d/recipe-system-setup",
            "#!/sbin/openrc-run\ndescription=\"Initialize machine identity and runtime directories\"\n\ndepend() {\n    need localmount\n    before dbus auditd\n}\n\nstart() {\n    /usr/libexec/recipe-system-setup machine-id || return 1\n    checkpath -d -m 0700 -o root:root /var/log/audit\n}\n", 0o755);
        enable(root, init, "recipe-system-setup");
    }
}

// ---------------------------------------------------------------------------
// Desktop-system policy: what the desktop system adds over the base system.
// ---------------------------------------------------------------------------

// The desktop system: what the desktop packages' install scripts and triggers
// would have done (their caches; their accounts are the backend's
// package_effects), the user session's services, and the boot into the
// display manager, which takes the first virtual console (`login_consoles`),
// and the variant's branding: its mark on the start button, and its
// wallpapers on the desktop and on the login screen. The mark comes before
// the caches, which index the icons.
fn desktop_system(root, inputs, init, display_manager) {
    start_button(root, inputs);
    desktop_caches(root, os_release_field(inputs, "REC_LIBC"));
    login_screen(root, inputs, display_manager, wallpaper(root, inputs));
    browser(root, inputs);
    default_apps(root, inputs);
    input_defaults(root, inputs);
    look_defaults(root, inputs);
    menu_entries(root, inputs);
    no_hibernation(root, init);
    desktop_polkit(root, inputs);
    session_services(root, init);
    desktop_services(root, init);
    graphical(root, init, display_manager);
}

// The system services the desktop needs that neither start at boot by
// themselves nor on D-Bus demand (udisks, UPower, bolt and fwupd start on
// demand): Bluetooth, the Avahi discovery that driverless printing uses, CUPS,
// the firewall, and power profiles on systemd. systemd starts the VM guest
// agents from their udev rules when the VM has their port; OpenRC runs
// `vm-guest-agents`, which starts them only then, so real hardware shows no
// failed service. A systemd unit's Alias= names are linked as `systemctl
// enable` would, so D-Bus activation (dbus-org.bluez,
// dbus-org.freedesktop.Avahi) finds the service.
fn desktop_services(root, init) {
    check_init(init);
    firewall_zone(root);
    if init == "openrc" {
        for service in ["bluetooth", "avahi-daemon", "cupsd", "firewalld", "qemu-guest-agent", "spice-vdagentd"] {
            if !is_file(join_path(root, service_file(init, service))) { throw "the desktop system lacks the OpenRC service " + service; }
        }
        for service in ["bluetooth", "avahi-daemon", "cupsd", "firewalld"] { enable(root, init, service); }
        let agents = "#!/sbin/openrc-run\ndescription=\"Start the VM guest agents when the machine is a VM with their ports\"\n\n";
        agents += "depend() {\n    need localmount\n    after udev-settle\n}\n\nstart() {\n";
        agents += "    [ -e /dev/virtio-ports/org.qemu.guest_agent.0 ] && rc-service qemu-guest-agent start\n";
        agents += "    [ -e /dev/virtio-ports/com.redhat.spice.0 ] && rc-service spice-vdagentd start\n";
        agents += "    return 0\n}\n";
        put(root, "etc/init.d/vm-guest-agents", agents, 0o755);
        enable(root, init, "vm-guest-agents");
        return;
    }
    for service in ["bluetooth", "avahi-daemon", "cups", "firewalld", "power-profiles-daemon"] {
        let file = service_file(init, service);
        if !is_file(join_path(root, file)) { throw "the desktop system lacks the systemd service " + service; }
        enable(root, init, service);
        for line in read_file(join_path(root, file)).split("\n") {
            if line.starts_with("Alias=") {
                for alias in trim(line.sub_string(6)).split(" ") {
                    if alias != "" { link(root, "etc/systemd/system/" + alias, "/" + file); }
                }
            }
        }
    }
}

// The firewall's default zone, public, blocks every incoming connection but
// ssh (whose server is off unless the user enables it) and DHCPv6 replies;
// the desktop's version also lets mDNS answers in, which Avahi needs to find
// driverless printers and other devices on the network.
fn firewall_zone(root) {
    if !is_file(join_path(root, "usr/lib/firewalld/zones/public.xml")) { throw "the desktop packages lack firewalld's public zone"; }
    let zone = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<zone>\n  <short>Public</short>\n";
    zone += "  <description>Incoming connections are refused, except replies, ssh, DHCPv6 and mDNS.</description>\n";
    zone += "  <service name=\"ssh\"/>\n  <service name=\"dhcpv6-client\"/>\n  <service name=\"mdns\"/>\n  <forward/>\n</zone>\n";
    put(root, "etc/firewalld/zones/public.xml", zone, 0o644);
}

// Firefox as the OS's browser: the distribution it names is the OS (its
// distribution.ini), the packaged default preferences keep everything but the
// distribution's start page, and a policy file turns off telemetry, studies,
// the first-run and what's-new pages, and the browser's own updater (the
// system updates it), and opens the OS's website (HOME_URL) at start where
// it has one. The packaged preferences file is renamed for the OS.
fn browser(root, inputs) {
    let libc = os_release_field(inputs, "REC_LIBC");
    let dir = if libc == "glibc" { "usr/lib64/firefox/" } else { "usr/lib/firefox/" };
    if !is_file(join_path(root, dir + "distribution/distribution.ini")) { throw "the desktop packages lack Firefox's distribution settings in /" + dir; }
    let id = os_release_field(inputs, "ID");
    let name = pretty_name(inputs);
    put(root, dir + "distribution/distribution.ini",
        "[Global]\nid=" + id + "\nversion=1.0\nabout=Mozilla Firefox for " + name + "\n\n[Preferences]\napp.distributor=" + id + "\napp.distributor.channel=" + id + "\n",
        0o644);
    let prefs = dir + "browser/defaults/preferences/";
    let packaged = "";
    for file in ["firefox-redhat-default-prefs.js", "vendor.js"] {
        if is_file(join_path(root, prefs + file)) { packaged = file; }
    }
    if packaged == "" { throw "the desktop packages lack Firefox's default preferences in /" + prefs; }
    let text = "";
    for line in read_file(join_path(root, prefs + packaged)).split("\n") {
        if line.contains("browser.startup.homepage") || line.contains("browser.newtabpage.pinned") { continue; }
        text += line + "\n";
    }
    rm(join_path(root, prefs + packaged));
    put(root, prefs + id + "-default-prefs.js", text, 0o644);
    let policies = "{\n  \"policies\": {\n";
    policies += "    \"DisableTelemetry\": true,\n    \"DisableFirefoxStudies\": true,\n    \"DisableAppUpdate\": true,\n";
    policies += "    \"DontCheckDefaultBrowser\": true,\n    \"OverrideFirstRunPage\": \"\",\n    \"OverridePostUpdatePage\": \"\",\n";
    let home = os_release_optional(inputs, "HOME_URL");
    if home != "" {
        policies += "    \"Homepage\": {\"URL\": \"" + home + "\", \"StartPage\": \"homepage\"},\n";
    }
    policies += "    \"UserMessaging\": {\"ExtensionRecommendations\": false, \"FeatureRecommendations\": false, \"MoreFromMozilla\": false, \"SkipOnboarding\": true}\n";
    policies += "  }\n}\n";
    put(root, dir + "distribution/policies.json", policies, 0o644);
}

// The default application for each kind of file on the desktop (REC_DESKTOP),
// for every user who has not chosen another: the system-wide mimeapps.list in
// /etc/xdg, which comes before the packages' lists (Fedora's
// kde-mimeapps.list included).
// A kind with no application in the image is left to the user.
fn default_apps(root, inputs) {
    let desktop = os_release_field(inputs, "REC_DESKTOP");
    let apps = if desktop == "plasma" {
        #{ browser: "org.mozilla.firefox.desktop", files: "org.kde.dolphin.desktop", text: "org.kde.kwrite.desktop",
           image: "org.mozilla.firefox.desktop", pdf: "org.mozilla.firefox.desktop", archive: "org.kde.ark.desktop" }
    } else if desktop == "lxqt" {
        #{ browser: "firefox.desktop", files: "pcmanfm-qt.desktop", image: "lximage-qt.desktop",
           pdf: "firefox.desktop", archive: "lxqt-archiver.desktop" }
    } else { throw "no default applications for the desktop " + desktop };
    let kinds = #{
        browser: ["x-scheme-handler/http", "x-scheme-handler/https", "text/html", "application/xhtml+xml"],
        files: ["inode/directory"],
        text: ["text/plain"],
        image: ["image/png", "image/jpeg", "image/gif", "image/webp", "image/svg+xml", "image/bmp", "image/tiff"],
        pdf: ["application/pdf"],
        archive: ["application/zip", "application/x-tar", "application/gzip", "application/x-compressed-tar",
                  "application/x-xz-compressed-tar", "application/x-zstd-compressed-tar",
                  "application/x-bzip2-compressed-tar", "application/x-7z-compressed", "application/vnd.rar"]
    };
    let text = "[Default Applications]\n";
    for kind in kinds.keys() {
        let app = apps[kind];
        if type_of(app) == "()" { continue; }
        if !is_file(join_path(root, "usr/share/applications/" + app)) { throw "the desktop packages lack the application " + app; }
        for mime in kinds[kind] { text += mime + "=" + app + ";\n"; }
    }
    put(root, "etc/xdg/mimeapps.list", text, 0o644);
}

// Touchpads tap to click and scroll naturally. Plasma's KWin sets its own
// libinput defaults; on X11 (LXQt) the X server's libinput driver reads them
// from xorg.conf.d.
fn input_defaults(root, inputs) {
    if os_release_field(inputs, "REC_DESKTOP") != "lxqt" { return; }
    put(root, "etc/X11/xorg.conf.d/40-touchpad.conf",
        "Section \"InputClass\"\n    Identifier \"touchpad defaults\"\n    MatchIsTouchpad \"on\"\n    Driver \"libinput\"\n    Option \"Tapping\" \"on\"\n    Option \"NaturalScrolling\" \"true\"\nEndSection\n",
        0o644);
}

// The cursor theme of the desktop (REC_DESKTOP): Breeze's on Plasma, Adwaita's
// with LXQt's GTK look.
fn cursor_theme(desktop) {
    if desktop == "plasma" { "breeze_cursors" } else if desktop == "lxqt" { "Adwaita" } else { throw "no cursor theme for the desktop " + desktop }
}

// ---------------------------------------------------------------------------
// The look: the mode the desktop starts in (REC_APPEARANCE, from the variant's
// `profile.appearance`) and the variant's colours in each mode (the builder's
// `look` input, from `[look.light]` and `[look.dark]`). Every choice below is
// keyed on the mode, never on the desktop; the desktop only says which files
// carry it.
// ---------------------------------------------------------------------------

// The mode the desktop starts in: "light" or "dark".
fn appearance(inputs) {
    let mode = os_release_field(inputs, "REC_APPEARANCE");
    if mode != "light" && mode != "dark" { throw "unknown appearance " + mode + " (light or dark)"; }
    mode
}
// The roles of a palette (distro_contract::Palette).
fn palette_roles() {
    ["surface", "surface_raised", "surface_sunken", "text", "text_secondary", "text_muted", "rule", "hairline",
     "accent", "highlight", "band", "on_band", "ok", "warn", "bad"]
}
// Mode `mode`'s palette from the `look` input: role -> "#RRGGBB".
fn palette(inputs, mode) {
    let colours = #{};
    let prefix = mode + ".";
    for line in read_file(join_path(inputs, "look")).split("\n") {
        if !line.starts_with(prefix) { continue; }
        let pair = line.sub_string(prefix.len()).split("=");
        if pair.len() != 2 { throw "malformed look line: " + line; }
        colours[pair[0]] = trim(pair[1]);
    }
    for role in palette_roles() {
        if !colours.contains(role) { throw "the look input has no " + mode + "." + role; }
        let hex = colours[role];
        if hex.len() != 7 || !hex.starts_with("#") { throw "look colour " + mode + "." + role + " is not #RRGGBB: " + hex; }
    }
    colours
}
// A palette's colours as [r, g, b] (0-255).
fn rgb_palette(p) {
    let out = #{};
    for role in palette_roles() {
        out[role] = rgb(p[role]);
    }
    out
}
// `a` moved towards `b` by `t` (0.0 to 1.0); both [r, g, b].
fn mix(a, b, t) {
    let out = [];
    for i in 0..3 { out.push((a[i].to_float() * (1.0 - t) + b[i].to_float() * t).round().to_int()); }
    out
}
// "r,g,b", as KDE and Konsole write a colour.
fn kde(c) { "" + c[0] + "," + c[1] + "," + c[2] }
// Whether a colour is dark (relative luminance under one half).
fn is_dark(c) {
    (0.2126 * c[0].to_float() + 0.7152 * c[1].to_float() + 0.0722 * c[2].to_float()) / 255.0 < 0.5
}
// The palette (as [r, g, b]) whose accent and status colours read on `bg`:
// the dark mode's on a dark background, the light mode's on a light one.
fn palette_on(inputs, bg) {
    let mode = if is_dark(bg) { "dark" } else { "light" };
    rgb_palette(palette(inputs, mode))
}
// The icon theme of the mode (Breeze on both desktops).
fn icon_theme(mode) { if mode == "dark" { "breeze-dark" } else { "breeze" } }
// The colour schemes' name for mode `mode`: the OS's name and the mode
// ("LevitateOSLight"), the name of a KDE colour scheme and of a terminal scheme.
fn scheme_name(inputs, mode) {
    let name = pretty_name(inputs).replace(" ", "");
    name + (if mode == "dark" { "Dark" } else { "Light" })
}

// `text` (an INI file) with `key` in `section` set to `value`: the line
// replaced where the key is there, added at the end of the section where it
// is not, and the section added at the end where the file lacks it.
fn ini_set(text, section, key, value) {
    if text.ends_with("\n") { text = text.sub_string(0, text.len() - 1); }
    let out = "";
    let current = "";
    let seen = false;
    let done = false;
    for line in text.split("\n") {
        let t = trim(line);
        if t.starts_with("[") && t.ends_with("]") {
            if current == section && !done { out += key + "=" + value + "\n"; done = true; }
            current = t.sub_string(1, t.len() - 2);
            if current == section { seen = true; }
        } else if current == section && t.starts_with(key + "=") {
            if !done { out += key + "=" + value + "\n"; done = true; }
            continue;
        }
        out += line + "\n";
    }
    if !done {
        if !seen { out += "\n[" + section + "]\n"; }
        out += key + "=" + value + "\n";
    }
    out
}

// One group of a KDE colour scheme: backgrounds `bg` and `alt`, text `fg`,
// `inactive` text, the `active`, link and focus colour `active`, `visited`
// links, and `status`'s ok, warn and bad as positive, neutral and negative.
fn kde_group(name, bg, alt, fg, inactive, active, visited, status) {
    let t = "[" + name + "]\n";
    t += "BackgroundAlternate=" + kde(alt) + "\nBackgroundNormal=" + kde(bg) + "\n";
    t += "DecorationFocus=" + kde(active) + "\nDecorationHover=" + kde(active) + "\n";
    t += "ForegroundActive=" + kde(active) + "\nForegroundInactive=" + kde(inactive) + "\n";
    t += "ForegroundLink=" + kde(active) + "\nForegroundNegative=" + kde(status.bad) + "\n";
    t += "ForegroundNeutral=" + kde(status.warn) + "\nForegroundNormal=" + kde(fg) + "\n";
    t += "ForegroundPositive=" + kde(status.ok) + "\nForegroundVisited=" + kde(visited) + "\n\n";
    t
}
// The colour groups of mode `mode`'s KDE colour scheme, from its palette:
// window surface, views raised with sunken alternate rows, the accent for
// selection (with surface text), focus, hover and links, the highlight for
// visited links, ok/warn/bad for positive/neutral/negative, and the band
// (with on_band text) for headers, title bars and the complementary areas
// (login and lock screens, OSDs). The effects are Breeze's.
fn kde_colour_groups(inputs, mode) {
    let p = rgb_palette(palette(inputs, mode));
    let s = palette_on(inputs, p.accent);
    let b = palette_on(inputs, p.band);
    let t = "[ColorEffects:Disabled]\nColor=56,56,56\nColorAmount=0\nColorEffect=0\nContrastAmount=0.65\nContrastEffect=1\nIntensityAmount=0.1\nIntensityEffect=2\n\n";
    t += "[ColorEffects:Inactive]\nChangeSelectionColor=true\nColor=112,111,110\nColorAmount=0.025\nColorEffect=2\nContrastAmount=0.1\nContrastEffect=2\nEnable=false\nIntensityAmount=0\nIntensityEffect=0\n\n";
    t += kde_group("Colors:Button", p.surface_raised, mix(p.surface_raised, p.accent, 0.25), p.text, p.text_muted, p.accent, p.highlight, p);
    t += kde_group("Colors:Complementary", p.band, mix(p.band, p.on_band, 0.1), p.on_band, mix(p.on_band, p.band, 0.35), b.accent, b.highlight, b);
    t += kde_group("Colors:Header", p.band, mix(p.band, p.on_band, 0.1), p.on_band, mix(p.on_band, p.band, 0.35), b.accent, b.highlight, b);
    t += kde_group("Colors:Header][Inactive", p.surface, p.surface_sunken, p.text, p.text_muted, p.accent, p.highlight, p);
    t += kde_group("Colors:Selection", p.accent, mix(p.accent, p.surface, 0.5), p.surface, mix(p.surface, p.accent, 0.3), p.surface, p.surface, s);
    t += kde_group("Colors:Tooltip", p.surface_raised, p.surface, p.text, p.text_muted, p.accent, p.highlight, p);
    t += kde_group("Colors:View", p.surface_raised, p.surface_sunken, p.text, p.text_muted, p.accent, p.highlight, p);
    t += kde_group("Colors:Window", p.surface, p.surface_sunken, p.text, p.text_muted, p.accent, p.highlight, p);
    t += "[WM]\nactiveBackground=" + kde(p.band) + "\nactiveBlend=" + kde(p.band) + "\nactiveForeground=" + kde(p.on_band) + "\n";
    t += "inactiveBackground=" + kde(p.surface_sunken) + "\ninactiveBlend=" + kde(p.surface_sunken) + "\ninactiveForeground=" + kde(p.text_muted) + "\n\n";
    t
}
// Mode `mode`'s whole KDE colour scheme file.
fn kde_colour_scheme(inputs, mode) {
    let t = kde_colour_groups(inputs, mode);
    t += "[General]\nColorScheme=" + scheme_name(inputs, mode) + "\nName=" + pretty_name(inputs) + (if mode == "dark" { " Dark" } else { " Light" }) + "\nshadeSortColumn=true\n\n";
    t += "[KDE]\ncontrast=4\n";
    t
}

// The terminal's blue, magenta and cyan for mode `mode` (the same on both
// distros): the look palette has no such hues, and its accent and highlight
// sit too close to green and yellow to tell apart in `ls`, diffs and
// compiler output.
fn terminal_hues(mode) {
    if mode == "dark" { #{ blue: "#9BB8D9", magenta: "#D4A5C9", cyan: "#9ACFC4" } }
    else { #{ blue: "#2A5C8A", magenta: "#7A3F6E", cyan: "#1F6F6B" } }
}
// "#RRGGBB" as [r, g, b].
fn rgb(hex) { [parse_int(hex.sub_string(1, 2), 16), parse_int(hex.sub_string(3, 2), 16), parse_int(hex.sub_string(5, 2), 16)] }

// Mode `mode`'s terminal colour scheme (Konsole's and QTermWidget's format):
// the palette's surface behind its text, and a 16-colour set at least 3:1 on
// that surface: black the text on light (on dark, the muted text dimmed
// towards the surface, which still reads), bright black the muted text,
// white the secondary text and bright white the text; red, green, yellow
// bad, ok, warn; blue, magenta, cyan terminal_hues. Bright colours lean
// towards the text, faint ones towards the surface.
fn terminal_scheme(inputs, mode) {
    let p = rgb_palette(palette(inputs, mode));
    let hues = terminal_hues(mode);
    let blue = rgb(hues.blue);
    let magenta = rgb(hues.magenta);
    let cyan = rgb(hues.cyan);
    let black = if mode == "dark" { mix(p.text_muted, p.surface, 0.35) } else { p.text };
    let colours = [
        [black, p.text_muted],
        [p.bad, mix(p.bad, p.text, 0.25)],
        [p.ok, mix(p.ok, p.text, 0.25)],
        [p.warn, mix(p.warn, p.text, 0.25)],
        [blue, mix(blue, p.text, 0.25)],
        [magenta, mix(magenta, p.text, 0.25)],
        [cyan, mix(cyan, p.text, 0.25)],
        [p.text_secondary, p.text],
    ];
    let t = "[Background]\nColor=" + kde(p.surface) + "\n\n[BackgroundFaint]\nColor=" + kde(p.surface) + "\n\n[BackgroundIntense]\nColor=" + kde(p.surface) + "\n\n";
    for i in 0..8 {
        let c = colours[i];
        t += "[Color" + i + "]\nColor=" + kde(c[0]) + "\n\n";
        t += "[Color" + i + "Faint]\nColor=" + kde(mix(c[0], p.surface, 0.5)) + "\n\n";
        t += "[Color" + i + "Intense]\nColor=" + kde(c[1]) + "\n\n";
    }
    t += "[Foreground]\nColor=" + kde(p.text) + "\n\n[ForegroundFaint]\nColor=" + kde(p.text_secondary) + "\n\n[ForegroundIntense]\nColor=" + kde(p.text) + "\n\n";
    t += "[General]\nBlur=false\nColorRandomization=false\nDescription=" + pretty_name(inputs) + (if mode == "dark" { " Dark" } else { " Light" }) + "\nOpacity=1\nWallpaper=\n";
    t
}

// The desktop's look for every user who has not chosen another, in the mode
// it starts in (appearance) and the variant's colours: one cursor theme for
// X, GTK, Qt and the login screen (the `default` icon theme, which every
// toolkit falls back to), then the desktop's own files (plasma_look,
// lxqt_look) and its terminal's colours.
fn look_defaults(root, inputs) {
    let desktop = os_release_field(inputs, "REC_DESKTOP");
    let mode = appearance(inputs);
    let cursor = cursor_theme(desktop);
    if !is_dir(join_path(root, "usr/share/icons/" + cursor + "/cursors")) { throw "the desktop packages lack the cursor theme " + cursor; }
    if !is_file(join_path(root, "usr/share/icons/" + icon_theme(mode) + "/index.theme")) { throw "the desktop packages lack the icon theme " + icon_theme(mode); }
    put(root, "usr/share/icons/default/index.theme", "[Icon Theme]\nInherits=" + cursor + "\n", 0o644);
    if desktop == "plasma" { plasma_look(root, inputs, mode); }
    else if desktop == "lxqt" { lxqt_look(root, inputs, mode, cursor); }
    else { throw "no look for the desktop " + desktop; }
}

// Plasma: the colour schemes <OS>Light and <OS>Dark from the palettes; the
// starting mode's scheme, Fedora's Global Theme of that mode, Breeze icons of
// that mode and the scheme's own accent in /etc/xdg/kdeglobals, with the
// scheme's colours there too (the login screen and apps outside the session
// read them). Plasma copies the Global Theme's defaults into each user's
// kdedefaults at login, ahead of /etc/xdg, so Fedora's light and dark Global
// Themes name our schemes in place of Breeze's. The About page shows the
// start button's mark and the website. Konsole's default profile uses the
// starting mode's terminal scheme.
fn plasma_look(root, inputs, mode) {
    let lnf = "usr/share/plasma/look-and-feel/";
    let look = if mode == "dark" { "org.fedoraproject.fedoradark.desktop" } else { "org.fedoraproject.fedoralight.desktop" };
    if !is_dir(join_path(root, lnf + look)) { throw "the Plasma packages lack the Global Theme " + look; }
    for m in ["light", "dark"] {
        put(root, "usr/share/color-schemes/" + scheme_name(inputs, m) + ".colors", kde_colour_scheme(inputs, m), 0o644);
    }
    let set = false;
    for pair in [["org.fedoraproject.fedora.desktop", "BreezeLight", "light"], ["org.fedoraproject.fedoralight.desktop", "BreezeLight", "light"],
                 ["org.fedoraproject.fedoradark.desktop", "BreezeDark", "dark"]] {
        let defaults = lnf + pair[0] + "/contents/defaults";
        if !is_file(join_path(root, defaults)) { continue; }
        let text = read_file(join_path(root, defaults));
        let packaged = "ColorScheme=" + pair[1] + "\n";
        if !text.contains(packaged) { continue; }
        text = text.replace(packaged, "ColorScheme=" + scheme_name(inputs, pair[2]) + "\n");
        write_file(join_path(root, defaults), text);
        if pair[0] == look { set = true; }
    }
    if !set { throw "the Global Theme " + look + " no longer names a Breeze colour scheme in its defaults"; }
    let globals = "[KDE]\nLookAndFeelPackage=" + look + "\n\n";
    globals += "[General]\nColorScheme=" + scheme_name(inputs, mode) + "\naccentColorFromWallpaper=false\n\n";
    globals += "[Icons]\nTheme=" + icon_theme(mode) + "\n\n";
    globals += kde_colour_groups(inputs, mode);
    put(root, "etc/xdg/kdeglobals", globals, 0o644);
    let about = "[General]\nLogoPath=/usr/share/icons/hicolor/scalable/apps/distributor-logo-" + os_release_field(inputs, "ID") + ".svg\nVariant=Desktop\n";
    let home = os_release_optional(inputs, "HOME_URL");
    if home != "" { about += "Website=" + home + "\n"; }
    put(root, "etc/xdg/kcm-about-distrorc", about, 0o644);
    // Konsole finds colour schemes and profiles in the data directories'
    // konsole/; konsolerc names the profile new windows use.
    // The profile takes the dark scheme whatever the desktop's mode: a terminal
    // is dark on both distros.
    if !is_file(join_path(root, "usr/bin/konsole")) { throw "the Plasma packages lack Konsole"; }
    for m in ["light", "dark"] {
        put(root, "usr/share/konsole/" + scheme_name(inputs, m) + ".colorscheme", terminal_scheme(inputs, m), 0o644);
    }
    let profile = pretty_name(inputs).replace(" ", "");
    put(root, "usr/share/konsole/" + profile + ".profile",
        "[Appearance]\nColorScheme=" + scheme_name(inputs, "dark") + "\n\n[General]\nName=" + profile + "\nParent=FALLBACK/\n", 0o644);
    let konsolerc = read_file_or_empty(join_path(root, "etc/xdg/konsolerc"));
    put(root, "etc/xdg/konsolerc", ini_set(konsolerc, "Desktop Entry", "DefaultProfile", profile + ".profile"), 0o644);
}

// LXQt: its packaged settings (lxqt.conf) with the packaged LXQt theme of
// the starting mode ("light" or "dark"), Breeze icons of that mode, Fusion
// widgets and the Qt palette from the mode's colours; GTK apps (Firefox,
// Mousepad, Simple Scan) in Adwaita with the same icons, cursor and font,
// dark when the mode is; Openbox's packaged decoration that fits the mode;
// and QTerminal's colours. The wallpaper and the desktop's text colours are
// pcmanfm-qt's (wallpaper()).
fn lxqt_look(root, inputs, mode, cursor) {
    let p = palette(inputs, mode);
    let packaged = join_path(root, "usr/share/lxqt/lxqt.conf");
    if !is_file(packaged) { throw "the LXQt packages lack LXQt's settings"; }
    // LXQt's light and dark themes are named after the mode.
    if !is_dir(join_path(root, "usr/share/lxqt/themes/" + mode)) { throw "the LXQt packages lack the LXQt theme " + mode; }
    let conf = read_file(packaged);
    conf = ini_set(conf, "General", "theme", mode);
    conf = ini_set(conf, "General", "icon_theme", icon_theme(mode));
    conf = ini_set(conf, "Qt", "style", "Fusion");
    // The keys LXQt's Qt platform plugin reads, as its packaged palettes
    // (/usr/share/lxqt/palettes) write them.
    for pair in [["window_color", p.surface], ["window_text_color", p.text], ["base_color", p.surface_raised],
                 ["text_color", p.text], ["highlight_color", p.accent], ["highlighted_text_color", p.surface],
                 ["link_color", p.accent], ["link_visited_color", p.highlight],
                 ["tooltip_base_color", p.surface_raised], ["tooltip_text_color", p.text]] {
        conf = ini_set(conf, "Palette", pair[0], pair[1]);
    }
    put(root, "etc/xdg/lxqt/lxqt.conf", conf, 0o644);
    let gtk = "[Settings]\ngtk-theme-name=Adwaita\ngtk-icon-theme-name=" + icon_theme(mode) + "\ngtk-cursor-theme-name=" + cursor + "\ngtk-font-name=Sans 11\n";
    gtk += "gtk-application-prefer-dark-theme=" + (if mode == "dark" { "true" } else { "false" }) + "\n";
    put(root, "etc/gtk-3.0/settings.ini", gtk, 0o644);
    put(root, "etc/gtk-4.0/settings.ini", gtk, 0o644);
    // Openbox reads /etc/xdg/openbox/rc.xml when the user has none. Its
    // packaged themes have no fully dark one: Onyx has dark active title bars
    // and menus; Clearlooks (the packaged choice) is light.
    let decoration = if mode == "dark" { "Onyx" } else { "Clearlooks" };
    if !is_file(join_path(root, "usr/share/themes/" + decoration + "/openbox-3/themerc")) { throw "the LXQt packages lack the Openbox theme " + decoration; }
    let rc = "etc/xdg/openbox/rc.xml";
    if !is_file(join_path(root, rc)) { throw "the LXQt packages lack Openbox's settings"; }
    let xml = read_file(join_path(root, rc));
    let theme = xml.index_of("<theme>");
    let open = if theme < 0 { -1 } else { xml.index_of("<name>", theme) };
    let close = if open < 0 { -1 } else { xml.index_of("</name>", open) };
    if close < 0 { throw "Openbox's settings name no theme"; }
    xml = xml.sub_string(0, open + 6) + decoration + xml.sub_string(close);
    put(root, rc, xml, 0o644);
    // QTermWidget finds colour schemes by file name in its color-schemes
    // directory; QTerminal reads its settings (QSettings, qterminal.org) from
    // /etc/xdg when the user has none.
    // The default is the dark scheme whatever the desktop's mode: a terminal
    // is dark on both distros.
    let schemes = "usr/share/qtermwidget6/color-schemes/";
    if !is_dir(join_path(root, schemes)) { throw "the LXQt packages lack QTermWidget's colour schemes"; }
    for m in ["light", "dark"] {
        put(root, schemes + scheme_name(inputs, m) + ".colorscheme", terminal_scheme(inputs, m), 0o644);
    }
    put(root, "etc/xdg/qterminal.org/qterminal.ini", "[General]\ncolorScheme=" + scheme_name(inputs, "dark") + "\n", 0o644);
}

// Menu entries of the packages that do nothing useful here, hidden (still
// there for whoever runs them): on LXQt, CUPS's web page (Print Settings
// does the job), Hibernate (no_hibernation) and the bare XScreenSaver daemon
// (its settings stay). Plasma's menu has none.
fn menu_entries(root, inputs) {
    if os_release_field(inputs, "REC_DESKTOP") != "lxqt" { return; }
    for name in ["cups.desktop", "lxqt-hibernate.desktop", "xscreensaver.desktop"] {
        let path = "usr/share/applications/" + name;
        if !is_file(join_path(root, path)) { throw "the desktop packages no longer ship the menu entry " + name; }
        let text = read_file(join_path(root, path));
        if !text.contains("[Desktop Entry]\n") || text.contains("\nNoDisplay=") { throw "the menu entry " + name + " is not one this recipe can hide"; }
        text = text.replace("[Desktop Entry]\n", "[Desktop Entry]\nNoDisplay=true\n");
        write_file(join_path(root, path), text);
    }
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

// What a user at the machine (an active local session) does without the
// administrator's password beyond the packages' own defaults, which already
// allow mounting removable drives, connecting to networks and suspending:
// setting the time zone, and saving a network for every user (a Wi-Fi
// password the system keeps).
fn desktop_polkit(root, inputs) {
    let rules = "// The desktop system (installed-system.rhai): what a user at the machine does without the administrator's password.\n";
    rules += "polkit.addRule(function(action, subject) {\n    if (subject.active && subject.local &&\n";
    rules += "            (action.id == \"org.freedesktop.timedate1.set-timezone\" ||\n";
    rules += "             action.id == \"org.freedesktop.NetworkManager.settings.modify.system\")) {\n";
    rules += "        return polkit.Result.YES;\n    }\n});\n";
    put(root, "etc/polkit-1/rules.d/50-" + os_release_field(inputs, "ID") + "-desktop.rules", rules, 0o644);
}

// The variant's mark (the builder's `logo_*` inputs) as the icon
// distributor-logo-<os id> of the fallback icon theme, and on the start
// button of the desktop (REC_DESKTOP), in the colour its panel needs: the
// panel follows the mode the desktop starts in (appearance), so the ink mark
// on light and the paper mark on dark. The About page shows the same icon.
fn start_button(root, inputs) {
    let icons = "usr/share/icons/hicolor/";
    if !is_file(join_path(root, icons + "index.theme")) { throw "the desktop packages lack the hicolor icon theme"; }
    let desktop = os_release_field(inputs, "REC_DESKTOP");
    if desktop != "plasma" && desktop != "lxqt" { throw "no start button for the desktop " + desktop; }
    let tone = if appearance(inputs) == "dark" { "paper" } else { "ink" };
    let name = "distributor-logo-" + os_release_field(inputs, "ID");
    let icon = icons + "scalable/apps/" + name + ".svg";
    for file in [[icon, "logo_" + tone], [icons + "16x16/apps/" + name + ".svg", "logo_micro_" + tone]] {
        mkdir(dirname(join_path(root, file[0])));
        copy_file(join_path(inputs, file[1]), join_path(root, file[0]));
        target_mode(root, file[0], 0o644);
    }
    if desktop == "plasma" {
        // Fedora's looks set a new launcher's icon to "start-here", Fedora's
        // logo (kde-settings-plasma); they set ours instead.
        let fedora = "writeConfig(\"icon\", \"start-here\")";
        let set = 0;
        for look in ["fedora", "fedoradark", "fedoralight"] {
            let scripts = "usr/share/plasma/look-and-feel/org.fedoraproject." + look + ".desktop/contents/plasmoidsetupscripts";
            for script in tree_files(root, scripts, ".js") {
                let text = read_file(script);
                if !text.contains(fedora) { continue; }
                text = text.replace(fedora, "writeConfig(\"icon\", \"" + name + "\")");
                write_file(script, text);
                set += 1;
            }
        }
        if set == 0 { throw "the Plasma packages no longer set the launcher's icon to start-here"; }
    } else {
        // LXQt's menu button takes its icon from the LXQt theme unless the
        // panel's settings give it its own.
        let conf = "etc/xdg/lxqt/panel.conf";
        let text = read_file_or_empty(join_path(root, conf));
        let section = "[fancymenu]\n";
        if !text.contains(section) { throw "the LXQt panel's settings have no fancymenu button"; }
        text = text.replace(section, section + "ownIcon=true\nicon=/" + icon + "\n");
        put(root, conf, text, 0o644);
    }
}

// The variant's wallpapers (the builder's `wallpaper_light` and
// `wallpaper_dark` inputs, 3840x2160 JPEGs) as a wallpaper package under the
// OS's name, and the default background of the desktop (REC_DESKTOP) for
// every user who has not chosen another: the one of the mode the desktop
// starts in (appearance). Returns that one's path in the system.
fn wallpaper(root, inputs) {
    let name = pretty_name(inputs);
    let wallpapers = "usr/share/wallpapers/" + name;
    // Plasma reads an image's size from its file name, and shows the
    // images_dark one under a dark colour scheme, the images one under a
    // light one: the starting mode's scheme picks the mode's image.
    let light = wallpapers + "/contents/images/3840x2160.jpg";
    let dark = wallpapers + "/contents/images_dark/3840x2160.jpg";
    let mode = appearance(inputs);
    let shown = if mode == "dark" { dark } else { light };
    for image in [[light, "wallpaper_light"], [dark, "wallpaper_dark"]] {
        mkdir(dirname(join_path(root, image[0])));
        copy_file(join_path(inputs, image[1]), join_path(root, image[0]));
        target_mode(root, image[0], 0o644);
    }
    put(root, wallpapers + "/metadata.json",
        "{\"KPlugin\": {\"Id\": \"" + name + "\", \"Name\": \"" + name + "\"}}\n", 0o644);
    let desktop = os_release_field(inputs, "REC_DESKTOP");
    if desktop == "plasma" {
        // Fedora's Global Theme shows the package "Fedora", a link to
        // "Default" (desktop-backgrounds-kde), which pointed at Fedora's own.
        if !exists(join_path(root, "usr/share/wallpapers/Fedora")) {
            throw "the Plasma packages no longer take their wallpaper from /usr/share/wallpapers/Default";
        }
        link(root, "usr/share/wallpapers/Default", name);
    } else if desktop == "lxqt" {
        // pcmanfm-qt draws the desktop and reads one whole settings file, the
        // first on the configuration path: the packaged one, with our
        // wallpaper, the mode's icons and desktop text in the mode's colours
        // (text with a shadow of the surface, which reads on that mode's
        // wallpaper), in /etc/xdg ahead of it. LXQt does not switch the
        // wallpaper with the look; the other one is in the package.
        let packaged = join_path(root, "usr/share/pcmanfm-qt/lxqt/settings.conf");
        if !is_file(packaged) { throw "the LXQt packages lack pcmanfm-qt's desktop settings"; }
        let text = read_file(packaged);
        for key in ["Wallpaper=", "WallpaperMode=", "BgColor=", "FgColor=", "ShadowColor="] {
            if !text.contains("\n" + key) { throw "pcmanfm-qt's desktop settings have no " + key; }
        }
        let p = palette(inputs, mode);
        text = ini_set(text, "System", "IconThemeName", icon_theme(mode));
        text = ini_set(text, "Desktop", "Wallpaper", "/" + shown);
        text = ini_set(text, "Desktop", "WallpaperMode", "zoom");
        text = ini_set(text, "Desktop", "BgColor", p.surface);
        text = ini_set(text, "Desktop", "FgColor", p.text);
        text = ini_set(text, "Desktop", "ShadowColor", p.surface);
        put(root, "etc/xdg/pcmanfm-qt/lxqt/settings.conf", text, 0o644);
    } else {
        throw "no default wallpaper for the desktop " + desktop;
    }
    "/" + shown
}

// The login screen: the display manager's theme of the desktop (REC_DESKTOP)
// with `background`, the wallpaper of the starting mode, behind it, and the
// desktop's cursor theme. Breeze's theme also takes a background colour (its
// `color`, shown while the image loads): the mode's surface. Breeze draws in
// the scheme of /etc/xdg/kdeglobals (plasma_look); the Maldives theme has no
// colour settings.
fn login_screen(root, inputs, display_manager, background) {
    if display_manager != "sddm" { throw "no login screen for the display manager " + display_manager; }
    let themes = "usr/share/sddm/themes/";
    let conf = "[General]\nbackground=" + background + "\n";
    let desktop = os_release_field(inputs, "REC_DESKTOP");
    let theme = "";
    if desktop == "plasma" {
        // KDE's Breeze theme, which shows no logo, in place of Fedora's copy
        // of it with Fedora's logo and background. A theme.conf.user beside
        // a theme's theme.conf overrides it.
        theme = "breeze";
        if !is_file(join_path(root, themes + theme + "/theme.conf")) { throw "the Plasma packages lack SDDM's Breeze theme"; }
        let p = palette(inputs, appearance(inputs));
        put(root, themes + theme + "/theme.conf.user", conf + "color=" + p.surface + "\n", 0o644);
        // Fedora's copy is not offered at all.
        let fedora = join_path(root, themes + "01-breeze-fedora");
        if exists(fedora) { rm(fedora); }
    } else if desktop == "lxqt" {
        // SDDM's own Maldives theme under our name: Alpine's copy does not
        // say it is for the Qt 6 greeter, the only one Alpine has.
        theme = os_release_field(inputs, "ID");
        let packaged = themes + "maldives/";
        if !is_file(join_path(root, packaged + "metadata.desktop")) { throw "the LXQt packages lack SDDM's Maldives theme"; }
        for file in ["Main.qml", "angle-down.png", "rectangle.png"] {
            mkdir(join_path(root, themes + theme));
            copy_file(join_path(root, packaged + file), join_path(root, themes + theme + "/" + file));
            target_mode(root, themes + theme + "/" + file, 0o644);
        }
        let metadata = "";
        for line in read_file(join_path(root, packaged + "metadata.desktop")).split("\n") {
            if line.starts_with("Name=") { line = "Name=" + pretty_name(inputs); }
            else if line.starts_with("Theme-Id=") { line = "Theme-Id=" + theme; }
            else if line.starts_with("Screenshot=") || line.starts_with("QtVersion=") || line == "" { continue; }
            metadata += line + "\n";
        }
        put(root, themes + theme + "/metadata.desktop", metadata + "QtVersion=6\n", 0o644);
        put(root, themes + theme + "/theme.conf", conf, 0o644);
    } else {
        throw "no login screen for the desktop " + desktop;
    }
    put(root, "etc/sddm.conf.d/10-theme.conf",
        "[Theme]\nCurrent=" + theme + "\nCursorTheme=" + cursor_theme(desktop) + "\n", 0o644);
}

// The first of `programs` (paths relative to the root) the tree has, or "".
fn tree_program(root, programs) {
    for program in programs { if is_file(join_path(root, program)) { return program; } }
    ""
}

// Runs the tree's `program` (relative to the root) with `args` and returns
// its output. The build has no chroot, so it runs through the tree's own
// dynamic loader with the tree's libraries first: the libc (REC_LIBC) names
// the loader and the library directories.
fn tree_run(root, libc, program, args) {
    let loader = "";
    let libs = [];
    if libc == "glibc" { loader = "usr/lib64/ld-linux-x86-64.so.2"; libs = ["usr/lib64"]; }
    else if libc == "musl" { loader = "lib/ld-musl-x86_64.so.1"; libs = ["lib", "usr/lib"]; }
    else { throw "unsupported libc " + libc; }
    let loader_path = join_path(root, loader);
    if !is_file(loader_path) { throw "the tree has no " + libc + " loader /" + loader; }
    chmod(loader_path, 0o755);
    let path = "";
    for dir in libs { path += (if path == "" { "" } else { ":" }) + join_path(root, dir); }
    exec_output(loader_path, ["--library-path", path, join_path(root, program)] + args)
}

// The files of directory `dir` (relative to the root) whose names end in
// `suffix`, as absolute paths.
fn tree_files(root, dir, suffix) {
    let files = [];
    let path = join_path(root, dir);
    if !is_dir(path) { return files; }
    for name in shell_output("ls '" + path + "'").split("\n") {
        if trim(name) != "" && name.ends_with(suffix) { files.push(join_path(path, trim(name))); }
    }
    files
}

// Runs the first of the tree's programs `names` (under usr/bin) with `args`
// and returns its output; () when the tree has none of them.
fn cache_program(root, libc, names, args) {
    let candidates = [];
    for name in names { candidates.push("usr/bin/" + name); }
    let program = tree_program(root, candidates);
    if program == "" { return (); }
    log("  " + basename(program));
    tree_run(root, libc, program, args)
}

// What the desktop packages' install scripts and triggers would have done:
// the caches their programs build from the tree's own files (GSettings
// schemas, GIO modules, GdkPixbuf loaders, GTK input methods, MIME types,
// fonts, icon themes, desktop entries). Each runs where the tree has its
// program and its files. A program that prints its cache (the loaders, the
// input methods) prints the tree's paths, which are written without the
// root. ldconfig is not among them: systemd's ldconfig.service makes
// /etc/ld.so.cache at the first boot, and musl has no cache.
fn desktop_caches(root, libc) {
    log("Building the desktop packages' caches...");
    let lib = if libc == "glibc" { "usr/lib64" } else { "usr/lib" };
    let schemas = join_path(root, "usr/share/glib-2.0/schemas");
    if is_dir(schemas) { cache_program(root, libc, ["glib-compile-schemas"], [schemas]); }
    let gio = join_path(root, lib + "/gio/modules");
    if is_dir(gio) { cache_program(root, libc, ["gio-querymodules-64", "gio-querymodules"], [gio]); }
    for cache in [
        [["gdk-pixbuf-query-loaders-64", "gdk-pixbuf-query-loaders"], lib + "/gdk-pixbuf-2.0/2.10.0/loaders", lib + "/gdk-pixbuf-2.0/2.10.0/loaders.cache"],
        [["gtk-query-immodules-3.0-64", "gtk-query-immodules-3.0"], lib + "/gtk-3.0/3.0.0/immodules", lib + "/gtk-3.0/3.0.0/immodules.cache"],
    ] {
        let modules = tree_files(root, cache[1], ".so");
        if modules.is_empty() { continue; }
        let text = cache_program(root, libc, cache[0], modules);
        if type_of(text) == "()" { continue; }
        put(root, cache[2], replace(text, root, ""), 0o644);
    }
    let mime = join_path(root, "usr/share/mime");
    if is_dir(mime) { cache_program(root, libc, ["update-mime-database"], [mime]); }
    if is_dir(join_path(root, "usr/share/fonts")) {
        cache_program(root, libc, ["fc-cache-64", "fc-cache"], ["--sysroot=" + root, "--system-only"]);
        // The X server's core font directories (bitmap fonts) are indexed
        // by fonts.scale and fonts.dir, as mkfontdir would.
        for dir in shell_output("ls '" + join_path(root, "usr/share/fonts") + "'").split("\n") {
            let path = join_path(root, "usr/share/fonts/" + trim(dir));
            if trim(dir) == "" || trim(dir) == "encodings" || tree_files(root, "usr/share/fonts/" + trim(dir), ".pcf.gz").is_empty() { continue; }
            cache_program(root, libc, ["mkfontscale"], [path]);
            cache_program(root, libc, ["mkfontscale"], ["-b", "-s", "-l", path]);
        }
    }
    let icons = join_path(root, "usr/share/icons");
    if is_dir(icons) {
        for theme in shell_output("ls '" + icons + "'").split("\n") {
            let dir = join_path(icons, trim(theme));
            if trim(theme) != "" && is_file(join_path(dir, "index.theme")) {
                cache_program(root, libc, ["gtk-update-icon-cache"], ["-q", "-t", "-f", dir]);
            }
        }
    }
    let applications = join_path(root, "usr/share/applications");
    if is_dir(applications) { cache_program(root, libc, ["update-desktop-database"], ["-q", applications]); }
}
