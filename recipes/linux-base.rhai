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
// Requires --define: KERNEL_VERSION, KERNEL_SHA256, KERNEL_LOCALVERSION

let build_deps = ["linux-deps"];

let major = KERNEL_VERSION.split(".")[0];
let ctx = #{
    description: "Linux kernel (" + KERNEL_VERSION + ")",
    name: "linux",
    tarball_url: "https://cdn.kernel.org/pub/linux/kernel/v" + major + ".x/linux-" + KERNEL_VERSION + ".tar.xz",
    tarball_filename: "linux-" + KERNEL_VERSION + ".tar.xz",
    source_dir_name: "linux-" + KERNEL_VERSION,
    kernel_version: KERNEL_VERSION,
    source_path: "",
    build_dir: "",
};

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

    shell("cp " + kconfig + " " + join_path(build_dir, ".config"));
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
    let tarball_source = join_path(BUILD_DIR, ctx.source_dir_name);
    if is_file(join_path(tarball_source, "Makefile")) {
        ctx.source_path = tarball_source;
        return ctx;
    }
    throw "linux source not acquired";
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);

    let tarball_path = join_path(BUILD_DIR, ctx.tarball_filename);
    let tarball_source = join_path(BUILD_DIR, ctx.source_dir_name);

    if !is_file(join_path(tarball_source, "Makefile")) {
        if !is_file(tarball_path) {
            log("Downloading kernel " + ctx.kernel_version + " from cdn.kernel.org...");
            shell("curl -fSL -o " + tarball_path + " " + ctx.tarball_url);
        }

        log("Verifying SHA256...");
        verify_sha256(tarball_path, KERNEL_SHA256);

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
    let distro_dir = dirname(BUILD_DIR);
    let kconfig = join_path(distro_dir, "kconfig");
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
    shell("cp " + bzimage + " " + vmlinuz);
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
