const RECIPE_API = 1;
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
const NEEDS = #{ inputs: #{ packages: "directory" }, outputs: #{ trees: "directory" }, memory_mib: 4096, storage_max_objects: 131072, timeout_secs: 7200, runtime: "minimal" };
const PACKAGE = #{ name: "fedora-rpm-unpack", description: "Locked RPMs, each unpacked into a tree of its own" };

// The package files, as [digest, file], each checked against its name.
fn package_files() {
    let dir = INPUTS.packages;
    let files = [];
    for name in shell_output("ls '" + dir + "'").split("\n") {
        let name = trim(name);
        if name == "" { continue; }
        if name.len() != 64 { throw "the packages input holds " + name + ", not a file named by its SHA-256"; }
        let file = join_path(dir, name);
        verify_sha256(file, name);
        files.push([name, file]);
    }
    if files.is_empty() { throw "the packages input is empty"; }
    files
}

fn build(src, out) {
    let files = package_files();
    let total = files.len();
    let done = 0;
    progress(0, total, "packages");
    for entry in files {
        let dir = join_path(out, entry[0]);
        mkdir(dir);
        // The helper makes the root itself, as extraction into a new root would.
        target_extract_rpm(entry[1], join_path(dir, "root"), join_path(dir, "members.json"));
        done += 1;
        progress(done, total, "packages");
    }
}
