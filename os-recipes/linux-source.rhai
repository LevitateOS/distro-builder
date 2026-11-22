const RECIPE_API = 1;
// The kernel.org source of the pin in kernel/metadata.rhai, unpacked: the first
// of the three recipes that build the kernel (linux-source, linux-compile,
// linux). Its one output holds the source tree as `linux-<version>/`
// (kernel::source_layout's `source_dir_name`). The tarball is chosen by
// KERNEL_TARGET and replaced by the `kernel_archive` input when given (checked
// against the same pin), so it is downloaded in build, not declared as SOURCES.
// Every step here is a Recipe helper, so it runs in the minimal image.
const NEEDS = #{
    inputs: #{ kernel_archive: "file?" },
    defines: #{ KERNEL_TARGET: "levitate" },
    outputs: #{ source: "directory" },
    network: ["cdn.kernel.org:443"],
    memory_mib: 4096,
    storage_max_objects: 262144,
    runtime: "minimal",
};
const PACKAGE = #{ name: "linux-source", description: "Linux kernel source" };
import "kernel/metadata.rhai" as kernel;

fn build(src, out) {
    let target = DEFINES.KERNEL_TARGET;
    let spec = kernel::source_spec(target);
    let layout = kernel::source_layout(target);

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
    let source_path = join_path(out, layout.source_dir_name);
    rm(source_path);
    mv(extracted, source_path);
    rm(unpacked);
    // The tarball is not needed once it is unpacked.
    if INPUTS.kernel_archive == () { rm(tarball); }
    log("Kernel source ready at " + source_path);
}

// The output holds the pinned version's source tree.
fn verify(out) {
    let layout = kernel::source_layout(DEFINES.KERNEL_TARGET);
    let makefile = join_path(join_path(out, layout.source_dir_name), "Makefile");
    if !is_file(makefile) {
        throw "the kernel source is missing at " + makefile;
    }
}
