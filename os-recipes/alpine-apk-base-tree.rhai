const RECIPE_API = 1;
// The AcornOS base tree: the base lock's APKs (the `apks` input) installed by
// the locked apk.static into an empty root, without their scripts, and the
// tree's target metadata captured from them, exactly as the system recipe
// did it before it was split. Published as one archive (the `tree` output,
// `target_pack_tree`'s format), which the package-effects step
// (`alpine-apk-package-effects.rhai`) copies.
//
// apk keeps its own database in the tree, so APKs are not unpacked one by
// one and composed like RPMs: a tree from separate apk runs would not have
// the database one apk run writes. Recipe reuses this step's result while
// this file, the base lock and the APKs are unchanged.
const NEEDS = #{ inputs: #{ apks: "directory" }, outputs: #{ tree: "file" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 14400, runtime: "minimal" };
const PACKAGE = #{ name: "alpine-apk-base-tree", description: "The locked Alpine v3.24 base APK set, installed" };
import "sources/acorn-apk-lock.rhai" as lock;

fn apk_file(row) { join_path(INPUTS.apks, basename(row[2])) }

fn verify_inputs() {
    for row in [lock::apk_tools()] + lock::packages() {
        let file = apk_file(row);
        if !is_file(file) { throw "the apk inputs lack " + basename(file) + "; run the rootfs source preseed"; }
        verify_sha256(file, row[3]);
    }
}

fn build(src, out) {
    verify_inputs();
    let rows = lock::packages();
    // The tree is build material; only its archive is the output.
    let root = join_path(WORK, "tree");
    if is_dir(root) { rm(root); }
    mkdir(root);
    progress(0, 0, "preparing apk");
    let tools = join_path(WORK, "apk-tools");
    if is_dir(tools) { rm(tools); }
    mkdir(tools);
    extract_with_format(apk_file(lock::apk_tools()), tools, "apk");
    let apk = join_path(tools, "sbin/apk.static");
    if !is_file(apk) { throw "the locked apk-tools-static has no sbin/apk.static"; }
    chmod(apk, 0o755);

    // apk checks every package's signature against the release key, where
    // alpine-keys keeps it. (What installed-system.rhai's `put` does.)
    let key = lock::signing_key();
    let path = "etc/apk/keys/" + key[0];
    mkdir(dirname(join_path(root, path)));
    write_file(join_path(root, path), key[1]);
    target_mode(root, path, 0o644);
    let cache = join_path(WORK, "apk-cache");
    mkdir(cache);
    log("Installing the locked Alpine " + lock::release() + " base APK set...");
    let args = ["--root", root, "--usermode", "--initdb", "--no-progress", "--no-scripts",
        "--force-non-repository", "--no-network", "--cache-dir", cache, "add"];
    for row in rows { args.push(apk_file(row)); }
    // APK 3 installs as a non-root user in usermode; the packages' owners come
    // from their metadata. One apk run installs them all: Recipe sees 0 until
    // it ends.
    let total = rows.len();
    progress(0, total, "packages (one apk add)");
    exec_checked(apk, args, #{uid: 1000, gid: 1000});
    progress(total, total, "packages (one apk add)");
    progress(0, 0, "capturing the installed packages");
    target_capture_apks(root, [INPUTS.apks, cache]);
    progress(0, 0, "archiving the tree");
    // The archive takes the tree's place (target_pack_tree removes it), so
    // only the archive is published.
    target_pack_tree(root, out);
}
