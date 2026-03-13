// Linux kernel - prebuilt artifact recipe (separate from source-build flow)
//
// This recipe installs prebuilt kernel artifacts from GitHub Releases into
// KERNEL_ARTIFACT_ROOT, preserving the canonical layout expected by builder:
// - kernel-build/include/config/kernel.release
// - staging/boot/vmlinuz
// - staging/{usr/lib,lib}/modules/<kernel.release>/
//
// Required defines:
// - KERNEL_ARTIFACT_ROOT
//
// Orchestrators may also provide:
// - KERNEL_KCONFIG_PATH (used only to resolve the canonical distro target)
//
// Optional env:
// - KERNEL_FORCE_REBUILD=1 forces re-download/reinstall

let build_deps = [];

let ctx = #{
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

fn _kernel_artifact_root_or_throw(kernel_artifact_root) {
    if kernel_artifact_root == "" {
        throw "KERNEL_ARTIFACT_ROOT is required";
    }
    mkdir(kernel_artifact_root);
    kernel_artifact_root
}

fn _normalize_kernel_target(target) {
    let normalized = target.to_lower();
    if normalized == "levitate" || normalized == "leviso" {
        return "levitate";
    }
    if normalized == "acorn" || normalized == "acornos" {
        return "acorn";
    }
    if normalized == "iuppiter" || normalized == "iuppiteros" {
        return "iuppiter";
    }
    if normalized == "ralph" || normalized == "ralphos" {
        return "ralph";
    }
    ""
}

fn _kernel_target_or_throw(build_dir, kconfig_path) {
    let build_target = _normalize_kernel_target(basename(dirname(build_dir)));
    if build_target != "" {
        return build_target;
    }

    if kconfig_path != "" {
        let kconfig_target = _normalize_kernel_target(basename(dirname(kconfig_path)));
        if kconfig_target != "" {
            return kconfig_target;
        }
    }

    throw "Cannot determine prebuilt kernel target from build dir '" + build_dir + "'";
}

fn _prebuilt_spec_or_throw(build_dir, kconfig_path) {
    let target = _kernel_target_or_throw(build_dir, kconfig_path);

    // Keep this table explicit so no caller has to discover URLs/hashes manually.
    if target == "levitate" {
        return #{
            distro_id: "levitate",
            repo: "LevitateOS/leviso",
            release_tag: "0.0.0-pre-alpha",
            asset_name: "levitate-6.12.71-levitate-runtime.tar.xz",
            asset_sha256: "064ec622912a8741967c92b4cdf523eca4311bc9179d8ef1049beaf95453a2c8",
            expected_release: "6.12.71-levitate",
            localversion: "-levitate",
            module_install_path: "/usr/lib/modules",
            version: "6.12.71",
        };
    }
    if target == "acorn" {
        return #{
            distro_id: "acorn",
            repo: "LevitateOS/AcornOS",
            release_tag: "0.0.0-pre-alpha",
            asset_name: "acorn-6.19.0-acorn-runtime.v2.tar.xz",
            asset_sha256: "04e10c6392680b75b80a09beb9036e5954b62cfabddd4cb0fe3c975c0a79e96e",
            expected_release: "6.19.0-acorn",
            localversion: "-acorn",
            module_install_path: "/usr/lib/modules",
            version: "6.19",
        };
    }

    throw "No prebuilt kernel mapping for target '" + target + "'. " +
        "Remediation: publish a prebuilt runtime tarball and add its repo/tag/asset/sha in distro-builder/recipes/linux-prebuilt.rhai, " +
        "or keep using distro-builder/recipes/linux.rhai for source builds.";
}

fn _resolve_asset_url(spec) {
    "https://github.com/" + spec.repo + "/releases/download/" + spec.release_tag + "/" + spec.asset_name
}

fn _apply_prebuilt_spec(ctx, build_dir, kconfig_path) {
    let spec = _prebuilt_spec_or_throw(build_dir, kconfig_path);
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

fn _kernel_force_rebuild_requested() {
    trim(shell_output("printf '%s' \"${KERNEL_FORCE_REBUILD:-0}\"")) == "1"
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
    ctx = _apply_prebuilt_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let spec = _prebuilt_spec_or_throw(BUILD_DIR, KERNEL_KCONFIG_PATH);
    if _kernel_force_rebuild_requested() {
        throw "forced prebuilt refresh requested";
    }

    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );

    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn acquire(ctx) {
    ctx = _apply_prebuilt_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let spec = _prebuilt_spec_or_throw(BUILD_DIR, KERNEL_KCONFIG_PATH);
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
        spec.version,
        spec.localversion,
        spec.module_install_path
    );

    shell("rm -rf '" + kernel_root + "'");
    shell("mv '" + extract_tmp + "' '" + kernel_root + "'");

    // Validate again in canonical destination for explicit diagnostics.
    _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );

    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx.tarball_url = asset_url;
    log("Prebuilt kernel artifact installed at " + kernel_root);
    ctx
}

fn is_built(ctx) {
    ctx = _apply_prebuilt_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let spec = _prebuilt_spec_or_throw(BUILD_DIR, KERNEL_KCONFIG_PATH);
    if _kernel_force_rebuild_requested() {
        throw "forced prebuilt refresh requested";
    }
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );
    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn build(ctx) {
    // Prebuilt recipe has no compile step; treat build as validated no-op.
    ctx = _apply_prebuilt_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let spec = _prebuilt_spec_or_throw(BUILD_DIR, KERNEL_KCONFIG_PATH);
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );
    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn is_installed(ctx) {
    ctx = _apply_prebuilt_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let spec = _prebuilt_spec_or_throw(BUILD_DIR, KERNEL_KCONFIG_PATH);
    if _kernel_force_rebuild_requested() {
        throw "forced prebuilt refresh requested";
    }
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
    );
    ctx.kernel_version = rel;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.source_path = "";
    ctx
}

fn install(ctx) {
    // Prebuilt recipe already installs in acquire; install is a validated no-op.
    ctx = _apply_prebuilt_spec(ctx, BUILD_DIR, KERNEL_KCONFIG_PATH);
    let spec = _prebuilt_spec_or_throw(BUILD_DIR, KERNEL_KCONFIG_PATH);
    let kernel_root = _kernel_artifact_root_or_throw(KERNEL_ARTIFACT_ROOT);
    let rel = _validate_payload_or_throw(
        kernel_root,
        spec.expected_release,
        spec.version,
        spec.localversion,
        spec.module_install_path
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
