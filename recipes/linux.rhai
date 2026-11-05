// Kernel products use the owned target tree; imports select configuration explicitly.
const RECIPE_API = 2;
import "kernel/metadata.rhai" as kernel;

let build_deps = ["linux-deps"];

let ctx = #{
    outputs: #{},
    target: KERNEL_TARGET,
    kconfig_path: join_path(RUNTIME.paths.inputs, "kconfig"),
    kconfig_fragments: join_path(RUNTIME.paths.inputs, "kconfig_fragments"),
    build_dir: "",
    description: "Linux kernel",
    kernel_version: "?",
    name: "linux",
    kernel_localversion: "",
    module_install_path: "/usr/lib/modules",
    source_dir_name: "",
    source_path: "",
    tarball_filename: "",
    tarball_url: "",
};

// AUTOFIX_PROMPT: This recipe is the SSOT for all kernel builds. Keep distros in parity.
// Only edit files under `distro-builder/recipes/` to fix the failure.
// Do NOT weaken lifecycle checks or correctness validation (kernel.release, artifacts-out enforcement, modules layout).
// Do NOT add `|| true` or otherwise ignore failures; fix the real issue.
// Keep changes minimal and local.

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
fn _kconfig_identity(kconfig, frag_dir) {
    let files = [kconfig];
    if is_dir(frag_dir) {
        for file in glob_list(join_path(frag_dir, "*.config")) { files.push(file); }
    }
    trim(exec_output("sha256sum", files))
}

fn _apply_kconfig(src, build_dir, kconfig, frag_dir, kernel_localversion) {
    let config_file = join_path(build_dir, ".config");
    let hash_file = join_path(build_dir, ".config.kconfig-hash");

    // Hash kconfig + fragments so we can avoid re-seeding defconfig on every run.
    let fragments = if is_dir(frag_dir) { glob_list(join_path(frag_dir, "*.config")) } else { [] };

    let kconfig_hash = _kconfig_identity(kconfig, frag_dir);
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

        // Enforce localversion from the canonical recipe spec. Kconfig may also
        // set CONFIG_LOCALVERSION, but this wins.
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

// === ACQUIRE ===

fn is_acquired(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let tarball_source = join_path(ctx.runtime.paths.build, ctx.source_dir_name);
    if is_file(join_path(tarball_source, "Makefile")) {
        ctx.source_path = tarball_source;
        return ready(ctx);
    }
    return needs_work(ctx, "linux source not acquired");
}

fn acquire(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    mkdir(ctx.runtime.paths.build);
    let spec = kernel::source_spec(ctx.target);

    let tarball_path = join_path(ctx.runtime.paths.build, ctx.tarball_filename);
    let supplied_archive = join_path(ctx.runtime.paths.inputs, "kernel_archive");
    if is_file(supplied_archive) {
        verify_sha256(supplied_archive, spec.sha256);
        copy_file(supplied_archive, tarball_path);
    }
    let tarball_source = join_path(ctx.runtime.paths.build, ctx.source_dir_name);

    if !is_file(join_path(tarball_source, "Makefile")) {
        if !is_file(tarball_path) {
            log("Downloading kernel " + ctx.kernel_version + " from cdn.kernel.org...");
            download(ctx.tarball_url, tarball_path, spec.sha256);
        } else {
            verify_sha256(tarball_path, spec.sha256);
        }

        log("Extracting kernel source...");
        extract(tarball_path, ctx.runtime.paths.build);
    }

    if is_file(join_path(tarball_source, "Makefile")) {
        ctx.source_path = tarball_source;
        log("Kernel source ready at " + tarball_source);
        return ctx;
    }

    throw "Failed to acquire linux source from cdn.kernel.org";
}

// === BUILD / INSTALL ===

fn is_built(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let build_dir = join_path(kernel_root, "kernel-build");
    let bzimage = join_path(build_dir, "arch/x86/boot/bzImage");

    if !is_file(bzimage) {
        return needs_work(ctx, "kernel not built");
    }

    if !is_file(join_path(build_dir, "include/config/kernel.release")) {
        return needs_work(ctx, "kernel release metadata is missing");
    }
    if !is_file(ctx.kconfig_path) { throw "kconfig input is missing"; }
    let identity = _kconfig_identity(ctx.kconfig_path, ctx.kconfig_fragments);
    if trim(read_file_or_empty(join_path(build_dir, ".config.kconfig-hash"))) != identity {
        return needs_work(ctx, "kernel configuration changed");
    }
    let rel = kernel::release(build_dir, spec.version, spec.localversion);
    ctx.kernel_version = rel;
    ctx.build_dir = build_dir;
    ready(ctx)
}

fn build(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    let kconfig = if ctx.kconfig_path == "" {
        throw "kconfig input is required";
    } else {
        ctx.kconfig_path
    };
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
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
        _apply_kconfig(ctx.source_path, build_dir_tmp, kconfig, ctx.kconfig_fragments, spec.localversion);

        let jobs = ctx.runtime.options.jobs.to_string();
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
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let staging = join_path(kernel_root, "staging");
    let vmlinuz = join_path(staging, "boot/vmlinuz");
    let build_dir = join_path(kernel_root, "kernel-build");
    if !is_file(join_path(build_dir, "include/config/kernel.release")) {
        return needs_work(ctx, "kernel release metadata is missing");
    }
    if !is_file(ctx.kconfig_path) { throw "kconfig input is missing"; }
    let identity = _kconfig_identity(ctx.kconfig_path, ctx.kconfig_fragments);
    if trim(read_file_or_empty(join_path(build_dir, ".config.kconfig-hash"))) != identity {
        return needs_work(ctx, "kernel configuration changed");
    }
    let rel = kernel::release(build_dir, spec.version, spec.localversion);


    let modules = if spec.module_install_path == "/usr/lib/modules" {
        join_path(staging, "usr/lib/modules/" + rel)
    } else {
        join_path(staging, "lib/modules/" + rel)
    };

    if !is_file(vmlinuz) {
        return needs_work(ctx, "vmlinuz not installed");
    }
    if !is_dir(modules) {
        return needs_work(ctx, "modules not installed");
    }

    ctx.kernel_version = rel;
    ctx.build_dir = build_dir;
    let source = join_path(ctx.runtime.paths.build, ctx.source_dir_name);
    ctx.source_path = if is_file(join_path(source, "Makefile")) { source } else { "" };
    ctx.outputs = kernel::output_paths(kernel_root, ctx.kernel_version, spec.module_install_path);
    ready(ctx)
}

fn install(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
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
        copy_file(bzimage, vmlinuz);
        log("  Installed vmlinuz");

        // Install modules via hookable helper
        log("  Installing modules...");
        _install_modules(staging_tmp, source_path, build_dir, spec.module_install_path);

        // Get kernel version for cleanup
        let release_file = join_path(build_dir, "include/config/kernel.release");
        let version = ctx.kernel_version;
        if is_file(release_file) {
            version = trim(read_file(release_file));
        }

        // Post-install cleanup via hookable helper
        _cleanup_modules(staging_tmp, version, spec.module_install_path);
        log("  Modules installed");
    } catch (e) {
        // Best-effort cleanup of temp outputs.
        shell("rm -rf '" + staging_tmp + "'");
        throw e;
    }

    // Swap into place atomically-ish.
    shell("rm -rf '" + staging + "'");
    shell("mv '" + staging_tmp + "' '" + staging + "'");

    ctx.outputs = kernel::output_paths(kernel_root, ctx.kernel_version, spec.module_install_path);
    ctx
}

// === CLEANUP ===

fn _kernel_cleanup_path(build_dir, name) {
    if name == "" || basename(name) != name || name == "." || name == ".." {
        throw "unsafe kernel cleanup filename: '" + name + "'";
    }
    let path = join_path(build_dir, name);
    // rm accepts glob patterns; cleanup owns exact paths only.
    if path.contains("*") || path.contains("?") || path.contains("[") || path.contains("]") {
        throw "unsafe kernel cleanup path: '" + path + "'";
    }
    path
}

fn cleanup(ctx, reason) {
    if reason != "manual" && reason != "auto.acquire.success" && reason != "auto.acquire.failure" {
        return ctx;
    }
    // Manual cleanup can run before any acquire/build hook initializes ctx.
    if ctx.source_dir_name == "" || ctx.tarball_filename == "" {
        ctx = kernel::apply_source_spec(ctx);
    }
    if !ctx.source_dir_name.starts_with("linux-") ||
       ctx.tarball_filename != ctx.source_dir_name + ".tar.xz" {
        throw "unsafe kernel cleanup: source and archive names do not identify a kernel source";
    }
    let downloaded = _kernel_cleanup_path(ctx.runtime.paths.build, ctx.source_dir_name);
    let tarball = _kernel_cleanup_path(ctx.runtime.paths.build, ctx.tarball_filename);

    // IMPORTANT:
    // - After acquire succeeds, the extracted source is still needed for build/install.
    // - After build succeeds, the extracted source is still needed for install (modules_install).
    // So automatic cleanup must be reason-aware.

    // Manual cleanup: full hygiene.
    if reason == "manual" {
        if is_dir(downloaded) {
            log("Removing downloaded kernel source...");
            rm(downloaded);
        }
        if is_file(tarball) {
            rm(tarball);
        }
        return ctx;
    }

    // Acquire succeeded: the tarball is no longer needed after extraction, but the source tree is.
    if reason == "auto.acquire.success" {
        if is_file(tarball) {
            rm(tarball);
        }
        return ctx;
    }

    // Acquire failed: remove any partial tarball/source so a retry starts clean.
    if reason == "auto.acquire.failure" {
        if is_dir(downloaded) {
            rm(downloaded);
        }
        if is_file(tarball) {
            rm(tarball);
        }
        return ctx;
    }

    // Build/install success or failure: keep sources by default (use manual cleanup if desired).
    ctx
}


fn remove(ctx) {
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    for name in ["kernel-build", "kernel-build.tmp", "staging", "staging.tmp"] {
        let path = join_path(kernel_root, name);
        if exists(path) {
            rm(path);
        }
    }
    ctx.build_dir = "";
    ctx.outputs = #{};
    ctx
}
