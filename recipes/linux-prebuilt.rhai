// The variant's kernel as a published prebuilt bundle: the release asset of the
// pin in kernel/metadata.rhai. Kernel products use the owned target tree.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ kconfig: "file?", kernel_archive: "file?", target: "directory?" }, outputs: #{ bundle: "directory", kernel: "file", modules: "directory" }, network: ["github.com:443", "release-assets.githubusercontent.com:443"] };
import "kernel/metadata.rhai" as kernel;

let build_deps = [];

let ctx = #{
    outputs: #{},
    target: if is_def_var("KERNEL_TARGET") { KERNEL_TARGET } else { "levitate" },
    kconfig_path: join_path(RUNTIME.paths.inputs, "kconfig"),
    build_dir: "",
    description: "Prebuilt Linux kernel artifacts",
    kernel_version: "",
    name: "linux-prebuilt",
};

// Where each variant's bundle is published (release tag 0.0.0-pre-alpha, asset
// `<variant>-<release>-runtime.tar.xz`) and the sha256 of that asset.
fn _asset(spec, target) {
    let assets = #{
        levitate: #{
            repo: "LevitateOS/leviso",
            sha256: "4c44c04db8cc2b5b495edd8d61f0783f1f6863b37b28083a4172249d63e7f583",
        },
        acorn: #{
            repo: "LevitateOS/AcornOS",
            sha256: "60c86254f02e054fe6223e7792d55fce56c3d0823e6293d2c9aa19ad802a6d18",
        },
    };
    if !assets.contains(target) {
        throw "no prebuilt kernel asset for KERNEL_TARGET '" + target + "' in linux-prebuilt.rhai; " +
            "publish one, or build the kernel from source with linux.rhai";
    }
    let asset = assets[target];
    asset["name"] = target + "-" + spec.release + "-runtime.tar.xz";
    asset["url"] = "https://github.com/" + asset.repo + "/releases/download/0.0.0-pre-alpha/" + asset.name;
    asset
}

// Fail when the bundle's .config lacks a value the variant's kconfig sets: the
// published kernel was built from another configuration. A bundle without its
// .config passes with a warning (bundles published before it was carried).
fn _check_config(ctx, kernel_root) {
    if !is_file(ctx.kconfig_path) { return; }
    let config = join_path(kernel_root, "kernel-build/.config");
    if !is_file(config) {
        warn("the prebuilt kernel carries no kernel-build/.config; its configuration is not checked against the kconfig");
        return;
    }
    let dropped = kernel::kconfig_dropped(ctx.kconfig_path, config);
    if dropped != "" {
        throw "the prebuilt kernel was built from a different kconfig (republish it); its .config lacks:" + dropped;
    }
}

fn _payload_context(ctx, kernel_root, spec) {
    ctx.description = "Prebuilt Linux kernel artifacts (" + spec.release + ")";
    ctx.kernel_version = spec.release;
    ctx.build_dir = join_path(kernel_root, "kernel-build");
    ctx.outputs = kernel::output_paths(kernel_root, spec.release);
    ctx
}

// Readiness of every phase: the installed bundle is the pinned one.
fn _ready(ctx) {
    let spec = kernel::source_spec(ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    let mismatch = kernel::bundle_mismatch(kernel_root, spec);
    if mismatch != "" { return needs_work(ctx, mismatch); }
    _check_config(ctx, kernel_root);
    ready(_payload_context(ctx, kernel_root, spec))
}

fn is_acquired(ctx) { _ready(ctx) }
fn is_built(ctx) { _ready(ctx) }
fn is_installed(ctx) { _ready(ctx) }

fn acquire(ctx) {
    let spec = kernel::source_spec(ctx.target);
    let asset = _asset(spec, ctx.target);
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);

    mkdir(ctx.runtime.paths.build);
    let tarball_path = join_path(ctx.runtime.paths.build, asset.name);
    let extract_tmp = join_path(ctx.runtime.paths.build, "prebuilt.tmp");

    let supplied_archive = join_path(ctx.runtime.paths.inputs, "kernel_archive");
    if is_file(supplied_archive) {
        log("Using the supplied prebuilt kernel archive");
        verify_sha256(supplied_archive, asset.sha256);
        copy_file(supplied_archive, tarball_path);
    } else {
        log("Downloading prebuilt kernel artifact " + asset.url);
        download(asset.url, tarball_path, asset.sha256);
    }

    log("Extracting prebuilt kernel artifact...");
    rm(extract_tmp);
    mkdir(extract_tmp);
    extract(tarball_path, extract_tmp);
    let mismatch = kernel::bundle_mismatch(extract_tmp, spec);
    if mismatch != "" { throw mismatch; }
    _check_config(ctx, extract_tmp);

    // Replace each tree by rename, so the old one is moved aside only once the
    // new one is complete beside it.
    let incoming = join_path(kernel_root, ".prebuilt.new");
    rm(incoming);
    mkdir(incoming);
    copy_tree_contents(extract_tmp, incoming);
    rm(extract_tmp);
    for entry in ["kernel-build", "staging"] {
        let path = join_path(kernel_root, entry);
        let old = path + ".old";
        rm(old);
        if exists(path) { mv(path, old); }
        mv(join_path(incoming, entry), path);
        rm(old);
    }
    rm(incoming);

    log("Prebuilt kernel artifact installed at " + kernel_root);
    _payload_context(ctx, kernel_root, spec)
}

// acquire installs the bundle; build and install have nothing left to do.
fn build(ctx) { ctx }
fn install(ctx) { ctx }

fn cleanup(ctx, reason) {
    if reason == "auto.acquire.failure" {
        rm(join_path(ctx.runtime.paths.build, "prebuilt.tmp"));
    }
    // The downloaded asset is not needed once it is installed.
    if reason == "auto.acquire.success" {
        let asset = _asset(kernel::source_spec(ctx.target), ctx.target);
        let tarball = join_path(ctx.runtime.paths.build, asset.name);
        if is_file(tarball) { rm(tarball); }
    }
    ctx
}

fn remove(ctx) {
    let kernel_root = kernel::artifact_root(ctx.runtime.paths.target);
    for name in ["kernel-build", "staging"] {
        let path = join_path(kernel_root, name);
        if exists(path) { rm(path); }
    }
    ctx.build_dir = "";
    ctx.outputs = #{};
    ctx
}
