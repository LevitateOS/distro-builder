// The live-system policy: what the live session adds over the installed system
// (the rootfs below this overlay, which it never modifies). Each feature is
// stated once; the init backend in installed-system.rhai supplies the on-disk
// mechanism. The network is the installed system's: its own stack brings up
// every interface. On the desktop system, which already boots into its
// display manager, the session also logs the live user into the desktop.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ config: "file", passwd: "file", group: "file", shadow: "file", shell_ux: "file", live_help: "file", shutdown_cleanup: "file", serial_profile: "file" }, outputs: #{ overlay: "directory" } };
import "installed-system.rhai" as system;
// The `config` input is distro_builder's `LiveSession` (pipeline/overlay.rs),
// serialized; the builder's types guarantee its values (init is "systemd" or
// "openrc", the variant contract validated the service names). The guest files
// arrive rendered, each as its own input.
let config = parse_json(read_file(join_path(RUNTIME.paths.inputs, "config")));
config.inputs = RUNTIME.paths.inputs;
let ctx = #{name: "live-overlay", config: config,
    root: join_path(RUNTIME.paths.output, "overlay")};

// The rendered guest file the builder passed as input `name`.
fn input(c, name) { read_file(join_path(c.inputs, name)) }

// The groups of the devices a desktop session uses, besides wheel (the
// privilege tool's admin group); the live user joins those the system has.
fn session_groups() { ["wheel", "video", "input", "audio", "netdev", "plugdev"] }

// The live desktop's user (`c.desktop`): the desktop does not run as root,
// so the display manager logs this account in without asking, through its
// autologin (the same /etc/sddm.conf.d drop-in on every init). It is in
// wheel, so the privilege tool takes its password in the session's terminal.
// Its home is made at boot, since a layer cannot own a directory.
fn live_user(root, c) {
    let d = c.desktop;
    let user = d.user;
    let id = "1000";
    let passwd = input(c, "passwd");
    for line in passwd.split("\n") {
        if line.starts_with(user + ":") || line.split(":").len() > 2 && line.split(":")[2] == id {
            throw "the rootfs already has the live user's name or id: " + line;
        }
    }
    if passwd != "" && !passwd.ends_with("\n") { passwd += "\n"; }
    system::put(root, "etc/passwd", passwd + user + ":x:" + id + ":" + id + ":Live User:/home/" + user + ":/bin/bash\n", 0o644);
    let group = "";
    for line in input(c, "group").split("\n") {
        if line == "" { continue; }
        let fields = line.split(":");
        if fields.len() == 4 && session_groups().contains(fields[0]) {
            line += if fields[3] == "" { user } else { "," + user };
        }
        group += line + "\n";
    }
    system::put(root, "etc/group", group + user + ":x:" + id + ":\n", 0o644);
    system::boot_directory(root, c.init, "live-home", "/var/home/" + user, "0700", user);
    if d.display_manager != "sddm" { throw "no autologin for the display manager " + d.display_manager; }
    system::put(root, "etc/sddm.conf.d/live-autologin.conf",
        "[Autologin]\nUser=" + user + "\nSession=" + d.session + "\n", 0o644);
    stay_awake(root, d.session);
    if c.launcher != () { install_launcher(root, c); }
}

// "Install <OS>" in the live desktop's menu: the desktop's terminal runs the
// install screen (`c.launcher`) through the privilege tool, which asks for
// the live user's password; the entry's description says what it is. The
// variant's own install icon is still to be drawn; the icon theme's
// installer icon stands in.
fn install_launcher(root, c) {
    let d = c.desktop;
    let terminal = if d.session == "plasma" { "konsole -e " + d.privilege + " " + c.launcher }
        else if d.session == "lxqt" { "qterminal -e \"" + d.privilege + " " + c.launcher + "\"" }
        else { throw "no terminal for the session " + d.session };
    let entry = "[Desktop Entry]\nType=Application\nName=Install " + c.os_name + "\n";
    entry += "Comment=Install " + c.os_name + " on this computer (the password is \"" + d.password + "\")\n";
    entry += "Exec=" + terminal + "\nIcon=system-software-install\nTerminal=false\nCategories=System;\nKeywords=install;installer;setup;\n";
    system::put(root, "usr/share/applications/" + d.os_id + "-install.desktop", entry, 0o644);
}

// The live desktop session (`session`) neither locks, blanks nor suspends by
// itself, so an install running in it is never interrupted. Plasma: the
// screen locker's and PowerDevil's system defaults. LXQt: xscreensaver does
// not start (its autostart entry, replaced hidden) and the X server never
// blanks.
fn stay_awake(root, session) {
    if session == "plasma" {
        system::put(root, "etc/xdg/kscreenlockerrc", "[Daemon]\nAutolock=false\nLockOnResume=false\n", 0o644);
        let power = "";
        for source in ["AC", "Battery", "LowBattery"] {
            power += "[" + source + "][SuspendAndShutdown]\nAutoSuspendAction=0\n\n";
            power += "[" + source + "][Display]\nDimDisplayWhenIdle=false\nTurnOffDisplayWhenIdle=false\n\n";
        }
        system::put(root, "etc/xdg/powerdevilrc", power, 0o644);
    } else if session == "lxqt" {
        system::put(root, "etc/xdg/autostart/lxqt-xscreensaver-autostart.desktop",
            "[Desktop Entry]\nType=Application\nName=XScreenSaver\nExec=xscreensaver -no-splash\nHidden=true\n", 0o644);
        system::put(root, "etc/X11/xorg.conf.d/10-live-no-blank.conf",
            "Section \"ServerFlags\"\n    Option \"BlankTime\" \"0\"\n    Option \"StandbyTime\" \"0\"\n    Option \"SuspendTime\" \"0\"\n    Option \"OffTime\" \"0\"\nEndSection\n",
            0o644);
    } else {
        throw "no stay-awake settings for the session " + session;
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
    // Consoles: the install screen on each console the variant lists, and
    // root logged in without asking on every other one.
    for console in c.consoles {
        if !system::all_consoles().contains(console) { throw "unsupported install console " + console; }
    }
    // The display manager keeps its console on the desktop system.
    let desktop = c.desktop != ();
    let gettys = [];
    for console in system::login_consoles(desktop) {
        let login = if c.consoles.contains(console) { c.launcher } else { "root" };
        gettys.push([console, login]);
    }
    system::consoles(root, c.init, gettys);
    // Root has no password; the issue text says so under its banner.
    system::put(root, "etc/issue",
        "\n" + c.os_name + " " + c.label + " - \\l\n\n" + c.hint + "\n\n", 0o644);
    let shadow = system::root_password(input(c, "shadow"), "");
    if desktop {
        live_user(root, c);
        shadow += system::shadow_line(c.desktop.user, c.desktop.password_hash);
    }
    system::put(root, "etc/shadow", shadow, 0o600);
    system::put(root, c.role_file, c.role + "\n", 0o644);
    // The shell and its help.
    system::put(root, "etc/profile.d/20-live-shell.sh", input(c, "shell_ux"), 0o644);
    system::put(root, "usr/local/bin/live-help", input(c, "live_help"), 0o755);
    // The serial test protocol (recqemu's guest profile). A test-harness hook
    // that ships in the product until the harness switches it on through boot
    // injection.
    system::put(root, "etc/profile.d/99-serial-test.sh",
        input(c, "serial_profile"), 0o644);
    // Magic SysRq, and logs in RAM with a size cap.
    system::put(root, "etc/sysctl.d/50-live-sysrq.conf", "kernel.sysrq = 1\n", 0o644);
    system::put(root, "etc/fstab",
        "tmpfs /var/log tmpfs nosuid,nodev,noexec,size=64M,mode=0755 0 0\n", 0o644);
    // Services, and what they need.
    system::sshd_runtime_dir(root, c.init);
    for service in c.services { system::enable(root, c.init, service); }
    // Release the boot medium and its loop devices at shutdown.
    system::shutdown_hook(root, c.init, "live-shutdown-cleanup",
        input(c, "shutdown_cleanup"), c.live_media_dir);
    system::put(root, ".recipe-live-overlay", "1\n", 0o644);
    ctx.outputs = #{overlay: root}; ctx
}
fn remove(ctx) { if exists(ctx.root) { rm(ctx.root); } ctx.outputs = #{}; ctx }
