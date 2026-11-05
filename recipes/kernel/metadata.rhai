// Shared kernel source facts, path guards, and release checks. No lifecycle hooks.

fn target(value) {
    if value == "levitate" || value == "acorn" { return value; }
    throw "KERNEL_TARGET must be levitate or acorn";
}

fn source_spec(selected_target) {
    let target = target(selected_target);

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

    throw "No kernel recipe spec registered for target '" + target + "'";
}

fn apply_source_spec(ctx) {
    let spec = source_spec(ctx.target);
    let major = spec.version.split(".")[0];
    ctx.description = "Linux kernel (" + spec.version + ")";
    ctx.kernel_localversion = spec.localversion;
    ctx.kernel_version = spec.version;
    ctx.module_install_path = spec.module_install_path;
    ctx.source_dir_name = "linux-" + spec.version;
    ctx.tarball_filename = "linux-" + spec.version + ".tar.xz";
    ctx.tarball_url = "https://cdn.kernel.org/pub/linux/kernel/v" + major + ".x/" + ctx.tarball_filename;
    ctx.source_path = join_path(ctx.runtime.paths.build, ctx.source_dir_name);
    ctx
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

fn release(build_dir, expected_version, expected_localversion) {
    let rel_file = join_path(build_dir, "include/config/kernel.release");
    if !is_file(rel_file) {
        throw "Missing kernel.release at " + rel_file;
    }
    let rel = trim(read_file(rel_file));

    if expected_version != "" && !rel.starts_with(expected_version) {
        throw "Wrong kernel version: kernel.release '" + rel + "' does not start with '" + expected_version + "'";
    }
    if !rel.ends_with(expected_localversion) {
        throw "Wrong kernel localversion: kernel.release '" + rel + "' does not end with '" + expected_localversion + "'";
    }

    rel
}

// Product names are shared by source and prebuilt kernel lifecycles. Rust
// selects/validates named products; it does not reconstruct package paths.
fn output_paths(kernel_root, release, module_install_path) {
    let modules = if module_install_path == "/usr/lib/modules" {
        "staging/usr/lib/modules/"
    } else if module_install_path == "/lib/modules" {
        "staging/lib/modules/"
    } else {
        throw "unsupported module installation path: " + module_install_path;
    };
    let outputs = #{bundle: kernel_root, kernel: join_path(kernel_root, "staging/boot/vmlinuz"),
                    modules: join_path(kernel_root, modules + release)};
    outputs
}
