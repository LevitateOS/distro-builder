const RECIPE_API = 1;
// The live-system policy: what the live session adds over the installed system
// (the rootfs below this overlay, which it never modifies). Each feature is
// stated once; the init backend in installed-system.rhai supplies the on-disk
// mechanism. Every live medium is the base system with no display manager;
// the builder adds the `network` group's tree (recipes/network.rhai and its
// deps, NetworkManager enabled by its own configuration) to this overlay
// beside what this recipe writes, so the live session brings up every
// interface.
const NEEDS = #{ inputs: #{ config: "file", shadow: "file", shell_ux: "file", live_help: "file", shutdown_cleanup: "file", serial_profile: "file" }, outputs: #{ overlay: "directory" } };
const PACKAGE = #{ name: "live-overlay" };
import "installed-system.rhai" as system;

// The rendered guest file the builder passed as input `name`.
fn input(name) { read_file(INPUTS[name]) }

fn build(src, out) {
    // The `config` input is distro_builder's `LiveSession` (pipeline/overlay.rs),
    // serialized; the builder's types guarantee its values (init is "systemd" or
    // "openrc", the variant contract validated the service names). The guest files
    // arrive rendered, each as its own input.
    let c = parse_json(read_file(INPUTS.config));
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
    system::consoles(out, c.init, gettys);
    // Root has no password; the issue text says so under its banner.
    put(out, "etc/issue",
        "\n" + c.os_name + " " + c.label + " - \\l\n\n" + c.hint + "\n\n", 0o644);
    put(out, "etc/shadow", system::root_password(input("shadow"), ""), 0o600);
    put(out, c.role_file, c.role + "\n", 0o644);
    // The shell and its help.
    put(out, "etc/profile.d/20-live-shell.sh", input("shell_ux"), 0o644);
    put(out, "usr/local/bin/live-help", input("live_help"), 0o755);
    // The serial test protocol (recqemu's guest profile). A test-harness hook
    // that ships in the product until the harness switches it on through boot
    // injection.
    put(out, "etc/profile.d/99-serial-test.sh", input("serial_profile"), 0o644);
    // Magic SysRq, and logs in RAM with a size cap.
    put(out, "etc/sysctl.d/50-live-sysrq.conf", "kernel.sysrq = 1\n", 0o644);
    put(out, "etc/fstab",
        "tmpfs /var/log tmpfs nosuid,nodev,noexec,size=64M,mode=0755 0 0\n", 0o644);
    // Services, and what they need.
    system::sshd_runtime_dir(out, c.init);
    for service in c.services { system::enable(out, c.init, service); }
    // Release the boot medium and its loop devices at shutdown.
    system::shutdown_hook(out, c.init, "live-shutdown-cleanup",
        input("shutdown_cleanup"), c.live_media_dir);
}
