//! extends: linux-base.rhai
//
// Linux kernel - Alpine-based distros (AcornOS, IuppiterOS)
//
// Extends linux-base.rhai with:
// - Kernel theft from leviso (DEV ONLY — steals pre-built kernel instead of compiling)
// - Theft-aware is_acquired/acquire/is_built/build/install phases
//
// When leviso has already built the kernel, we steal it via symlink.
// Source tarball is always downloaded (needed for make modules_install even in theft mode).

let major = KERNEL_VERSION.split(".")[0];
let ctx = #{
    description: "Linux kernel (" + KERNEL_VERSION + ")",
    name: "linux",
    tarball_url: "https://cdn.kernel.org/pub/linux/kernel/v" + major + ".x/linux-" + KERNEL_VERSION + ".tar.xz",
    tarball_filename: "linux-" + KERNEL_VERSION + ".tar.xz",
    source_dir_name: "linux-" + KERNEL_VERSION,
    kernel_version: KERNEL_VERSION,
    source_path: "",
    leviso_kernel_path: "",
    build_dir: "",
    stolen: false,
};

// === ACQUIRE (override: adds theft detection) ===

fn is_acquired(ctx) {
    let distro_dir = dirname(BUILD_DIR);
    let monorepo = dirname(distro_dir);

    // Check for tarball-extracted source (needed for modules_install even in theft mode)
    let tarball_source = join_path(BUILD_DIR, ctx.source_dir_name);
    if is_file(join_path(tarball_source, "Makefile")) {
        ctx.source_path = tarball_source;
    } else {
        throw "linux source not acquired";
    }

    // Check for leviso's pre-built kernel (theft mode — DEV ONLY)
    let leviso_kernel = join_path(monorepo, "leviso/output/kernel-build");
    let leviso_bzimage = join_path(leviso_kernel, "arch/x86/boot/bzImage");
    if is_file(leviso_bzimage) {
        ctx.leviso_kernel_path = leviso_kernel;
        ctx.stolen = true;
    }

    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);

    let distro_dir = dirname(BUILD_DIR);
    let monorepo = dirname(distro_dir);

    // Primary: download tarball from cdn.kernel.org
    // (needed even in theft mode for make modules_install)
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
    }

    // Check for leviso's pre-built kernel (theft mode — DEV ONLY)
    let leviso_kernel = join_path(monorepo, "leviso/output/kernel-build");
    let leviso_bzimage = join_path(leviso_kernel, "arch/x86/boot/bzImage");
    if is_file(leviso_bzimage) {
        log("DEV ONLY: Found pre-built kernel from LevitateOS — will steal it!");
        ctx.leviso_kernel_path = leviso_kernel;
        ctx.stolen = true;
        return ctx;
    }

    if ctx.source_path != "" {
        log("Kernel source ready at " + tarball_source);
        return ctx;
    }

    throw "Failed to acquire linux source from cdn.kernel.org";
}

// === BUILD (override: adds theft shortcut) ===

fn is_built(ctx) {
    if ctx.stolen {
        return ctx;
    }

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
    // If stealing from leviso, just symlink
    if ctx.stolen {
        log("STEALING KERNEL FROM LEVITATEOS");

        let distro_dir = dirname(BUILD_DIR);
        let our_build = join_path(distro_dir, "output/kernel-build");

        if exists(our_build) {
            rm(our_build);
        }

        mkdir(join_path(distro_dir, "output"));
        shell("ln -s " + ctx.leviso_kernel_path + " " + our_build);
        log("Created symlink: output/kernel-build -> " + ctx.leviso_kernel_path);

        ctx.build_dir = our_build;
        return ctx;
    }

    // Build kernel from source (delegates to base _apply_kconfig)
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

// === INSTALL (override: theft-aware build_dir) ===

fn install(ctx) {
    let distro_dir = dirname(BUILD_DIR);
    let staging = join_path(distro_dir, "output/staging");
    let build_dir = if ctx.stolen { ctx.leviso_kernel_path } else { ctx.build_dir };

    let source_path = ctx.source_path;
    if source_path == "" {
        throw "No kernel source path available — cannot install modules";
    }

    log("Installing kernel to staging...");

    mkdir(join_path(staging, "boot"));
    mkdir(join_path(staging, "lib/modules"));

    let bzimage = join_path(build_dir, "arch/x86/boot/bzImage");
    let vmlinuz = join_path(staging, "boot/vmlinuz");
    shell("cp " + bzimage + " " + vmlinuz);
    log("  Installed vmlinuz");

    log("  Installing modules...");
    _install_modules(staging, source_path, build_dir);
    log("  Modules installed");

    ctx
}

// is_installed, cleanup, remove — inherited from linux-base.rhai
