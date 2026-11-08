// The live-system policy: what the live session adds over the installed system
// (the rootfs below this overlay, which it never modifies). Each feature is
// stated once; the init backend in installed-system.rhai supplies the on-disk
// mechanism. The network is the installed system's: its own stack brings up
// every interface.
const RECIPE_API = 2;
import "installed-system.rhai" as system;
// The `config` input is distro_builder's `LiveOverlayConfig`, serialized; the
// builder's types guarantee its values (init is "systemd" or "openrc", the
// variant contract validated the service names).
let config = parse_json(read_file(join_path(RUNTIME.paths.inputs, "config")));
config.inputs = RUNTIME.paths.inputs;
// Where recinit's live init keeps the boot medium mounted (recinit::live::LIVE_MEDIA_DIR).
config.live_media_dir = "/run/live-media";
let ctx = #{name: "live-overlay", config: config,
    root: join_path(RUNTIME.paths.output, "overlay")};

// A name between two @ that is left after rendering is an unrendered placeholder.
fn is_placeholder(text) {
    if text == "" { return false; }
    let name = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_";
    for ch in text.chars() { if !name.contains(ch) { return false; } }
    true
}
// A guest/live asset with its @NAME@ placeholders filled in.
fn asset(name, config) {
    let values = #{OS_NAME: config.os_name, SHELL_READY_MARKER: config.shell_ready_marker,
        LIVE_MEDIA_DIR: config.live_media_dir};
    let text = read_file(join_path(config.inputs, "assets/live/" + name));
    for key in values.keys() { text = text.replace("@" + key + "@", values[key]); }
    let parts = text.split("@");
    for i in 1..(parts.len() - 1) {
        if is_placeholder(parts[i]) { throw "unrendered placeholder @" + parts[i] + "@ in live asset " + name; }
    }
    text
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
    let gettys = [];
    for console in system::all_consoles() {
        let login = if c.consoles.contains(console) { c.launcher } else { "root" };
        gettys.push([console, login]);
    }
    system::consoles(root, c.init, gettys);
    // Root has no password; the issue text says so.
    system::put(root, "etc/issue", c.issue, 0o644);
    system::put(root, "etc/shadow",
        system::root_password(read_file(join_path(c.inputs, "shadow")), ""), 0o600);
    system::put(root, c.role_file, c.role + "\n", 0o644);
    // The shell and its help.
    system::put(root, "etc/profile.d/20-live-shell.sh", asset("shell-ux.sh", c), 0o644);
    system::put(root, "usr/local/bin/live-help", asset("live-help.sh", c), 0o755);
    // The serial test protocol (recqemu's guest profile). A test-harness hook
    // that ships in the product until the harness switches it on through boot
    // injection.
    system::put(root, "etc/profile.d/99-serial-test.sh",
        read_file(join_path(c.inputs, "serial_profile")), 0o644);
    // Magic SysRq, and logs in RAM with a size cap.
    system::put(root, "etc/sysctl.d/50-live-sysrq.conf", "kernel.sysrq = 1\n", 0o644);
    system::put(root, "etc/fstab",
        "tmpfs /var/log tmpfs nosuid,nodev,noexec,size=64M,mode=0755 0 0\n", 0o644);
    // Services, and what they need.
    system::sshd_runtime_dir(root, c.init);
    for service in c.services { system::enable(root, c.init, service); }
    // Release the boot medium and its loop devices at shutdown.
    system::shutdown_hook(root, c.init, "live-shutdown-cleanup",
        asset("live-shutdown-cleanup.sh", c), c.live_media_dir);
    system::put(root, ".recipe-live-overlay", "1\n", 0o644);
    ctx.outputs = #{overlay: root}; ctx
}
fn remove(ctx) { if exists(ctx.root) { rm(ctx.root); } ctx.outputs = #{}; ctx }
