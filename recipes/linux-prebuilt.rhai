// Kernel products use the owned target tree; imports select configuration explicitly.
const RECIPE_API = 2;
import "kernel/metadata.rhai" as kernel;

let build_deps = [];

let ctx = #{
    outputs: #{},
    target: KERNEL_TARGET,
    kconfig_path: join_path(RUNTIME.paths.inputs, "kconfig"),
    kconfig_fragments: join_path(RUNTIME.paths.inputs, "kconfig_fragments"),
    build_dir: "",
    description: "Prebuilt Linux kernel artifacts",
    kernel_localversion: "",
    kernel_version: "?",
    name: "linux-prebuilt",
    module_install_path: "/usr/lib/modules",
    source_path: "",
    tarball_filename: "",
    tarball_url: "",
};

fn _prebuilt_spec_or_throw(selected_target) {
    let target = kernel::target(selected_target);

    // Pinned release facts are independent of the source recipe's current version.
    // Keep this table explicit so callers do not discover URLs/hashes manually.
    if target == "levitate" {
        return #{
            distro_id: "levitate",
            repo: "LevitateOS/leviso",
            release_tag: "0.0.0-pre-alpha",
            asset_name: "levitate-6.12.116-levitate-runtime.tar.xz",
            asset_sha256: "a904236a538f488717fbb846ce359638676714585f08d98f4e27ebe3ad5c2c0b",
            expected_release: "6.12.116-levitate",
            localversion: "-levitate",
            module_install_path: "/usr/lib/modules",
            version: "6.12.116",
        };
    }
    if target == "acorn" {
        return #{
            distro_id: "acorn",
            repo: "LevitateOS/AcornOS",
            release_tag: "0.0.0-pre-alpha",
            asset_name: "acorn-7.3.3-acorn-runtime.tar.xz",
            asset_sha256: "c3634752d628695eba64130b0b342042a283b164d6fa16e0c871c125256aabfc",
            expected_release: "7.3.3-acorn",
            localversion: "-acorn",
            module_install_path: "/usr/lib/modules",
            version: "7.3.3",
        };
    }

    throw "No prebuilt kernel mapping for target '" + target + "'. " +
        "Remediation: publish a prebuilt runtime tarball and add its repo/tag/asset/sha in distro/builder/recipes/linux-prebuilt.rhai, " +
        "or keep using distro/builder/recipes/linux.rhai for source builds.";
}

fn _resolve_asset_url(spec) {
    "https://github.com/" + spec.repo + "/releases/download/" + spec.release_tag + "/" + spec.asset_name
}

fn _apply_prebuilt_spec(ctx) {
    let spec = _prebuilt_spec_or_throw(ctx.target);
    ctx.build_dir = "";
    ctx.description = "Prebuilt Linux kernel artifacts (" + spec.expected_release + ")";
    ctx.kernel_localversion = spec.localversion;
    ctx.kernel_version = spec.version;
    ctx.module_install_path = spec.module_install_path;
    ctx.source_path = "";
    ctx.tarball_filename = spec.asset_name;
    ctx.tarball_url = _resolve_asset_url(spec);
    ctx
}

fn _validate_payload_or_throw(kernel_root, expected_release, expected_version, expected_localversion, module_install_path) {
    let rel_file = join_path(kernel_root, "kernel-build/include/config/kernel.release");
    if !is_file(rel_file) {
        throw "Missing kernel.release at " + rel_file;
    }
    let rel = trim(read_file(rel_file));

    if rel != expected_release {
        throw "Wrong kernel.release: got '" + rel + "', expected '" + expected_release + "'";
    }
    if expected_version != "" && !rel.starts_with(expected_version) {
        throw "Wrong kernel version in kernel.release '" + rel + "' (expected prefix '" + expected_version + "')";
    }
    if !rel.ends_with(expected_localversion) {
        throw "Wrong kernel localversion in kernel.release '" + rel + "' (expected suffix '" + expected_localversion + "')";
    }

    let vmlinuz = join_path(kernel_root, "staging/boot/vmlinuz");
    if !is_file(vmlinuz) {
        throw "Missing staged kernel image at " + vmlinuz;
    }

    let modules = if module_install_path == "/usr/lib/modules" {
        join_path(kernel_root, "staging/usr/lib/modules/" + rel)
    } else {
        join_path(kernel_root, "staging/lib/modules/" + rel)
    };
    if !is_dir(modules) {
        throw "Missing staged kernel modules at " + modules;
    }

    rel
}

fn _payload_context(ctx, kernel_root, release, module_install_path) {
    ctx.kernel_version = release;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx.outputs = kernel::output_paths(kernel_root, release, module_install_path);
    ctx
}

fn is_acquired(ctx) {
    ctx = _apply_prebuilt_spec(ctx);
    let spec = _prebuilt_spec_or_throw(ctx.target);

    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    if !is_file(join_path(kernel_root, "kernel-build/include/config/kernel.release")) ||
       !is_file(join_path(kernel_root, "staging/boot/vmlinuz")) {
        return needs_work(ctx, "kernel payload is missing");
    }
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );

    ready(_payload_context(ctx, kernel_root, rel, spec.module_install_path))
}

fn acquire(ctx) {
    ctx = _apply_prebuilt_spec(ctx);
    let spec = _prebuilt_spec_or_throw(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let asset_url = _resolve_asset_url(spec);

    mkdir(ctx.runtime.paths.build);
    let tarball_path = join_path(ctx.runtime.paths.build, spec.asset_name);
    let extract_tmp = join_path(ctx.runtime.paths.build, "prebuilt.tmp");

    log("Downloading prebuilt kernel artifact...");
    log("  URL: " + asset_url);
    let supplied_archive = join_path(ctx.runtime.paths.inputs, "kernel_archive");
    if is_file(supplied_archive) {
        verify_sha256(supplied_archive, spec.asset_sha256);
        copy_file(supplied_archive, tarball_path);
    } else {
        download(asset_url, tarball_path, spec.asset_sha256);
    }

    log("Extracting prebuilt kernel artifact...");
    rm(extract_tmp);
    mkdir(extract_tmp);
    extract(tarball_path, extract_tmp);

    let rel = _validate_payload_or_throw(
        extract_tmp,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );

    // Publication happens only after the successful hook checkpoint. Keep the
    // previous coordinator-owned publication intact if this copy fails.
    for entry in ["kernel-build", "staging"] {
        let path = join_path(kernel_root, entry);
        if exists(path) { rm(path); }
    }
    copy_tree_contents(extract_tmp, kernel_root);
    rm(extract_tmp);

    // Validate again in canonical destination for explicit diagnostics.
    _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );

    ctx = _payload_context(ctx, kernel_root, rel, spec.module_install_path);
    ctx.tarball_url = asset_url;
    log("Prebuilt kernel artifact installed at " + kernel_root);
    ctx
}

fn is_built(ctx) {
    ctx = _apply_prebuilt_spec(ctx);
    let spec = _prebuilt_spec_or_throw(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    if !is_file(join_path(kernel_root, "kernel-build/include/config/kernel.release")) ||
       !is_file(join_path(kernel_root, "staging/boot/vmlinuz")) {
        return needs_work(ctx, "kernel payload is missing");
    }
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );
    ready(_payload_context(ctx, kernel_root, rel, spec.module_install_path))
}

fn build(ctx) {
    // Prebuilt recipe has no compile step; treat build as validated no-op.
    ctx = _apply_prebuilt_spec(ctx);
    let spec = _prebuilt_spec_or_throw(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );
    _payload_context(ctx, kernel_root, rel, spec.module_install_path)
}

fn is_installed(ctx) {
    ctx = _apply_prebuilt_spec(ctx);
    let spec = _prebuilt_spec_or_throw(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    if !is_file(join_path(kernel_root, "kernel-build/include/config/kernel.release")) ||
       !is_file(join_path(kernel_root, "staging/boot/vmlinuz")) {
        return needs_work(ctx, "kernel payload is missing");
    }
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );
    ready(_payload_context(ctx, kernel_root, rel, spec.module_install_path))
}

fn install(ctx) {
    // Prebuilt recipe already installs in acquire; install is a validated no-op.
    ctx = _apply_prebuilt_spec(ctx);
    let spec = _prebuilt_spec_or_throw(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );
    _payload_context(ctx, kernel_root, rel, spec.module_install_path)
}

fn cleanup(ctx, reason) {
    if reason == "auto.acquire.failure" {
        rm(join_path(ctx.runtime.paths.build, "prebuilt.tmp"));
    }
    // Remove downloaded prebuilt tarball after successful acquire.
    if reason == "auto.acquire.success" {
        let tarball = join_path(ctx.runtime.paths.build, ctx.tarball_filename);
        if is_file(tarball) {
            rm(tarball);
        }
    }
    ctx
}

fn remove(ctx) {
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    for name in ["kernel-build", "kernel-build.tmp", "staging", "staging.tmp"] {
        let path = join_path(kernel_root, name);
        if exists(path) {
            rm(path);
        }
    }
    ctx.build_dir = "";
    ctx.outputs = #{};
    ctx
}
