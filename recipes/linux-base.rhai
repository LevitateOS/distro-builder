// Linux kernel - shared base recipe
//
// Base recipe for all distros. Handles:
// - Tarball download + SHA256 verification (acquire)
// - Config application + compilation (build)
// - vmlinuz + modules installation (install)
// - Cleanup + removal
//
// Hookable helpers (override in child via //! extends:):
// - _apply_kconfig(src, build_dir, kconfig) — how kconfig is applied
// - _install_modules(staging, source_path, build_dir) — how modules are installed
// - _cleanup_modules(staging, version) — post-install module cleanup
//
// Canonical kernel source metadata lives here in recipe space. Orchestrators
// provide build topology like BUILD_DIR and kconfig/artifact paths, but they do
// not own kernel version/SHA/localversion/module layout facts.

let build_deps = ["linux-deps"];

let ctx = #{
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

fn _normalize_kernel_target(target) {
    let normalized = target.to_lower();
    if normalized == "levitate" || normalized == "leviso" {
        return "levitate";
    }
    if normalized == "acorn" || normalized == "acornos" {
        return "acorn";
    }
    if normalized == "iuppiter" || normalized == "iuppiteros" {
        return "iuppiter";
    }
    if normalized == "ralph" || normalized == "ralphos" {
        return "ralph";
    }
    ""
}

fn _kernel_target_or_throw(build_dir, kconfig_path) {
    let build_target = _normalize_kernel_target(basename(dirname(build_dir)));
    if build_target != "" {
        return build_target;
    }

    if kconfig_path != "" {
        let kconfig_target = _normalize_kernel_target(basename(dirname(kconfig_path)));
        if kconfig_target != "" {
            return kconfig_target;
        }
    }

    throw "Cannot determine kernel target from build dir '" + build_dir + "'";
}

fn _kernel_spec_or_throw(build_dir, kconfig_path) {
    let target = _kernel_target_or_throw(build_dir, kconfig_path);

    if target == "levitate" {
        return #{
            version: "6.12.71",
            sha256: "143e8bc76cc41f831b51aa5e75819bed55bed41f299d35922820f1d2d2b02600",
            localversion: "-levitate",
            module_install_path: "/usr/lib/modules",
        };
    }
    if target == "acorn" {
        return #{
            version: "6.19",
            sha256: "303079a8250b8f381f82b03f90463d12ac98d4f6b149b761ea75af1323521357",
            localversion: "-acorn",
            module_install_path: "/usr/lib/modules",
        };
    }
    if target == "iuppiter" {
        return #{
            version: "6.12.71",
            sha256: "143e8bc76cc41f831b51aa5e75819bed55bed41f299d35922820f1d2d2b02600",
            localversion: "-iuppiter",
            module_install_path: "/usr/lib/modules",
        };
    }
    if target == "ralph" {
        return #{
            version: "6.12.71",
            sha256: "143e8bc76cc41f831b51aa5e75819bed55bed41f299d35922820f1d2d2b02600",
            localversion: "-ralph",
            module_install_path: "/usr/lib/modules",
        };
    }

    throw "No kernel recipe spec registered for target '" + target + "'";
}

fn _apply_kernel_spec(ctx, build_dir, kconfig_path) {
    let spec = _kernel_spec_or_throw(build_dir, kconfig_path);
    let major = spec.version.split(".")[0];
    ctx.description = "Linux kernel (" + spec.version + ")";
    ctx.kernel_localversion = spec.localversion;
    ctx.kernel_version = spec.version;
    ctx.module_install_path = spec.module_install_path;
    ctx.source_dir_name = "linux-" + spec.version;
    ctx.tarball_filename = "linux-" + spec.version + ".tar.xz";
    ctx.tarball_url = "https://cdn.kernel.org/pub/linux/kernel/v" + major + ".x/linux-" + spec.version + ".tar.xz";
    ctx
}

// The executor persists `ctx` into the recipe file. Clear run-specific paths on
// load; canonical kernel metadata is re-derived inside each phase where
// BUILD_DIR is available.
ctx.source_path = "";
ctx.build_dir = "";

// === HOOKABLE HELPERS ===

/// Apply kconfig to build directory. Default: cp + olddefconfig.
fn _apply_kconfig(src, build_dir, kconfig) {
    // Optional: merge config fragments from <distro>/kconfig.d/*.config.
    // This keeps backwards compatibility with the single-file `kconfig` while
    // enabling reusable base configs + small per-distro overlays.
    let kconfig_dir = dirname(kconfig);
    let frag_dir = join_path(kconfig_dir, "kconfig.d");
    let fragments = if is_dir(frag_dir) { glob_list(join_path(frag_dir, "*.config")) } else { [] };

    if fragments.len() > 0 {
        let merge = join_path(src, "scripts/kconfig/merge_config.sh");
        if !is_file(merge) {
            throw "merge_config.sh not found at " + merge;
        }

        // merge_config.sh runs `make ... alldefconfig` and writes <build_dir>/.config.
        let cmd = "cd " + src + " && " + merge + " -O " + build_dir + " " + kconfig;
        for f in fragments {
            cmd += " " + f;
        }
        shell(cmd);
        return;
    }

    copy_file(kconfig, join_path(build_dir, ".config"));
    shell("make -C " + src + " O=" + build_dir + " olddefconfig");
}

/// Install modules to staging. Default: make modules_install to /lib/modules.
fn _install_modules(staging, source_path, build_dir) {
    shell("make -C " + source_path + " O=" + build_dir + " INSTALL_MOD_PATH=" + staging + " modules_install");
}

/// Clean up modules after install. Default: remove build/source symlinks.
fn _cleanup_modules(staging, version) {
    let modules_dir = join_path(staging, "lib/modules/" + version);
    if is_dir(modules_dir) {
        shell("rm -f " + join_path(modules_dir, "build") + " " + join_path(modules_dir, "source"));
    }
}

// === ACQUIRE ===

fn is_acquired(ctx) {
    ctx = _apply_kernel_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let tarball_source = join_path(BUILD_DIR, ctx.source_dir_name);
    if is_file(join_path(tarball_source, "Makefile")) {
        ctx.source_path = tarball_source;
        return ctx;
    }
    throw "linux source not acquired";
}

fn acquire(ctx) {
    ctx = _apply_kernel_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    mkdir(BUILD_DIR);
    let spec = _kernel_spec_or_throw(BUILD_DIR, KERNEL_KCONFIG_PATH);

    let tarball_path = join_path(BUILD_DIR, ctx.tarball_filename);
    let tarball_source = join_path(BUILD_DIR, ctx.source_dir_name);

    if !is_file(join_path(tarball_source, "Makefile")) {
        if !is_file(tarball_path) {
            log("Downloading kernel " + ctx.kernel_version + " from cdn.kernel.org...");
            shell("curl -fSL -o " + tarball_path + " " + ctx.tarball_url);
        }

        log("Verifying SHA256...");
        verify_sha256(tarball_path, spec.sha256);

        log("Extracting kernel source...");
        shell("tar xf " + tarball_path + " -C " + BUILD_DIR);
    }

    if is_file(join_path(tarball_source, "Makefile")) {
        ctx.source_path = tarball_source;
        log("Kernel source ready at " + tarball_source);
        return ctx;
    }

    throw "Failed to acquire linux source from cdn.kernel.org";
}

// === BUILD ===

fn is_built(ctx) {
    ctx = _apply_kernel_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let distro_dir = dirname(BUILD_DIR);
    let build_dir = join_path(distro_dir, "output/kernel-build");
    let bzimage = join_path(build_dir, "arch/x86/boot/bzImage");

    if !is_file(bzimage) {
        throw "kernel not built";
    }

    ctx.build_dir = build_dir;
    ctx
}

fn build(ctx) {
    ctx = _apply_kernel_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let distro_dir = dirname(BUILD_DIR);
    let canonical_kconfig = join_path(distro_dir, "build-host/kernel/kconfig");
    let legacy_kconfig = join_path(distro_dir, "kconfig");
    let kconfig = if KERNEL_KCONFIG_PATH != "" {
        KERNEL_KCONFIG_PATH
    } else if is_file(canonical_kconfig) {
        canonical_kconfig
    } else {
        legacy_kconfig
    };
    let build_dir = join_path(distro_dir, "output/kernel-build");

    if !is_file(kconfig) {
        throw "kconfig not found at " + kconfig;
    }

    mkdir(build_dir);

    log("Building Linux kernel...");
    log("  Source: " + ctx.source_path);
    log("  Config: " + kconfig);
    log("  Output: " + build_dir);

    _apply_kconfig(ctx.source_path, build_dir, kconfig);

    let jobs = trim(shell_output("nproc"));
    shell("make -C " + ctx.source_path + " O=" + build_dir + " -j" + jobs);

    ctx.build_dir = build_dir;
    log("Kernel build complete");
    ctx
}

// === INSTALL ===

fn is_installed(ctx) {
    ctx = _apply_kernel_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let distro_dir = dirname(BUILD_DIR);
    let staging = join_path(distro_dir, "output/staging");
    let vmlinuz = join_path(staging, "boot/vmlinuz");
    let modules = join_path(staging, "lib/modules");

    if !is_file(vmlinuz) {
        throw "vmlinuz not installed";
    }
    if !is_dir(modules) {
        throw "modules not installed";
    }

    ctx
}

fn install(ctx) {
    ctx = _apply_kernel_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let distro_dir = dirname(BUILD_DIR);
    let staging = join_path(distro_dir, "output/staging");
    let build_dir = ctx.build_dir;

    let source_path = ctx.source_path;
    if source_path == "" {
        throw "No kernel source path available — cannot install modules";
    }

    log("Installing kernel to staging...");

    mkdir(join_path(staging, "boot"));
    mkdir(join_path(staging, "lib/modules"));

    // Copy vmlinuz
    let bzimage = join_path(build_dir, "arch/x86/boot/bzImage");
    let vmlinuz = join_path(staging, "boot/vmlinuz");
    copy_file(bzimage, vmlinuz);
    log("  Installed vmlinuz");

    // Install modules via hookable helper
    log("  Installing modules...");
    _install_modules(staging, source_path, build_dir);

    // Get kernel version for cleanup
    let release_file = join_path(build_dir, "include/config/kernel.release");
    let version = if is_file(release_file) { trim(read_file(release_file)) } else { ctx.kernel_version };

    // Post-install cleanup via hookable helper
    _cleanup_modules(staging, version);
    log("  Modules installed");

    ctx
}

// === CLEANUP ===

fn cleanup(ctx, reason) {
    let downloaded = join_path(BUILD_DIR, ctx.source_dir_name);
    let tarball = join_path(BUILD_DIR, ctx.tarball_filename);

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

// === REMOVE ===

fn remove(ctx) {
    let distro_dir = dirname(BUILD_DIR);
    let build_dir = join_path(distro_dir, "output/kernel-build");
    let staging = join_path(distro_dir, "output/staging");

    if exists(build_dir) {
        log("Removing kernel build...");
        rm(build_dir);
    }

    let vmlinuz = join_path(staging, "boot/vmlinuz");
    if is_file(vmlinuz) {
        rm(vmlinuz);
    }

    ctx
}
