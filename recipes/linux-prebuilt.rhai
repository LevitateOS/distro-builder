// Linux kernel - prebuilt artifact recipe (separate from source-build flow)
//
// This recipe installs prebuilt kernel artifacts from GitHub Releases into
// KERNEL_ARTIFACT_ROOT, preserving the canonical layout expected by builder:
// - kernel-build/include/config/kernel.release
// - staging/boot/vmlinuz
// - staging/{usr/lib,lib}/modules/<kernel.release>/
//
// Required defines:
// - KERNEL_VERSION
// - KERNEL_LOCALVERSION
// - KERNEL_ARTIFACT_ROOT
// - MODULE_INSTALL_PATH
//
// Optional:
// - KERNEL_FORCE_REBUILD=1 forces re-download/reinstall

let build_deps = [];

let ctx = #{
    build_dir: "/data/vince/LevitateOS/.artifacts/kernel/levitate/current/kernel-build",
    description: "Prebuilt Linux kernel artifacts",
    kernel_version: "6.12.71-levitate",
    name: "linux-prebuilt",
    source_path: "",
    tarball_filename: "levitate-6.12.71-levitate-runtime.tar.xz",
    tarball_url: "https://github.com/LevitateOS/leviso/releases/download/0.0.0-pre-alpha/levitate-6.12.71-levitate-runtime.tar.xz",
};

fn _kernel_artifact_root_or_throw(kernel_artifact_root) {
    if kernel_artifact_root == "" {
        throw "KERNEL_ARTIFACT_ROOT is required";
    }
    mkdir(kernel_artifact_root);
    kernel_artifact_root
}

fn _prebuilt_spec_or_throw(localversion) {
    // Keep this table explicit so no caller has to discover URLs/hashes manually.
    if localversion == "-levitate" {
        return #{
            distro_id: "leviso",
            repo: "LevitateOS/leviso",
            release_tag: "0.0.0-pre-alpha",
            asset_name: "levitate-6.12.71-levitate-runtime.tar.xz",
            asset_sha256: "064ec622912a8741967c92b4cdf523eca4311bc9179d8ef1049beaf95453a2c8",
            expected_release: "6.12.71-levitate",
        };
    }
    if localversion == "-acorn" {
        return #{
            distro_id: "acorn",
            repo: "LevitateOS/AcornOS",
            release_tag: "0.0.0-pre-alpha",
            asset_name: "acorn-6.19.0-acorn-runtime.v2.tar.xz",
            asset_sha256: "04e10c6392680b75b80a09beb9036e5954b62cfabddd4cb0fe3c975c0a79e96e",
            expected_release: "6.19.0-acorn",
        };
    }

    throw "No prebuilt kernel mapping for localversion '" + localversion + "'. " +
        "Remediation: publish a prebuilt runtime tarball and add its repo/tag/asset/sha in distro-builder/recipes/linux-prebuilt.rhai.";
}

fn _resolve_asset_url(spec) {
    "https://github.com/" + spec.repo + "/releases/download/" + spec.release_tag + "/" + spec.asset_name
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

fn is_acquired(ctx) {
    let spec = _prebuilt_spec_or_throw(KERNEL_LOCALVERSION);
    if KERNEL_FORCE_REBUILD == "1" {
        throw "forced prebuilt refresh requested";
    }

    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        KERNEL_VERSION,
        KERNEL_LOCALVERSION,
        MODULE_INSTALL_PATH
    );

    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx.tarball_filename = spec.asset_name;
    ctx.tarball_url = _resolve_asset_url(spec);
    ctx
}

fn acquire(ctx) {
    let spec = _prebuilt_spec_or_throw(KERNEL_LOCALVERSION);
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let asset_url = _resolve_asset_url(spec);

    mkdir(BUILD_DIR);
    let tarball_path = join_path(BUILD_DIR, spec.asset_name);
    let extract_tmp = kernel_root + ".prebuilt.tmp";

    log("Downloading prebuilt kernel artifact...");
    log("  URL: " + asset_url);
    shell("curl -fSL -o '" + tarball_path + "' '" + asset_url + "'");

    log("Verifying SHA256...");
    verify_sha256(tarball_path, spec.asset_sha256);

    log("Extracting prebuilt kernel artifact...");
    shell("rm -rf '" + extract_tmp + "'");
    mkdir(extract_tmp);
    shell("tar xf '" + tarball_path + "' -C '" + extract_tmp + "'");

    let rel = _validate_payload_or_throw(
        extract_tmp,
        spec.expected_release,
        KERNEL_VERSION,
        KERNEL_LOCALVERSION,
        MODULE_INSTALL_PATH
    );

    shell("rm -rf '" + kernel_root + "'");
    shell("mv '" + extract_tmp + "' '" + kernel_root + "'");

    // Validate again in canonical destination for explicit diagnostics.
    _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        KERNEL_VERSION,
        KERNEL_LOCALVERSION,
        MODULE_INSTALL_PATH
    );

    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx.tarball_filename = spec.asset_name;
    ctx.tarball_url = asset_url;
    log("Prebuilt kernel artifact installed at " + kernel_root);
    ctx
}

fn is_built(ctx) {
    let spec = _prebuilt_spec_or_throw(KERNEL_LOCALVERSION);
    if KERNEL_FORCE_REBUILD == "1" {
        throw "forced prebuilt refresh requested";
    }
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        KERNEL_VERSION,
        KERNEL_LOCALVERSION,
        MODULE_INSTALL_PATH
    );
    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn build(ctx) {
    // Prebuilt recipe has no compile step; treat build as validated no-op.
    let spec = _prebuilt_spec_or_throw(KERNEL_LOCALVERSION);
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        KERNEL_VERSION,
        KERNEL_LOCALVERSION,
        MODULE_INSTALL_PATH
    );
    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn is_installed(ctx) {
    let spec = _prebuilt_spec_or_throw(KERNEL_LOCALVERSION);
    if KERNEL_FORCE_REBUILD == "1" {
        throw "forced prebuilt refresh requested";
    }
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        KERNEL_VERSION,
        KERNEL_LOCALVERSION,
        MODULE_INSTALL_PATH
    );
    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn install(ctx) {
    // Prebuilt recipe already installs in acquire; install is a validated no-op.
    let spec = _prebuilt_spec_or_throw(KERNEL_LOCALVERSION);
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        KERNEL_VERSION,
        KERNEL_LOCALVERSION,
        MODULE_INSTALL_PATH
    );
    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn cleanup(ctx, reason) {
    // Remove downloaded prebuilt tarball after successful acquire.
    if reason == "auto.acquire.success" {
        let tarball = join_path(BUILD_DIR, ctx.tarball_filename);
        if is_file(tarball) {
            rm(tarball);
        }
    }
    ctx
}

fn remove(ctx) {
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    if exists(kernel_root) {
        rm(kernel_root);
    }
    ctx
}
