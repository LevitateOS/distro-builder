const RECIPE_API = 1;
// The package-effects step: the build both package backends share for the
// first half of making a system tree. A backend recipe starts with
// `//! extends: package-effects.rhai`, declares its NEEDS (with the `tree`
// output, a file) and supplies three functions:
//
//   verify_inputs()          check its inputs against the locks it imports
//   unpack(root)             put the locked packages' files into `root`:
//                            the composed tree (`copy_composed`), plus
//                            whatever the backend layers on it
//   package_effects(root)    do what the packages' scripts would have done
//
// Inputs every backend takes: `composed`, the tree the builder composed from
// the locked packages (`tree.tar`, the tree with its target metadata manifest,
// as `target_pack_tree` writes it; `packages.txt`, one `<sha256> <id>` line
// per package in the order they were composed). The tree is the base lock's
// alone: no image carries anything above the base.
//
// Then the tree's target metadata is finalized, the build's caches and logs
// are dropped, and the step publishes the tree as one archive (the `tree`
// output, `target_pack_tree`'s format; only the archive crosses to the next
// step), which the system recipe (`rootfs-producer.rhai`) copies and
// configures. This step imports neither the installed-system policy nor
// any configuration input, so a configuration change never reruns it.
const NEEDS = #{};
import "system-files.rhai" as files;

fn composed_file(name) { join_path(INPUTS.composed, name) }

// Refuses a composed tree that is not made of exactly the lock rows `rows`
// ([name, id, URL, SHA256]), in that order.
fn verify_composed(rows) {
    if !is_file(composed_file("tree.tar")) { throw "the composed input lacks tree.tar"; }
    let lines = [];
    for line in read_file(composed_file("packages.txt")).split("\n") {
        let line = trim(line);
        if line != "" { lines.push(line); }
    }
    if lines.len() != rows.len() {
        throw "the composed tree is made of " + lines.len() + " packages, the locks name " + rows.len();
    }
    for i in 0..rows.len() {
        let digest = lines[i].split(" ")[0];
        if digest != rows[i][3].to_lower() {
            throw "package " + (i + 1) + " of the composed tree is " + lines[i] + ", the locks name " + rows[i][1];
        }
    }
}

// The composed tree, its files, links, staging modes and target metadata
// manifest, into the empty `root`.
fn copy_composed(root) {
    progress(0, 0, "copying the composed tree");
    target_unpack_tree(composed_file("tree.tar"), root);
}

fn build(src, out) {
    verify_inputs();
    // The tree is build material; only its archive is the output.
    let root = join_path(WORK, "tree");
    if is_dir(root) { rm(root); }
    mkdir(root);
    progress(0, 0, "unpacking packages");
    unpack(root);
    progress(0, 0, "package effects");
    package_effects(root);
    progress(0, 0, "finalizing the tree");
    // Resolve the packages' file owners against their accounts and make the
    // staging tree writable before cleanup; package modes stay in metadata.
    target_finalize(root);
    // No caches or logs from the build.
    for dir in ["var/cache", "var/log"] {
        let path = join_path(root, dir);
        if exists(path) { rm(path); }
        mkdir(path);
    }
    // auditd's log directory.
    files::directory(root, "var/log/audit", 0o700);
    progress(0, 0, "archiving the tree");
    // The archive takes the tree's place (target_pack_tree removes it), so
    // only the archive is published and the domain never holds two trees.
    target_pack_tree(root, out);
}
