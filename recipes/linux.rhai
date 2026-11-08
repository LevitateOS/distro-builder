// The variant's kernel built from the kernel.org source of the pin in
// kernel/metadata.rhai, with the `kconfig` input (distro-builder gives the
// shared kconfig followed by the variant's). Kernel products use the owned
// target tree.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ kconfig: "file", target: "directory?", kconfig_fragments: "directory?", kernel_archive: "file?" }, outputs: #{ bundle: "directory", kernel: "file", modules: "directory" }, network: ["cdn.kernel.org:443"] };
import "kernel/metadata.rhai" as kernel;

let build_deps = ["linux-deps"];

let ctx = #{
    outputs: #{},
    target: if is_def_var("KERNEL_TARGET") { KERNEL_TARGET } else { "levitate" },
    kconfig_path: join_path(RUNTIME.paths.inputs, "kconfig"),
    build_dir: "",
    description: "Linux kernel",
    kernel_version: "",
    name: "linux",
    source_dir_name: "",
    source_path: "",
    tarball_filename: "",
    tarball_url: "",
};

// AUTOFIX_PROMPT: This recipe is the SSOT for all kernel builds. Keep distros in parity.
// Only edit files under `distro/builder/recipes/` to fix the failure.
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

// The identity of the kconfig a build was configured from.
fn _kconfig_identity(kconfig) {
    trim(exec_output("sha256sum", [kconfig]))
}

/// Configure `build_dir`: x86_64_defconfig, then the kconfig and the pinned
/// localversion appended (a later assignment wins), then olddefconfig. Every
/// value the kconfig sets must survive olddefconfig.
fn _apply_kconfig(src, build_dir, kconfig, spec) {
    for line in read_file(kconfig).split("\n") {
        if trim(line).starts_with("CONFIG_LOCALVERSION=") {
            throw "kconfig sets CONFIG_LOCALVERSION; the kernel recipe derives it from KERNEL_TARGET ('" + spec.localversion + "')";
        }
    }
    let config = join_path(build_dir, ".config");
    log("Generating kernel config...");
    shell(_kernel_make_env_prefix() + "make -C " + src + " O=" + build_dir + " x86_64_defconfig");
    append_file(config, "\n" + read_file(kconfig) + "\nCONFIG_LOCALVERSION=\"" + spec.localversion + "\"\n");
    log("Resolving config dependencies...");
    shell(_kernel_make_env_prefix() + "make -C " + src + " O=" + build_dir + " olddefconfig");
    let dropped = kernel::kconfig_dropped(kconfig, config);
    if dropped != "" {
        throw "olddefconfig dropped kconfig values (an unmet dependency, or not in Linux " + spec.version + "):" + dropped;
    }
    write_file(join_path(build_dir, ".config.kconfig-hash"), _kconfig_identity(kconfig));
}

/// Install the modules into `staging`: the kernel installs them under
/// lib/modules, and both distros keep them in /usr/lib/modules.
fn _install_modules(staging, source_path, build_dir, release) {
    shell(_kernel_make_env_prefix() + "make -C " + source_path + " O=" + build_dir + " INSTALL_MOD_PATH=" + staging + " modules_install");
    mkdir(join_path(staging, "usr/lib"));
    mv(join_path(staging, "lib/modules"), join_path(staging, "usr/lib/modules"));
    rm(join_path(staging, "lib"));
    // The build and source links point into this build; drop the links only.
    let modules_dir = join_path(staging, "usr/lib/modules/" + release);
    shell("rm -f '" + join_path(modules_dir, "build") + "' '" + join_path(modules_dir, "source") + "'");
}

// Replace `path` with the complete tree `incoming` by rename: the old tree is
// moved aside only once the new one is complete beside it.
fn _swap_in(incoming, path) {
    let old = path + ".old";
    rm(old);
    if exists(path) { mv(path, old); }
    mv(incoming, path);
    rm(old);
}

// === ACQUIRE ===

// The source tree is renamed into place only once it is completely extracted
// from a tarball whose sha256 matched the pin.
fn is_acquired(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    if is_file(join_path(ctx.source_path, "Makefile")) {
        return ready(ctx);
    }
    needs_work(ctx, "linux source not acquired")
}

fn acquire(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    mkdir(ctx.runtime.paths.build);

    let tarball_path = join_path(ctx.runtime.paths.build, ctx.tarball_filename);
    let supplied_archive = join_path(ctx.runtime.paths.inputs, "kernel_archive");
    if is_file(supplied_archive) {
        verify_sha256(supplied_archive, spec.sha256);
        copy_file(supplied_archive, tarball_path);
    } else if is_file(tarball_path) {
        verify_sha256(tarball_path, spec.sha256);
    } else {
        log("Downloading kernel " + spec.version + " from cdn.kernel.org...");
        download(ctx.tarball_url, tarball_path, spec.sha256);
    }

    log("Extracting kernel source...");
    let extract_tmp = ctx.source_path + ".extract";
    rm(extract_tmp);
    mkdir(extract_tmp);
    extract(tarball_path, extract_tmp);
    let extracted = join_path(extract_tmp, ctx.source_dir_name);
    if !is_file(join_path(extracted, "Makefile")) {
        throw "the kernel tarball " + ctx.tarball_filename + " has no " + ctx.source_dir_name + "/Makefile";
    }
    _swap_in(extracted, ctx.source_path);
    rm(extract_tmp);
    log("Kernel source ready at " + ctx.source_path);
    ctx
}

// === BUILD / INSTALL ===

fn is_built(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let build_dir = join_path(kernel_root, "kernel-build");
    if !is_file(join_path(build_dir, "arch/x86/boot/bzImage")) {
        return needs_work(ctx, "kernel not built");
    }
    let mismatch = _configured_mismatch(ctx, kernel_root, spec);
    if mismatch != "" { return needs_work(ctx, mismatch); }
    ctx.kernel_version = spec.release;
    ctx.build_dir = build_dir;
    ready(ctx)
}

// Why kernel-build is not the pinned kernel configured from the kconfig input,
// or "" when it is.
fn _configured_mismatch(ctx, kernel_root, spec) {
    let mismatch = kernel::release_mismatch(kernel_root, spec);
    if mismatch != "" { return mismatch; }
    if !is_file(ctx.kconfig_path) { throw "kconfig input is missing"; }
    let configured = trim(read_file_or_empty(join_path(kernel_root, "kernel-build/.config.kconfig-hash")));
    if configured != _kconfig_identity(ctx.kconfig_path) {
        return "kernel configuration changed";
    }
    ""
}

fn build(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    if !is_file(ctx.kconfig_path) { throw "kconfig input is missing"; }
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let build_dir = join_path(kernel_root, "kernel-build");
    let build_dir_tmp = join_path(kernel_root, "kernel-build.tmp");

    // Build into a temp dir and swap it in on success.
    rm(build_dir_tmp);
    mkdir(build_dir_tmp);

    log("Building Linux kernel " + spec.release + "...");
    log("  Source: " + ctx.source_path);
    log("  Config: " + ctx.kconfig_path);
    log("  Output: " + build_dir);

    try {
        _apply_kconfig(ctx.source_path, build_dir_tmp, ctx.kconfig_path, spec);
        let jobs = ctx.runtime.options.jobs.to_string();
        shell(_kernel_make_env_prefix() + "make -C " + ctx.source_path + " O=" + build_dir_tmp + " -j" + jobs);
        let release = trim(read_file_or_empty(join_path(build_dir_tmp, "include/config/kernel.release")));
        if release != spec.release {
            throw "Wrong kernel.release: the build made '" + release + "', pinned '" + spec.release + "'";
        }
    } catch (e) {
        rm(build_dir_tmp);
        throw e;
    }
    _swap_in(build_dir_tmp, build_dir);

    ctx.kernel_version = spec.release;
    ctx.build_dir = build_dir;
    log("Kernel build complete");
    ctx
}

fn is_installed(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let mismatch = _configured_mismatch(ctx, kernel_root, spec);
    if mismatch == "" { mismatch = kernel::bundle_mismatch(kernel_root, spec); }
    if mismatch != "" { return needs_work(ctx, mismatch); }

    ctx.kernel_version = spec.release;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    if !is_file(join_path(ctx.source_path, "Makefile")) { ctx.source_path = ""; }
    ctx.outputs = kernel::output_paths(kernel_root, spec.release);
    ready(ctx)
}

fn install(ctx) {
    ctx = kernel::apply_source_spec(ctx);
    let spec = kernel::source_spec(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let build_dir = join_path(kernel_root, "kernel-build");
    let staging = join_path(kernel_root, "staging");
    let staging_tmp = join_path(kernel_root, "staging.tmp");
    if !is_file(join_path(ctx.source_path, "Makefile")) {
        throw "No kernel source at " + ctx.source_path + " — cannot install modules";
    }
    let mismatch = kernel::release_mismatch(kernel_root, spec);
    if mismatch != "" { throw mismatch; }

    log("Installing kernel to staging...");
    // Install into a temp dir and swap it in on success.
    rm(staging_tmp);
    mkdir(join_path(staging_tmp, "boot"));
    try {
        copy_file(join_path(build_dir, "arch/x86/boot/bzImage"), join_path(staging_tmp, "boot/vmlinuz"));
        log("  Installed vmlinuz");
        log("  Installing modules...");
        _install_modules(staging_tmp, ctx.source_path, build_dir, spec.release);
        log("  Modules installed");
    } catch (e) {
        rm(staging_tmp);
        throw e;
    }
    _swap_in(staging_tmp, staging);

    ctx.kernel_version = spec.release;
    ctx.build_dir = build_dir;
    ctx.outputs = kernel::output_paths(kernel_root, spec.release);
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
        let extracting = downloaded + ".extract";
        if is_dir(extracting) {
            rm(extracting);
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
    for name in ["kernel-build", "kernel-build.tmp", "kernel-build.old", "staging", "staging.tmp", "staging.old"] {
        let path = join_path(kernel_root, name);
        if exists(path) {
            rm(path);
        }
    }
    ctx.build_dir = "";
    ctx.outputs = #{};
    ctx
}
