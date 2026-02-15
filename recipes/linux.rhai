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

/// LevitateOS invariant: all kernel artifacts MUST land under `.artifacts/out/<DistroDir>/...`.
///
/// The recipe writes to `<distro>/output/...` and relies on `<distro>/output` being a symlink:
/// `<distro>/output -> ../.artifacts/out/<DistroDir>`.
///
/// This helper enforces that, and will create the symlink if `output/` is missing.
fn _ensure_output_is_artifacts_out(distro_dir) {
    let output = join_path(distro_dir, "output");
    let distro_name = trim(shell_output("basename '" + distro_dir + "'"));
    let expected_rel = "../.artifacts/out/" + distro_name;
    // On fresh clones `.artifacts/out` may not exist yet. `readlink -f` fails when
    // the target path doesn't exist, so ensure the directory exists first.
    shell("cd '" + distro_dir + "' && mkdir -p '" + expected_rel + "'");
    let expected = trim(shell_output("cd '" + distro_dir + "' && readlink -f '" + expected_rel + "'"));

    if exists(output) {
        let is_link = trim(shell_output("[ -L '" + output + "' ] && echo yes || echo no"));
        if is_link != "yes" {
            throw "Refusing to install kernel outside `.artifacts/out`: `" + output + "` exists but is not a symlink. Expected: `<distro>/output -> ../.artifacts/out/" + distro_name + "`";
        }

        let actual = trim(shell_output("readlink -f '" + output + "'"));
        if actual != expected {
            throw "Refusing to install kernel outside `.artifacts/out`: `" + output + "` points to `" + actual + "`, expected `" + expected + "`";
        }

        return;
    }

    // Create the symlink (safe: `output` does not exist).
    shell("mkdir -p '" + expected + "'");
    shell("ln -s '../.artifacts/out/" + distro_name + "' '" + output + "'");
}

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

/// Read kernel.release from a completed build dir.
fn _kernel_release_or_throw(build_dir) {
    let rel_file = join_path(build_dir, "include/config/kernel.release");
    if !is_file(rel_file) {
        throw "Missing kernel.release at " + rel_file;
    }
    let rel = trim(read_file(rel_file));

    if KERNEL_VERSION != "" && !rel.starts_with(KERNEL_VERSION) {
        throw "Wrong kernel version: kernel.release '" + rel + "' does not start with '" + KERNEL_VERSION + "'";
    }
    if !rel.ends_with(KERNEL_LOCALVERSION) {
        throw "Wrong kernel localversion: kernel.release '" + rel + "' does not end with '" + KERNEL_LOCALVERSION + "'";
    }

    rel
}

// === OVERRIDES (enforce `.artifacts/out` + usrmerge-aware install detection) ===

fn is_built(ctx) {
    let distro_dir = dirname(BUILD_DIR);
    _ensure_output_is_artifacts_out(distro_dir);

    let build_dir = join_path(distro_dir, "output/kernel-build");
    let bzimage = join_path(build_dir, "arch/x86/boot/bzImage");

    if KERNEL_FORCE_REBUILD == "1" {
        throw "force rebuild requested";
    }

    if !is_file(bzimage) {
        throw "kernel not built";
    }

    // Ensure the build we found matches the distro-spec SSOT.
    let rel = _kernel_release_or_throw(build_dir);
    ctx.kernel_version = rel;
    ctx.build_dir = build_dir;
    ctx
}

fn build(ctx) {
    let distro_dir = dirname(BUILD_DIR);
    _ensure_output_is_artifacts_out(distro_dir);

    let kconfig = join_path(distro_dir, "kconfig");
    let output_dir = join_path(distro_dir, "output");
    let build_dir = join_path(output_dir, "kernel-build");
    let build_dir_tmp = join_path(output_dir, "kernel-build.tmp");

    if !is_file(kconfig) {
        throw "kconfig not found at " + kconfig;
    }

    // Build into a temp dir and swap on success so failures don't leave partial artifacts.
    shell("rm -rf '" + build_dir_tmp + "'");
    mkdir(build_dir_tmp);

    log("Building Linux kernel...");
    log("  Source: " + ctx.source_path);
    log("  Config: " + kconfig);
    log("  Output: " + build_dir);

    try {
        _apply_kconfig(ctx.source_path, build_dir_tmp, kconfig);

        let jobs = trim(shell_output("nproc"));
        shell("make -C " + ctx.source_path + " O=" + build_dir_tmp + " -j" + jobs);
    } catch (e) {
        // Best-effort cleanup of temp outputs.
        shell("rm -rf '" + build_dir_tmp + "'");
        throw e;
    }

    // Swap into place atomically-ish.
    shell("rm -rf '" + build_dir + "'");
    shell("mv '" + build_dir_tmp + "' '" + build_dir + "'");

    ctx.build_dir = build_dir;
    log("Kernel build complete");
    ctx
}

fn is_installed(ctx) {
    let distro_dir = dirname(BUILD_DIR);
    _ensure_output_is_artifacts_out(distro_dir);

    let staging = join_path(distro_dir, "output/staging");
    let vmlinuz = join_path(staging, "boot/vmlinuz");
    let build_dir = join_path(distro_dir, "output/kernel-build");
    let rel = _kernel_release_or_throw(build_dir);

    if KERNEL_FORCE_REBUILD == "1" {
        throw "force rebuild requested";
    }

    let modules = if MODULE_INSTALL_PATH == "/usr/lib/modules" {
        join_path(staging, "usr/lib/modules/" + rel)
    } else {
        join_path(staging, "lib/modules/" + rel)
    };

    if !is_file(vmlinuz) {
        throw "vmlinuz not installed";
    }
    if !is_dir(modules) {
        throw "modules not installed";
    }

    ctx
}

fn install(ctx) {
    let distro_dir = dirname(BUILD_DIR);
    _ensure_output_is_artifacts_out(distro_dir);

    let output_dir = join_path(distro_dir, "output");
    let staging = join_path(output_dir, "staging");
    let staging_tmp = join_path(output_dir, "staging.tmp");
    let build_dir = ctx.build_dir;

    let source_path = ctx.source_path;
    if source_path == "" {
        throw "No kernel source path available — cannot install modules";
    }

    log("Installing kernel to staging...");

    // Install into a temp dir and swap on success so failures don't leave partial artifacts.
    shell("rm -rf '" + staging_tmp + "'");
    mkdir(staging_tmp);

    try {
        mkdir(join_path(staging_tmp, "boot"));

        // Base install creates lib/modules; the usrmerge override relocates after install.
        mkdir(join_path(staging_tmp, "lib/modules"));

        // Copy vmlinuz
        let bzimage = join_path(build_dir, "arch/x86/boot/bzImage");
        let vmlinuz = join_path(staging_tmp, "boot/vmlinuz");
        shell("cp " + bzimage + " " + vmlinuz);
        log("  Installed vmlinuz");

        // Install modules via hookable helper
        log("  Installing modules...");
        _install_modules(staging_tmp, source_path, build_dir);

        // Get kernel version for cleanup
        let release_file = join_path(build_dir, "include/config/kernel.release");
        let version = if is_file(release_file) { trim(read_file(release_file)) } else { ctx.kernel_version };

        // Post-install cleanup via hookable helper
        _cleanup_modules(staging_tmp, version);
        log("  Modules installed");
    } catch (e) {
        // Best-effort cleanup of temp outputs.
        shell("rm -rf '" + staging_tmp + "'");
        throw e;
    }

    // Swap into place atomically-ish.
    shell("rm -rf '" + staging + "'");
    shell("mv '" + staging_tmp + "' '" + staging + "'");

    ctx
}
