//! extends: linux-base.rhai
//
// Linux kernel - single SSOT recipe for all distros
//
// This recipe intentionally contains the *shared* logic so all distros stay in parity.
// Differences are injected via --define (from distro-spec):
// - KERNEL_VERSION, KERNEL_SHA256, KERNEL_LOCALVERSION
// - MODULE_INSTALL_PATH: "/lib/modules" (Alpine) or "/usr/lib/modules" (UsrMerge)
// - KERNEL_ARTIFACT_ROOT: canonical kernel artifact root directory
//
// Distro-specific knobs live in distro-spec + each distro's `kconfig`.

// AUTOFIX_PROMPT: This recipe is the SSOT for all kernel builds. Keep distros in parity.
// Only edit files under `distro-builder/recipes/` to fix the failure.
// Do NOT weaken lifecycle checks or correctness validation (kernel.release, artifacts-out enforcement, modules layout).
// Do NOT add `|| true` or otherwise ignore failures; fix the real issue.
// Keep changes minimal and local.

/// Resolve canonical kernel artifact root.
///
/// Required define:
/// - KERNEL_ARTIFACT_ROOT
///
/// This recipe no longer supports `<distro>/output` symlink indirection.
fn _kernel_artifact_root_or_throw(kernel_artifact_root) {
    if kernel_artifact_root == "" {
        throw "KERNEL_ARTIFACT_ROOT is required";
    }
    mkdir(kernel_artifact_root);
    kernel_artifact_root
}

/// Environment prefix for kernel make invocations.
///
/// Resolve BISON_PKGDATADIR from the active `bison` on PATH so this works
/// with either host tools or `linux-deps` extracted tools.
fn _kernel_make_env_prefix() {
    let env = "";
    let bison = "";
    if shell_status("command -v bison >/dev/null 2>&1") == 0 {
        bison = trim(shell_output("command -v bison"));
    }
    if bison != "" {
        let bison_pkgdatadir = join_path(dirname(dirname(bison)), "share/bison");
        if is_dir(bison_pkgdatadir) {
            env += "BISON_PKGDATADIR='" + bison_pkgdatadir + "' ";
        }
    }

    // Prefer m4 next to bison. Some packaged bison binaries use a hardcoded
    // fallback m4 path that may not exist on the host.
    let m4 = "";
    if bison != "" {
        let sibling_m4 = join_path(dirname(bison), "m4");
        if is_file(sibling_m4) {
            m4 = sibling_m4;
        }
    }
    if m4 == "" && shell_status("command -v m4 >/dev/null 2>&1") == 0 {
        m4 = trim(shell_output("command -v m4"));
    }
    if m4 != "" {
        env += "M4='" + m4 + "' ";
    }

    // Host tools (objtool, scripts/*) need libelf headers/libs.
    if bison != "" {
        let tools_usr = dirname(dirname(bison));
        let include_dir = join_path(tools_usr, "include");
        if is_dir(include_dir) {
            env += "C_INCLUDE_PATH='" + include_dir + "' ";
        }

        let lib_search_path = "";
        let lib64_dir = join_path(tools_usr, "lib64");
        if is_dir(lib64_dir) {
            lib_search_path = lib64_dir;
        }
        let lib_dir = join_path(tools_usr, "lib");
        if is_dir(lib_dir) {
            if lib_search_path != "" {
                lib_search_path += ":";
            }
            lib_search_path += lib_dir;
        }
        if lib_search_path != "" {
            env += "LIBRARY_PATH='" + lib_search_path + "' ";
            env += "LD_LIBRARY_PATH='" + lib_search_path + "' ";
        }
    }

    env
}

/// Apply kconfig: start from defconfig, apply CONFIG_* lines, enforce localversion, then olddefconfig.
///
/// This supports both:
/// - a single <distro>/kconfig file
/// - optional fragments in <distro>/kconfig.d/*.config (applied after kconfig)
fn _apply_kconfig(src, build_dir, kconfig, kernel_localversion) {
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
        shell(_kernel_make_env_prefix() + "make -C " + src + " O=" + build_dir + " x86_64_defconfig");

        log("Applying kconfig options...");
        let apply_one = |cfg| {
            // Apply CONFIG_* lines only; ignore comments/blank lines.
            let cmd = "grep -E '^CONFIG_' " + cfg;
            cmd += " | while read line; do ";
            cmd += src + "/scripts/config --file " + config_file + " --set-val $(echo $line | sed 's/=/ /'); ";
            cmd += "done";
            shell(cmd);
        };

        apply_one.call(kconfig);
        for f in fragments {
            apply_one.call(f);
        }

        // Enforce localversion from distro-spec (single SSOT).
        // Kconfig may also set CONFIG_LOCALVERSION, but this wins.
        //
        // NOTE: recipe defines are injected as scope constants by the executor. In Rhai,
        // scope constants are visible to the top-level phase functions, but not to helper
        // functions called by those phase functions. So pass the define value in from
        // `build(ctx)` rather than referencing it here.
        shell(src + "/scripts/config --file " + config_file + " --set-str CONFIG_LOCALVERSION " + kernel_localversion);

        log("Resolving config dependencies...");
        shell(_kernel_make_env_prefix() + "make -C " + src + " O=" + build_dir + " olddefconfig");
        write_file(hash_file, kconfig_hash);
    } else {
        log("Config unchanged, running olddefconfig...");
        shell(_kernel_make_env_prefix() + "make -C " + src + " O=" + build_dir + " olddefconfig");
    }
}

/// Install modules to staging, then normalize module dir layout.
///
/// The kernel build system always installs under <staging>/lib/modules when using
/// INSTALL_MOD_PATH. LevitateOS is UsrMerge so we relocate to /usr/lib/modules.
fn _install_modules(staging, source_path, build_dir, module_install_path) {
    shell(_kernel_make_env_prefix() + "make -C " + source_path + " O=" + build_dir + " INSTALL_MOD_PATH=" + staging + " modules_install");

    if module_install_path == "/usr/lib/modules" {
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
fn _cleanup_modules(staging, version, module_install_path) {
    let base = if module_install_path == "/usr/lib/modules" { "usr/lib/modules/" } else { "lib/modules/" };
    let modules_dir = join_path(staging, base + version);
    if is_dir(modules_dir) {
        shell("rm -f " + join_path(modules_dir, "build") + " " + join_path(modules_dir, "source"));
    }
}

/// Read kernel.release from a completed build dir.
fn _kernel_release_or_throw(build_dir, expected_version, expected_localversion) {
    let rel_file = join_path(build_dir, "include/config/kernel.release");
    if !is_file(rel_file) {
        throw "Missing kernel.release at " + rel_file;
    }
    let rel = trim(read_file(rel_file));

    if expected_version != "" && !rel.starts_with(expected_version) {
        throw "Wrong kernel version: kernel.release '" + rel + "' does not start with '" + expected_version + "'";
    }
    if !rel.ends_with(expected_localversion) {
        throw "Wrong kernel localversion: kernel.release '" + rel + "' does not end with '" + expected_localversion + "'";
    }

    rel
}

// === OVERRIDES (explicit kernel root + usrmerge-aware install detection) ===

fn is_built(ctx) {
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let build_dir = join_path(kernel_root, "kernel-build");
    let bzimage = join_path(build_dir, "arch/x86/boot/bzImage");

    if KERNEL_FORCE_REBUILD == "1" {
        throw "force rebuild requested";
    }

    if !is_file(bzimage) {
        throw "kernel not built";
    }

    // Ensure the build we found matches the distro-spec SSOT.
    let rel = _kernel_release_or_throw(build_dir, KERNEL_VERSION, KERNEL_LOCALVERSION);
    ctx.kernel_version = rel;
    ctx.build_dir = build_dir;
    ctx
}

fn build(ctx) {
    let kconfig = if KERNEL_KCONFIG_PATH == "" {
        throw "KERNEL_KCONFIG_PATH is required";
    } else {
        KERNEL_KCONFIG_PATH
    };
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let build_dir = join_path(kernel_root, "kernel-build");
    let build_dir_tmp = join_path(kernel_root, "kernel-build.tmp");

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
        _apply_kconfig(ctx.source_path, build_dir_tmp, kconfig, KERNEL_LOCALVERSION);

        let jobs = trim(shell_output("nproc"));
        shell(_kernel_make_env_prefix() + "make -C " + ctx.source_path + " O=" + build_dir_tmp + " -j" + jobs);
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
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let staging = join_path(kernel_root, "staging");
    let vmlinuz = join_path(staging, "boot/vmlinuz");
    let build_dir = join_path(kernel_root, "kernel-build");
    let rel = _kernel_release_or_throw(build_dir, KERNEL_VERSION, KERNEL_LOCALVERSION);

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
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let staging = join_path(kernel_root, "staging");
    let staging_tmp = join_path(kernel_root, "staging.tmp");
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
        _install_modules(staging_tmp, source_path, build_dir, MODULE_INSTALL_PATH);

        // Get kernel version for cleanup
        let release_file = join_path(build_dir, "include/config/kernel.release");
        let version = ctx.kernel_version;
        if is_file(release_file) {
            version = trim(read_file(release_file));
        }

        // Post-install cleanup via hookable helper
        _cleanup_modules(staging_tmp, version, MODULE_INSTALL_PATH);
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
