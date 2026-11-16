const RECIPE_API = 1;
// The variant's kernel built from the kernel.org source of the pin in
// kernel/metadata.rhai, with the `kconfig` input (distro-builder gives the
// shared kconfig followed by the variant's). Its one output is the kernel
// bundle (kernel/metadata.rhai: kernel-build/include/config/kernel.release,
// staging/boot/vmlinuz, staging/usr/lib/modules/<release>/); the builder
// checks the kernel and the modules inside it. The `target` input, the
// previous bundle, seeds the output: a kernel-build of the pinned release
// configured from the same kconfig is kept instead of compiled again. The
// source is chosen by KERNEL_TARGET and replaced by the `kernel_archive` input
// when given, so it is downloaded in build, not declared as SOURCES.
const NEEDS = #{
    inputs: #{ kconfig: "file", target: "directory?", kconfig_fragments: "directory?", kernel_archive: "file?" },
    defines: #{ KERNEL_TARGET: "levitate" },
    outputs: #{ bundle: "directory" },
    network: ["cdn.kernel.org:443"],
    memory_mib: 16384,
    storage_max_objects: 262144,
    timeout_secs: 14400,
};
const PACKAGE = #{ name: "linux", description: "Linux kernel" };
import "kernel/metadata.rhai" as kernel;

let build_deps = ["linux-deps"];

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

// Why kernel-build in `bundle` is not the pinned kernel configured from the
// kconfig input, or "" when it is.
fn _configured_mismatch(bundle, spec) {
    let mismatch = kernel::release_mismatch(bundle, spec);
    if mismatch != "" { return mismatch; }
    let configured = trim(read_file_or_empty(join_path(bundle, "kernel-build/.config.kconfig-hash")));
    if configured != _kconfig_identity(INPUTS.kconfig) {
        return "kernel configuration changed";
    }
    ""
}

// The pinned kernel source, from the `kernel_archive` input or kernel.org,
// checked against the pin and unpacked at `source_path` (in WORK).
fn _fetch_source(spec, layout, source_path) {
    let tarball = join_path(WORK, layout.tarball_filename);
    if INPUTS.kernel_archive != () {
        verify_sha256(INPUTS.kernel_archive, spec.sha256);
        tarball = INPUTS.kernel_archive;
    } else {
        log("Downloading kernel " + spec.version + " from cdn.kernel.org...");
        download(layout.tarball_url, tarball, spec.sha256);
    }

    log("Extracting kernel source...");
    let unpacked = join_path(WORK, "extract");
    rm(unpacked);
    mkdir(unpacked);
    extract(tarball, unpacked);
    let extracted = join_path(unpacked, layout.source_dir_name);
    if !is_file(join_path(extracted, "Makefile")) {
        throw "the kernel tarball " + layout.tarball_filename + " has no " + layout.source_dir_name + "/Makefile";
    }
    mv(extracted, source_path);
    rm(unpacked);
    // The tarball is not needed once it is unpacked.
    if INPUTS.kernel_archive == () { rm(tarball); }
    log("Kernel source ready at " + source_path);
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

// Compile the kernel into `build_dir`, a fresh folder.
fn _compile(source_path, build_dir, spec) {
    rm(build_dir);
    mkdir(build_dir);
    log("Building Linux kernel " + spec.release + "...");
    log("  Source: " + source_path);
    log("  Config: " + INPUTS.kconfig);
    log("  Output: " + build_dir);
    _apply_kconfig(source_path, build_dir, INPUTS.kconfig, spec);
    shell(_kernel_make_env_prefix() + "make -C " + source_path + " O=" + build_dir + " -j" + JOBS.to_string());
    let release = trim(read_file_or_empty(join_path(build_dir, "include/config/kernel.release")));
    if release != spec.release {
        throw "Wrong kernel.release: the build made '" + release + "', pinned '" + spec.release + "'";
    }
    log("Kernel build complete");
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

// `out` starts as a copy of the `target` input when one is given. A resumed
// run skips the download after checkpoint "source" and the compile after
// checkpoint "compiled".
fn build(src, out) {
    let target = DEFINES.KERNEL_TARGET;
    let spec = kernel::source_spec(target);
    let layout = kernel::source_layout(target);
    let source_path = join_path(WORK, layout.source_dir_name);
    let build_dir = join_path(out, "kernel-build");
    let staging = join_path(out, "staging");

    // The previous bundle (the `target` input) already holds this kernel:
    // kept, or only installed again. A resumed run never takes this path.
    let kept = !reached("source") && _configured_mismatch(out, spec) == "";
    if kept && kernel::bundle_mismatch(out, spec) == "" {
        log("The kernel " + spec.release + " is already built and installed from this kconfig");
        return;
    }
    let compiled = kept || reached("compiled");

    if !reached("source") {
        _fetch_source(spec, layout, source_path);
        checkpoint("source");
    }

    if !compiled {
        _compile(source_path, build_dir, spec);
        checkpoint("compiled");
    }

    let mismatch = kernel::release_mismatch(out, spec);
    if mismatch != "" { throw mismatch; }
    log("Installing kernel to staging...");
    rm(staging);
    mkdir(join_path(staging, "boot"));
    copy_file(join_path(build_dir, "arch/x86/boot/bzImage"), join_path(staging, "boot/vmlinuz"));
    log("  Installed vmlinuz");
    log("  Installing modules...");
    _install_modules(staging, source_path, build_dir, spec.release);
    log("  Modules installed");
}

// The bundle holds the pinned kernel image and its modules.
fn verify(out) {
    let spec = kernel::source_spec(DEFINES.KERNEL_TARGET);
    let mismatch = kernel::bundle_mismatch(out, spec);
    if mismatch != "" { throw mismatch; }
}
