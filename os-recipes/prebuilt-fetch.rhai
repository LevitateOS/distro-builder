// Fetch one published prebuilt: the release `<family>-<key16>` of the prebuilt
// repository holds `prebuilt.json` and the archive of the programs.
//   --define BASE_URL=https://github.com/<owner>/<repo>/releases/download/<tag>
//   --define KEY=<64 hex>      the key the builder computed; prebuilt.json must carry it
//   --define ARCHIVE=<asset>   the archive's file name (`<family>-<name>.tar.gz`)
// Output `files` holds the extracted programs and `manifest` is prebuilt.json. The
// builder checks every file against the manifest's SHA-256 again before using it;
// a 404 or any mismatch fails this recipe, which the builder treats as "no prebuilt".
const RECIPE_API = 2;
const NEEDS = #{
    outputs: #{ files: "directory", manifest: "file" },
    network: ["github.com:443", "release-assets.githubusercontent.com:443"],
    network_max_bytes: 536870912,
    memory_mib: 2048,
    timeout_secs: 900,
};

let ctx = #{
    name: "prebuilt-fetch",
    base_url: if is_def_var("BASE_URL") { BASE_URL } else { "" },
    key: if is_def_var("KEY") { KEY } else { "" },
    archive: if is_def_var("ARCHIVE") { ARCHIVE } else { "" },
    state: #{ archive_sha: "" },
};

fn manifest_path(ctx) { join_path(ctx.runtime.paths.build, "prebuilt.json") }
fn archive_path(ctx) { join_path(ctx.runtime.paths.build, ctx.archive) }
fn unpacked(ctx) { join_path(ctx.runtime.paths.build, "unpacked") }
fn files(ctx) { join_path(ctx.runtime.paths.output, "files") }
fn manifest_out(ctx) { join_path(ctx.runtime.paths.output, "prebuilt.json") }

fn is_acquired(ctx) {
    if ctx.state.archive_sha.len() != 64 || !is_file(archive_path(ctx)) || !is_file(manifest_path(ctx)) {
        return needs_work(ctx, "download the prebuilt manifest and archive");
    }
    verify_sha256(archive_path(ctx), ctx.state.archive_sha);
    ready(ctx)
}
fn acquire(ctx) {
    if !ctx.base_url.starts_with("https://github.com/") || ctx.key.len() != 64 ||
        ctx.archive == "" || ctx.archive.contains("/") {
        throw "Define BASE_URL (https://github.com/...), KEY (64 hex) and ARCHIVE (a file name)";
    }
    fetch_bounded(ctx.base_url + "/prebuilt.json", manifest_path(ctx), 1048576);
    let manifest = parse_json(read_file(manifest_path(ctx)));
    if manifest.key != ctx.key { throw "prebuilt.json is for another key"; }
    let archive = manifest.archive;
    if archive.name != ctx.archive { throw "prebuilt.json names another archive"; }
    download_exact(ctx.base_url + "/" + ctx.archive, archive_path(ctx), archive.sha256, archive.bytes);
    ctx.state.archive_sha = archive.sha256;
    ctx
}
fn is_built(ctx) {
    if ctx.state.archive_sha.len() != 64 || !is_dir(unpacked(ctx)) {
        return needs_work(ctx, "extract the prebuilt archive");
    }
    ready(ctx)
}
fn build(ctx) {
    verify_sha256(archive_path(ctx), ctx.state.archive_sha);
    if exists(unpacked(ctx)) { rm(unpacked(ctx)); }
    mkdir(unpacked(ctx));
    extract(archive_path(ctx), unpacked(ctx));
    ctx
}
fn is_installed(ctx) {
    if !is_dir(files(ctx)) || !is_file(manifest_out(ctx)) {
        return needs_work(ctx, "publish the prebuilt programs");
    }
    ctx.outputs.files = files(ctx);
    ctx.outputs.manifest = manifest_out(ctx);
    ready(ctx)
}
fn install(ctx) {
    if exists(files(ctx)) { rm(files(ctx)); }
    mkdir(files(ctx));
    copy_tree_contents(unpacked(ctx), files(ctx));
    copy_file(manifest_path(ctx), manifest_out(ctx));
    ctx.outputs.files = files(ctx);
    ctx.outputs.manifest = manifest_out(ctx);
    ctx
}
