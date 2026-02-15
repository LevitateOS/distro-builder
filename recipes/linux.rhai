//! extends: linux-base.rhai
//
// Linux kernel - single SSOT recipe for all distros
//
// This recipe intentionally contains the *shared* logic so all distros stay in parity.
// Differences are injected via --define (from distro-spec):
// - KERNEL_VERSION, KERNEL_SHA256, KERNEL_LOCALVERSION
// - MODULE_INSTALL_PATH: "/lib/modules" (Alpine) or "/usr/lib/modules" (UsrMerge)
//
// Distro-specific knobs live in distro-spec + each distro's `kconfig`.

/// Apply kconfig: start from defconfig, apply CONFIG_* lines, enforce localversion, then olddefconfig.
///
/// This supports both:
/// - a single <distro>/kconfig file
/// - optional fragments in <distro>/kconfig.d/*.config (applied after kconfig)
fn _apply_kconfig(src, build_dir, kconfig) {
    let config_file = join_path(build_dir, ".config");
    let hash_file = join_path(build_dir, ".config.kconfig-hash");

    // Hash kconfig + fragments so we can avoid re-seeding defconfig on every run.
    let kconfig_dir = dirname(kconfig);
    let frag_dir = join_path(kconfig_dir, "kconfig.d");
    let fragments = if is_dir(frag_dir) { glob_list(join_path(frag_dir, "*.config")) } else { [] };

    let hash_cmd = " (sha256sum " + kconfig + " 2>/dev/null; " +
        "for f in " + join_path(frag_dir, "*.config") + "; do [ -f \"$f\" ] && sha256sum \"$f\"; done) " +
        " | sha256sum | cut -d' ' -f1 ";
    let kconfig_hash = trim(shell_output(hash_cmd));
    let cached_hash = trim(read_file_or_empty(hash_file));

    if cached_hash != kconfig_hash || !is_file(config_file) {
        log("Generating kernel config...");
        shell("make -C " + src + " O=" + build_dir + " x86_64_defconfig");

        log("Applying kconfig options...");
        let apply_one = |cfg| {
            // Apply CONFIG_* lines only; ignore comments/blank lines.
            let cmd = "grep -E '^CONFIG_' " + cfg + " | while read line; do " +
                src + "/scripts/config --file " + config_file + " --set-val $(echo $line | sed 's/=/ /'); " +
                "done";
            shell(cmd);
        };

        apply_one(kconfig);
        for f in fragments {
            apply_one(f);
        }

        // Enforce localversion from distro-spec (single SSOT).
        // Kconfig may also set CONFIG_LOCALVERSION, but this wins.
        shell(src + "/scripts/config --file " + config_file + " --set-str CONFIG_LOCALVERSION " + KERNEL_LOCALVERSION);

        log("Resolving config dependencies...");
        shell("make -C " + src + " O=" + build_dir + " olddefconfig");
        write_file(hash_file, kconfig_hash);
    } else {
        log("Config unchanged, running olddefconfig...");
        shell("make -C " + src + " O=" + build_dir + " olddefconfig");
    }
}

/// Install modules to staging, then normalize module dir layout.
///
/// The kernel build system always installs under <staging>/lib/modules when using
/// INSTALL_MOD_PATH. LevitateOS is UsrMerge so we relocate to /usr/lib/modules.
fn _install_modules(staging, source_path, build_dir) {
    shell("make -C " + source_path + " O=" + build_dir + " INSTALL_MOD_PATH=" + staging + " modules_install");

    if MODULE_INSTALL_PATH == "/usr/lib/modules" {
        let lib_modules = join_path(staging, "lib/modules");
        let usr_lib_modules = join_path(staging, "usr/lib/modules");

        if is_dir(lib_modules) {
            mkdir(usr_lib_modules);
            log("  UsrMerge: moving modules to usr/lib/modules...");
            shell("mv " + lib_modules + "/* " + usr_lib_modules + "/ 2>/dev/null || true");
            shell("rm -rf " + join_path(staging, "lib"));
        }
    }
}

/// Remove build/source symlinks after install.
fn _cleanup_modules(staging, version) {
    let base = if MODULE_INSTALL_PATH == "/usr/lib/modules" { "usr/lib/modules/" } else { "lib/modules/" };
    let modules_dir = join_path(staging, base + version);
    if is_dir(modules_dir) {
        shell("rm -f " + join_path(modules_dir, "build") + " " + join_path(modules_dir, "source"));
    }
}

