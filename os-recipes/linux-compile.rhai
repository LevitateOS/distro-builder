const RECIPE_API = 1;
// The variant's kernel configured and compiled: the second of the three
// recipes that build the kernel (linux-source, linux-compile, linux), the one
// that takes hours. It configures the source linux-source unpacked with the
// `kconfig` input (distro-builder gives the shared kconfig followed by the
// variant's) and compiles it. Its one output holds the build tree as
// `kernel-build/`, laid out as in the kernel bundle (kernel/metadata.rhai), with
// `.config.kconfig-hash` naming the kconfig it was configured from. Recipe keeps
// the finished output by its identity, so a later run whose install fails does
// not compile again. The `target` input, the previous kernel bundle, seeds the
// output: a kernel-build of the pinned release configured from the same kconfig
// is kept instead of compiled again.
const NEEDS = #{
    inputs: #{ kconfig: "file", target: "directory?", kconfig_fragments: "directory?" },
    defines: #{ KERNEL_TARGET: "levitate" },
    outputs: #{ build: "directory" },
    memory_mib: 16384,
    storage_max_objects: 262144,
    timeout_secs: 14400,
};
const PACKAGE = #{ name: "linux-compile", description: "Linux kernel build tree" };
import "kernel/metadata.rhai" as kernel;
import "kernel/make.rhai" as make;

let build_deps = ["linux-deps", "linux-source"];

// The identity of the kconfig a build was configured from.
fn _kconfig_identity(kconfig) {
    trim(exec_output("sha256sum", [kconfig]))
}

// Why kernel-build in `root` is not the pinned kernel configured from the
// kconfig input, or "" when it is.
fn _configured_mismatch(root, spec) {
    let mismatch = kernel::release_mismatch(root, spec);
    if mismatch != "" { return mismatch; }
    let configured = trim(read_file_or_empty(join_path(root, "kernel-build/.config.kconfig-hash")));
    if configured != _kconfig_identity(INPUTS.kconfig) {
        return "kernel configuration changed";
    }
    ""
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
    shell(make::env_prefix() + "make -C " + src + " O=" + build_dir + " x86_64_defconfig");
    append_file(config, "\n" + read_file(kconfig) + "\nCONFIG_LOCALVERSION=\"" + spec.localversion + "\"\n");
    log("Resolving config dependencies...");
    shell(make::env_prefix() + "make -C " + src + " O=" + build_dir + " olddefconfig");
    let dropped = kernel::kconfig_dropped(kconfig, config);
    if dropped != "" {
        throw "olddefconfig dropped kconfig values (an unmet dependency, or not in Linux " + spec.version + "):" + dropped;
    }
    write_file(join_path(build_dir, ".config.kconfig-hash"), _kconfig_identity(kconfig));
}

// Compile the kernel into `build_dir`, a fresh folder. The source stays
// read-only (an O= build writes only into `build_dir`).
fn _compile(source_path, build_dir, spec) {
    rm(build_dir);
    mkdir(build_dir);
    log("Building Linux kernel " + spec.release + "...");
    log("  Source: " + source_path);
    log("  Config: " + INPUTS.kconfig);
    log("  Output: " + build_dir);
    _apply_kconfig(source_path, build_dir, INPUTS.kconfig, spec);
    shell(make::env_prefix() + "make -C " + source_path + " O=" + build_dir + " -j" + JOBS.to_string());
    let release = trim(read_file_or_empty(join_path(build_dir, "include/config/kernel.release")));
    if release != spec.release {
        throw "Wrong kernel.release: the build made '" + release + "', pinned '" + spec.release + "'";
    }
    log("Kernel build complete");
}

// `out` starts as a copy of the `target` input when one is given.
fn build(src, out) {
    let target = DEFINES.KERNEL_TARGET;
    let spec = kernel::source_spec(target);
    let layout = kernel::source_layout(target);
    // Only the build tree is this recipe's; linux installs staging again.
    rm(join_path(out, "staging"));

    // The previous bundle (the `target` input) already holds this build.
    if _configured_mismatch(out, spec) == "" {
        log("The kernel " + spec.release + " is already compiled from this kconfig");
        return;
    }

    let source_path = join_path(DEPS["linux-source"], layout.source_dir_name);
    _compile(source_path, join_path(out, "kernel-build"), spec);
}

// The build tree is the pinned kernel configured from the kconfig input
// (verify starts no program, so the recorded digest is checked with
// verify_sha256 instead of sha256sum).
fn verify(out) {
    let spec = kernel::source_spec(DEFINES.KERNEL_TARGET);
    let mismatch = kernel::release_mismatch(out, spec);
    if mismatch != "" { throw mismatch; }
    let configured = trim(read_file_or_empty(join_path(out, "kernel-build/.config.kconfig-hash")));
    verify_sha256(INPUTS.kconfig, configured.split(" ")[0]);
}
