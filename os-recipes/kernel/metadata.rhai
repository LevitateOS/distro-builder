// The kernel pins and the kernel bundle rules the kernel recipes share (the
// source build's linux-source, linux-compile and linux, and linux-prebuilt). distro-builder's kernel module (src/pipeline/kernel.rs)
// also evaluates `source_spec` and `bundle_mismatch` (with `release_mismatch`),
// with only `join_path`, `is_file`, `is_dir`, `read_file_or_empty` and `trim`
// defined, so keep those functions to these helpers and plain Rhai.

// Each variant's kernel (KERNEL_TARGET is the variant id): the kernel line it
// follows (its distro.toml `[profile] kernel_line`), the exact kernel.org
// version, and the sha256 of that version's source tarball. A kernel bump
// edits only this table (and the prebuilt asset digest in linux-prebuilt.rhai).
fn pins() {
    #{
        levitate: #{
            line: "latest",
            version: "7.3.4",
            sha256: "b11b6df8295e8e991ff040b71aaa4994a8b55caa23b6b092907f195b5840fd1f",
        },
        acorn: #{
            line: "lts",
            version: "6.18.63",
            sha256: "81764a545072d160748beff4d9de2e7d95aa9f16e8261110288f067a2cc61d71",
        },
    }
}

// The pin of `target`, with the localversion (`-<target>`) and the exact
// kernel.release it builds.
fn source_spec(target) {
    let pins = pins();
    if !pins.contains(target) {
        throw "no kernel pin for KERNEL_TARGET '" + target + "' in kernel/metadata.rhai";
    }
    let spec = pins[target];
    spec["localversion"] = "-" + target;
    spec["release"] = spec.version + spec.localversion;
    spec
}

// What the kernel source of `target` is called and where it comes from: the
// description, exact version, source folder name, tarball name and URL. The
// caller joins `source_dir_name` to its build folder for the source path.
fn source_layout(target) {
    let spec = source_spec(target);
    let major = spec.version.split(".")[0];
    let tarball = "linux-" + spec.version + ".tar.xz";
    #{
        description: "Linux kernel (" + spec.version + ")",
        kernel_version: spec.version,
        source_dir_name: "linux-" + spec.version,
        tarball_filename: tarball,
        tarball_url: "https://cdn.kernel.org/pub/linux/kernel/v" + major + ".x/" + tarball,
    }
}

fn artifact_root(kernel_artifact_root) {
    if kernel_artifact_root == "" {
        throw "kernel artifact root is required";
    }
    if basename(kernel_artifact_root) == "" || kernel_artifact_root == "." ||
       kernel_artifact_root.contains("*") || kernel_artifact_root.contains("?") ||
       kernel_artifact_root.contains("[") || kernel_artifact_root.contains("]") {
        throw "unsafe kernel artifact root: '" + kernel_artifact_root + "'";
    }
    for component in kernel_artifact_root.split("/") {
        if component == ".." {
            throw "unsafe kernel artifact root: '" + kernel_artifact_root + "'";
        }
    }
    kernel_artifact_root
}

// The kernel bundle: kernel-build/include/config/kernel.release,
// staging/boot/vmlinuz and staging/usr/lib/modules/<release>/.

// Why kernel-build under `kernel_root` is not `spec`'s release, or "" when it is.
fn release_mismatch(kernel_root, spec) {
    let release_file = join_path(kernel_root, "kernel-build/include/config/kernel.release");
    if !is_file(release_file) {
        return "kernel release metadata is missing at " + release_file;
    }
    let release = trim(read_file_or_empty(release_file));
    if release != spec.release {
        return "Wrong kernel.release: found '" + release + "', pinned '" + spec.release + "'";
    }
    ""
}

// Why the bundle at `kernel_root` is not `spec`'s kernel, or "" when it is.
// Readiness reports this as work to do, so a new pin replaces the old kernel
// instead of failing on it; a fresh payload that fails it is rejected.
fn bundle_mismatch(kernel_root, spec) {
    let mismatch = release_mismatch(kernel_root, spec);
    if mismatch != "" { return mismatch; }
    let image = join_path(kernel_root, "staging/boot/vmlinuz");
    if !is_file(image) {
        return "kernel image is missing at " + image;
    }
    let modules = join_path(kernel_root, "staging/usr/lib/modules/" + spec.release);
    if !is_dir(modules) {
        return "kernel modules are missing at " + modules;
    }
    ""
}

// Named products shared by the source and prebuilt kernel lifecycles.
fn output_paths(kernel_root, release) {
    #{
        bundle: kernel_root,
        kernel: join_path(kernel_root, "staging/boot/vmlinuz"),
        modules: join_path(kernel_root, "staging/usr/lib/modules/" + release),
    }
}

// Every CONFIG_ line of `kconfig` whose value `config` (a kernel .config) does
// not carry, one "CONFIG_X=want (got ...)" per line; "" when it carries them
// all. `=n` is satisfied by an unset or unknown symbol.
fn kconfig_dropped(kconfig, config) {
    let have = #{};
    for line in read_file(config).split("\n") {
        if line.starts_with("CONFIG_") {
            let eq = line.index_of("=");
            have[line.sub_string(0, eq)] = line.sub_string(eq + 1);
        }
    }
    let dropped = "";
    for line in read_file(kconfig).split("\n") {
        let line = trim(line);
        if !line.starts_with("CONFIG_") { continue; }
        let eq = line.index_of("=");
        if eq < 0 { throw "kconfig line is not CONFIG_X=value: '" + line + "'"; }
        let name = line.sub_string(0, eq);
        let want = line.sub_string(eq + 1);
        let got = if have.contains(name) { have[name] } else { "unset" };
        let ok = if have.contains(name) { got == want } else { want == "n" };
        if !ok { dropped += "\n  " + line + " (got " + got + ")"; }
    }
    dropped
}
