const RECIPE_API = 1;
// The rootfs producer: the system recipe both package backends share, the
// second half of making a system tree. A backend recipe starts with
// `//! extends: rootfs-producer.rhai`, declares its NEEDS (with the `rootfs`
// output) and sets its init system as a literal constant, which `build`
// reads: `const INIT_SYSTEM = "systemd";`.
//
// The `system` input is the package-effects step's tree (`package-effects.rhai`
// and its backends: the locked packages composed, their effects applied, the
// target metadata finalized), one archive in `target_pack_tree`'s format. This
// recipe copies it, the installed-system policy (`installed-system.rhai`)
// makes it the installable system, and the tree must have every command in
// the `capabilities` input (distro/contract/capabilities.txt) before it counts
// as built. Recipe's identity covers this file, the backend, their imports
// and every input, so a change to any of them starts from an empty output; a
// configuration change reruns this step only, not the packages'.
const NEEDS = #{};
import "installed-system.rhai" as installed;

// What a built tree must have: every shared command on PATH, and what the
// install tools need on the target (recpart's mkfs.fat, bootctl's EFI binary).
// The first one missing, or "".
fn missing(root) {
    let needed = [];
    for line in read_file(INPUTS.capabilities).split("\n") {
        let line = trim(line);
        if line.starts_with("command ") { needed.push(trim(line.sub_string(8))); }
    }
    for command in needed + ["mkfs.fat"] {
        let found = false;
        for dir in ["usr/bin", "usr/sbin", "bin", "sbin"] {
            if exists(join_path(root, dir + "/" + command)) { found = true; break; }
        }
        if !found { return "the command " + command; }
    }
    let efi = "usr/lib/systemd/boot/efi/systemd-bootx64.efi";
    if !is_file(join_path(root, efi)) { return efi; }
    ""
}

fn build(src, out) {
    // The package tree, as the package-effects step left it: packages
    // composed, their effects applied, metadata finalized, caches and logs
    // dropped, auditd's log directory made.
    progress(0, 0, "copying the package tree");
    target_unpack_tree(INPUTS.system, out);
    progress(0, 0, "system configuration");
    // The installed-system policy reads its inputs (os_release, branding,
    // ...) by name from INPUTS.
    installed::apply(out, INIT_SYSTEM);
    progress(0, 0, "finalizing the configured tree");
    target_finalize(out);
}

// The capability check: the configured tree has every command and file the
// install needs.
fn verify(out) {
    let gap = missing(out);
    if gap != "" { throw "the built rootfs lacks " + gap; }
}
