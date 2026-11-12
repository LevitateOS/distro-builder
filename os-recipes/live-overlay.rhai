// The live-system policy: what the live session adds over the installed system
// (the rootfs below this overlay, which it never modifies). Each feature is
// stated once; the init backend in installed-system.rhai supplies the on-disk
// mechanism. The network is the installed system's: its own stack brings up
// every interface. On the desktop system the live session is still the
// installer: its display manager does not start, and the install screen has
// tty1 as on every live ISO; the desktop starts once the system is installed.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ config: "file", shadow: "file", shell_ux: "file", live_help: "file", shutdown_cleanup: "file", serial_profile: "file" }, outputs: #{ overlay: "directory" } };
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

// The desktop system under the live installer (`c.desktop`): the installed
// system starts its display manager at boot; the live session does not, so
// the install screen on tty1 is what the user meets. systemd boots
// multi-user.target instead of graphical.target. OpenRC boots a `live`
// runlevel: every service of the installed system's `default` runlevel
// (`c.desktop.default_runlevel`, which the builder read from the rootfs)
// except the display manager, which `rc-service <display manager> start`
// still starts by hand. Returns the runlevel the inittab ends in.
fn installer_desktop(root, c) {
    let dm = c.desktop.display_manager;
    if c.init == "systemd" {
        system::link(root, "etc/systemd/system/default.target", "/usr/lib/systemd/system/multi-user.target");
        return "default";
    }
    if !c.desktop.default_runlevel.contains(dm) {
        throw "the desktop system's default runlevel has no " + dm;
    }
    system::directory(root, "etc/runlevels/live", 0o755);
    for service in c.desktop.default_runlevel {
        if service != dm {
            system::link(root, "etc/runlevels/live/" + service, "/etc/init.d/" + service);
        }
    }
    "live"
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
    // Every console has a login, tty1 included: on the desktop system too the
    // live session does not start the display manager (`installer_desktop`).
    let runlevel = if c.desktop != () { installer_desktop(root, c) } else { "default" };
    let gettys = [];
    for console in system::login_consoles(false) {
        let login = if c.consoles.contains(console) { c.launcher } else { "root" };
        gettys.push([console, login]);
    }
    system::consoles_in_runlevel(root, c.init, gettys, runlevel);
    // Root has no password; the issue text says so under its banner.
    system::put(root, "etc/issue",
        "\n" + c.os_name + " " + c.label + " - \\l\n\n" + c.hint + "\n\n", 0o644);
    system::put(root, "etc/shadow", system::root_password(input(c, "shadow"), ""), 0o600);
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
