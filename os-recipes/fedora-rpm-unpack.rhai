// Unpacks locked RPMs one by one, each into an empty tree of its own, for the
// builder's unpack store (`src/compose.rs`): the `packages` input holds the
// package files, each named by its SHA-256; the `trees` output holds, per
// package, `<sha256>/root` (what `target_extract_rpm` makes of the package in
// an empty root: its files without its scripts, and its target metadata
// manifest) and `<sha256>/members.json` (the record of that extraction, from
// which the builder composes system trees exactly as extracting the packages
// one after another into one root would).
//
// The builder runs several of these at once, each on a share of the missing
// packages, so the object limit is sized for one share, not for a system.
// Changing this file changes every package's store key, so the store unpacks
// everything again.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ packages: "directory" }, outputs: #{ trees: "directory" }, memory_mib: 4096, storage_max_objects: 131072, timeout_secs: 7200 };
let ctx = #{
    name: "fedora-rpm-unpack",
    description: "Locked RPMs, each unpacked into a tree of its own",
};

fn trees_of(ctx) { join_path(ctx.runtime.paths.output, "trees") }
fn stamp_of(ctx) { join_path(ctx.runtime.paths.output, "trees.built") }

// The package files, as [digest, file].
fn package_files(ctx) {
    let dir = join_path(ctx.runtime.paths.inputs, "packages");
    let files = [];
    for name in shell_output("ls '" + dir + "'").split("\n") {
        let name = trim(name);
        if name == "" { continue; }
        if name.len() != 64 { throw "the packages input holds " + name + ", not a file named by its SHA-256"; }
        files.push([name, join_path(dir, name)]);
    }
    if files.is_empty() { throw "the packages input is empty"; }
    files
}

fn verify_inputs(ctx) {
    for entry in package_files(ctx) { verify_sha256(entry[1], entry[0]); }
}

fn is_acquired(ctx) { verify_inputs(ctx); ready(ctx) }
fn acquire(ctx) { verify_inputs(ctx); ctx }

fn is_built(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the packages are not unpacked"); }
    ready(ctx)
}

fn build(ctx) {
    let trees = trees_of(ctx);
    if is_file(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    if is_dir(trees) { rm(trees); }
    mkdir(trees);
    let files = package_files(ctx);
    let total = files.len();
    let done = 0;
    progress(0, total, "packages");
    for entry in files {
        let dir = join_path(trees, entry[0]);
        mkdir(dir);
        // The helper makes the root itself, as extraction into a new root would.
        target_extract_rpm(entry[1], join_path(dir, "root"), join_path(dir, "members.json"));
        done += 1;
        progress(done, total, "packages");
    }
    write_file(stamp_of(ctx), "unpacked " + total + "\n");
    ctx
}

fn with_outputs(ctx) { ctx.outputs = #{ trees: trees_of(ctx) }; ctx }
fn is_installed(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the packages are not unpacked"); }
    ready(with_outputs(ctx))
}
fn install(ctx) { with_outputs(ctx) }

fn remove(ctx) {
    if is_file(stamp_of(ctx)) { rm(stamp_of(ctx)); }
    if is_dir(trees_of(ctx)) { rm(trees_of(ctx)); }
    ctx.outputs = #{};
    ctx
}
