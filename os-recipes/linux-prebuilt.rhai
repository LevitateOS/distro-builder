const RECIPE_API = 1;
// The variant's kernel as a published prebuilt bundle: the release asset of the
// pin in kernel/metadata.rhai. Its one output is the kernel bundle; the
// builder checks the kernel and the modules inside it. The `target` input, the
// previous bundle, seeds the output: when it already holds the pinned kernel
// nothing is downloaded. The asset is chosen by KERNEL_TARGET and replaced by
// the `kernel_archive` input when given, so it is downloaded in build, not
// declared as SOURCES.
const NEEDS = #{
    inputs: #{ kconfig: "file?", kernel_archive: "file?", target: "directory?" },
    defines: #{ KERNEL_TARGET: "levitate" },
    outputs: #{ bundle: "directory" },
    network: ["github.com:443", "release-assets.githubusercontent.com:443"],
    runtime: "minimal",
};
const PACKAGE = #{ name: "linux-prebuilt", description: "Prebuilt Linux kernel artifacts" };
import "kernel/metadata.rhai" as kernel;

// Where each variant's bundle is published (release tag 0.0.0-pre-alpha, asset
// `<variant>-<release>-runtime.tar.xz`) and the sha256 of that asset.
// `cargo xtask kernel publish <variant> --upload` rewrites the sha256 here.
const ASSETS = #{
    levitate: #{
        repo: "LevitateOS/leviso",
        sha256: "959ea8c40488783be54f7dd5eaafd4e28532384c58894ec890220aba7be52e0b",
    },
    acorn: #{
        repo: "LevitateOS/AcornOS",
        sha256: "939b159680b8e4442464d1b684a0cef8697f1f13d282b42445730f1b88299297",
    },
};

// The asset of `target` for the pinned release: repo, sha256, name and URL.
fn _asset(spec, target) {
    if ASSETS[target] == () {
        throw "no prebuilt kernel asset for KERNEL_TARGET '" + target + "' in linux-prebuilt.rhai; " +
            "publish one, or build the kernel from source with linux.rhai";
    }
    let asset = ASSETS[target];
    asset["name"] = target + "-" + spec.release + "-runtime.tar.xz";
    asset["url"] = "https://github.com/" + asset.repo + "/releases/download/0.0.0-pre-alpha/" + asset.name;
    asset
}

// Fail when the bundle's .config lacks a value the variant's kconfig sets: the
// published kernel was built from another configuration. A bundle without its
// .config passes with a warning (bundles published before it was carried).
fn _check_config(kernel_root) {
    if INPUTS.kconfig == () { return; }
    let config = join_path(kernel_root, "kernel-build/.config");
    if !is_file(config) {
        warn("the prebuilt kernel carries no kernel-build/.config; its configuration is not checked against the kconfig");
        return;
    }
    let dropped = kernel::kconfig_dropped(INPUTS.kconfig, config);
    if dropped != "" {
        throw "the prebuilt kernel was built from a different kconfig (republish it); its .config lacks:" + dropped;
    }
}

// `out` starts as a copy of the `target` input when one is given.
fn build(src, out) {
    let target = DEFINES.KERNEL_TARGET;
    let spec = kernel::source_spec(target);
    if kernel::bundle_mismatch(out, spec) == "" {
        _check_config(out);
        log("The prebuilt kernel " + spec.release + " is already in place");
        return;
    }

    let asset = _asset(spec, target);
    let tarball = join_path(WORK, asset.name);
    if INPUTS.kernel_archive != () {
        log("Using the supplied prebuilt kernel archive");
        verify_sha256(INPUTS.kernel_archive, asset.sha256);
        tarball = INPUTS.kernel_archive;
    } else {
        log("Downloading prebuilt kernel artifact " + asset.url);
        download(asset.url, tarball, asset.sha256);
    }

    log("Extracting prebuilt kernel artifact...");
    let unpacked = join_path(WORK, "prebuilt");
    rm(unpacked);
    mkdir(unpacked);
    extract_with_format(tarball, unpacked, "tar.xz");
    let mismatch = kernel::bundle_mismatch(unpacked, spec);
    if mismatch != "" { throw mismatch; }
    _check_config(unpacked);

    // The bundle's two trees replace the previous ones; anything else the
    // previous bundle holds stays.
    for entry in ["kernel-build", "staging"] {
        let path = join_path(out, entry);
        rm(path);
        mv(join_path(unpacked, entry), path);
    }
    log("Prebuilt kernel artifact " + spec.release + " is in the bundle");
}

// The bundle holds the pinned kernel image and its modules.
fn verify(out) {
    let spec = kernel::source_spec(DEFINES.KERNEL_TARGET);
    let mismatch = kernel::bundle_mismatch(out, spec);
    if mismatch != "" { throw mismatch; }
}
