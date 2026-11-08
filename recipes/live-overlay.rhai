// The live-system policy: what the live session adds over the installed system
// (the rootfs below this overlay, which it never modifies). Each feature is
// stated once; the init backend in installed-system.rhai supplies the on-disk
// mechanism. The network is the installed system's: its own stack brings up
// every interface.
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
    let gettys = [];
    for console in system::all_consoles() {
        let login = if c.consoles.contains(console) { c.launcher } else { "root" };
        gettys.push([console, login]);
    }
    system::consoles(root, c.init, gettys);
    // Root has no password; the issue text says so under its banner.
    system::put(root, "etc/issue",
        "\n" + c.os_name + " " + c.label + " - \\l\n\n" + c.hint + "\n\n", 0o644);
    system::put(root, "etc/shadow",
        system::root_password(input(c, "shadow"), ""), 0o600);
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
    if c.display_manager != () { system::graphical(root, c.init, c.display_manager); }
    // Release the boot medium and its loop devices at shutdown.
    system::shutdown_hook(root, c.init, "live-shutdown-cleanup",
        input(c, "shutdown_cleanup"), c.live_media_dir);
    system::put(root, ".recipe-live-overlay", "1\n", 0o644);
    ctx.outputs = #{overlay: root}; ctx
}
fn remove(ctx) { if exists(ctx.root) { rm(ctx.root); } ctx.outputs = #{}; ctx }
