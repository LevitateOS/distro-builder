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
// and the variant's wallpapers.
fn desktop_system(root, inputs, init, display_manager) {
    desktop_caches(root, os_release_field(inputs, "REC_LIBC"));
    wallpaper(root, inputs);
    session_services(root, init);
    graphical(root, init, display_manager);
}

// The variant's wallpapers (the builder's `wallpaper_light` and
// `wallpaper_dark` inputs, 3840x2160 JPEGs) as a wallpaper package under the
// OS's name, and the default background of the desktop (REC_DESKTOP) for
// every user who has not chosen another.
fn wallpaper(root, inputs) {
    let name = pretty_name(inputs);
    let package = "usr/share/wallpapers/" + name;
    // Plasma reads an image's size from its file name, and shows the
    // images_dark one under a dark colour scheme.
    let dark = package + "/contents/images_dark/3840x2160.jpg";
    for image in [[package + "/contents/images/3840x2160.jpg", "wallpaper_light"], [dark, "wallpaper_dark"]] {
        mkdir(dirname(join_path(root, image[0])));
        copy_file(join_path(inputs, image[1]), join_path(root, image[0]));
        target_mode(root, image[0], 0o644);
    }
    put(root, package + "/metadata.json",
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
        // wallpaper, in /etc/xdg ahead of it. LXQt does not follow a light or
        // dark look, so it shows the dark one; the light one is in the package.
        let packaged = join_path(root, "usr/share/pcmanfm-qt/lxqt/settings.conf");
        if !is_file(packaged) { throw "the LXQt packages lack pcmanfm-qt's desktop settings"; }
        let text = "";
        let set = 0;
        for line in read_file(packaged).split("\n") {
            if line.starts_with("Wallpaper=") { line = "Wallpaper=/" + dark; set += 1; }
            else if line.starts_with("WallpaperMode=") { line = "WallpaperMode=zoom"; set += 1; }
            text += line + "\n";
        }
        if set != 2 { throw "pcmanfm-qt's desktop settings do not name a wallpaper and its mode"; }
        put(root, "etc/xdg/pcmanfm-qt/lxqt/settings.conf", text, 0o644);
    } else {
        throw "no default wallpaper for the desktop " + desktop;
    }
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
