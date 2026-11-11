// The AcornOS base tree: the base lock's APKs (the `apks` input) installed by
// the locked apk.static into an empty root, without their scripts, and the
// tree's target metadata captured from them, exactly as the system recipe
// did it before it was split. Published as one archive (the `tree` output,
// `target_pack_tree`'s format), which the package-effects step
// (`alpine-apk-package-effects.rhai`) copies for the base system, and lays
// the desktop lock over for the desktop system.
//
// apk keeps its own database in the tree, so APKs are not unpacked one by
// one and composed like RPMs: a tree from separate apk runs would not have
// the database one apk run writes. Recipe's own state reuses this step while
// this file, the base lock and the APKs are unchanged.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ apks: "directory" }, outputs: #{ tree: "file" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 14400 };
import "sources/acorn-apk-lock.rhai" as lock;
let ctx = #{
    name: "alpine-apk-base-tree",
    description: "The locked Alpine " + lock::release() + " base APK set, installed",
    rows: lock::packages(),
    apk_tools: lock::apk_tools(),
};

fn root_of(ctx) { join_path(ctx.runtime.paths.output, "tree") }
fn archive_of(ctx) { join_path(ctx.runtime.paths.output, "tree.tar") }
fn stamp_of(ctx) { join_path(ctx.runtime.paths.output, "tree.built") }
fn apk_dir(ctx) { join_path(ctx.runtime.paths.inputs, "apks") }
fn apk_file(ctx, row) { join_path(apk_dir(ctx), basename(row[2])) }

fn verify_inputs(ctx) {
    for row in [ctx.apk_tools] + ctx.rows {
        let file = apk_file(ctx, row);
        if !is_file(file) { throw "the apk inputs lack " + basename(file) + "; run the rootfs source preseed"; }
        verify_sha256(file, row[3]);
    }
}

fn is_acquired(ctx) { verify_inputs(ctx); ready(ctx) }
fn acquire(ctx) { verify_inputs(ctx); ctx }

fn is_built(ctx) {
    if !is_file(stamp_of(ctx)) { return needs_work(ctx, "the base tree is not installed"); }
    if !is_file(archive_of(ctx)) { return needs_work(ctx, "the base tree is not archived"); }
    ready(ctx)
}

fn build(ctx) {
    let root = root_of(ctx);
    for file in [stamp_of(ctx), archive_of(ctx)] { if is_file(file) { rm(file); } }
    if is_dir(root) { rm(root); }
    mkdir(root);
    progress(0, 0, "preparing apk");
    let build = ctx.runtime.paths.build;
    let tools = join_path(build, "apk-tools");
    if is_dir(tools) { rm(tools); }
    mkdir(tools);
    extract_with_format(apk_file(ctx, ctx.apk_tools), tools, "apk");
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
    let cache = join_path(build, "apk-cache");
    mkdir(cache);
    log("Installing the locked Alpine " + lock::release() + " base APK set...");
    let args = ["--root", root, "--usermode", "--initdb", "--no-progress", "--no-scripts",
        "--force-non-repository", "--no-network", "--cache-dir", cache, "add"];
    for row in ctx.rows { args.push(apk_file(ctx, row)); }
    // APK 3 installs as a non-root user in usermode; the packages' owners come
    // from their metadata. One apk run installs them all: Recipe sees 0 until
    // it ends.
    let total = ctx.rows.len();
    progress(0, total, "packages (one apk add)");
    exec_checked(apk, args, #{uid: 1000, gid: 1000});
    progress(total, total, "packages (one apk add)");
    progress(0, 0, "capturing the installed packages");
    target_capture_apks(root, [apk_dir(ctx), cache]);
    progress(0, 0, "archiving the tree");
    // The archive takes the tree's place (target_pack_tree removes it), so
    // only the archive is published.
    target_pack_tree(root, archive_of(ctx));
    write_file(stamp_of(ctx), lock::locked_on() + "\n");
    ctx
}

fn with_outputs(ctx) { ctx.outputs = #{ tree: archive_of(ctx) }; ctx }
fn is_installed(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { return status; }
    ready(with_outputs(status.ctx))
}
fn install(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { throw status.reason; }
    with_outputs(status.ctx)
}

fn remove(ctx) {
    if is_dir(root_of(ctx)) { rm(root_of(ctx)); }
    for file in [stamp_of(ctx), archive_of(ctx)] { if is_file(file) { rm(file); } }
    ctx.outputs = #{};
    ctx
}
