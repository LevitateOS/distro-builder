const RECIPE_API = 1;
// The variant's kernel built from the kernel.org source of the pin in
// kernel/metadata.rhai, in three recipes: linux-source unpacks the source,
// linux-compile configures it with the `kconfig` input and compiles it, and
// this one installs the kernel image and the modules into staging. Its one
// output is the kernel bundle (kernel/metadata.rhai:
// kernel-build/include/config/kernel.release, staging/boot/vmlinuz,
// staging/usr/lib/modules/<release>/); the builder checks the kernel and the
// modules inside it. Recipe keeps each recipe's finished output by its
// identity, so an interrupted run resumes after the last finished one. The
// `target` input, the previous bundle, seeds the output: when it already holds
// the build linux-compile made, installed, it is kept.
const NEEDS = #{
    inputs: #{ target: "directory?" },
    defines: #{ KERNEL_TARGET: "levitate" },
    outputs: #{ bundle: "directory" },
    memory_mib: 16384,
    storage_max_objects: 262144,
};
const PACKAGE = #{ name: "linux", description: "Linux kernel" };
import "kernel/metadata.rhai" as kernel;
import "kernel/make.rhai" as make;

let build_deps = ["linux-deps", "linux-source", "linux-compile"];

// The kconfig identity linux-compile recorded in the kernel-build under `root`.
fn _configured_from(root) {
    trim(read_file_or_empty(join_path(root, "kernel-build/.config.kconfig-hash")))
}

/// Install the modules into `staging`: the kernel installs them under
/// lib/modules, and both distros keep them in /usr/lib/modules.
fn _install_modules(staging, source_path, build_dir, release) {
    shell(make::env_prefix() + "make -C " + source_path + " O=" + build_dir + " INSTALL_MOD_PATH=" + staging + " modules_install");
    mkdir(join_path(staging, "usr/lib"));
    mv(join_path(staging, "lib/modules"), join_path(staging, "usr/lib/modules"));
    rm(join_path(staging, "lib"));
    // The build and source links point into this build; drop the links only.
    let modules_dir = join_path(staging, "usr/lib/modules/" + release);
    shell("rm -f '" + join_path(modules_dir, "build") + "' '" + join_path(modules_dir, "source") + "'");
}

// `out` starts as a copy of the `target` input when one is given.
fn build(src, out) {
    let target = DEFINES.KERNEL_TARGET;
    let spec = kernel::source_spec(target);
    let layout = kernel::source_layout(target);
    let compiled = DEPS["linux-compile"];
    let mismatch = kernel::release_mismatch(compiled, spec);
    if mismatch != "" { throw mismatch; }

    // The previous bundle (the `target` input) already holds this build,
    // installed.
    let configured = _configured_from(compiled);
    if configured != "" && _configured_from(out) == configured && kernel::bundle_mismatch(out, spec) == "" {
        log("The kernel " + spec.release + " is already built and installed from this kconfig");
        return;
    }

    let source_path = join_path(DEPS["linux-source"], layout.source_dir_name);
    let build_dir = join_path(out, "kernel-build");
    let staging = join_path(out, "staging");
    // The bundle carries the build tree; modules_install writes into it, so
    // it is copied out of the read-only dependency output with its times
    // (make compares them).
    rm(build_dir);
    shell("cp -RP --preserve=mode,timestamps,links '" + join_path(compiled, "kernel-build") + "' '" + build_dir + "'");
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
