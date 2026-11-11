// Private acquisition producer. The builder fills only the configured endpoint;
// recab supplies a small request file, never a running/prepared system tree.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ request: "file" }, outputs: #{ artifact: "file" },
    network: [] /* recab configured endpoint */,
    timeout_secs: 1800, memory_mib: 8192,
    storage_max_bytes: 4294967296, storage_max_objects: 512,
    network_max_bytes: 4294967296
};
let ctx = #{ name: "recab-acquire", version: "1", file: RUNTIME.paths.output + "/artifact" };
fn is_acquired(ctx) { ready(ctx) }
fn is_built(ctx) { ready(ctx) }
fn is_installed(ctx) {
    if !is_file(ctx.file) { return needs_work(ctx,"update object is missing"); }
    let request = parse_json(read_file(join_path(RUNTIME.paths.inputs,"request")));
    if request.sha256 != "" { verify_sha256(ctx.file,request.sha256); }
    ctx.outputs.artifact=ctx.file; ready(ctx)
}
fn install(ctx) {
    let request = parse_json(read_file(join_path(RUNTIME.paths.inputs,"request")));
    if "prefixes" in request {
        if request.sha256 == "" { fetch_bounded(request.url,ctx.file,request.size,request.prefixes); }
        else { download_exact(request.url,ctx.file,request.sha256,request.size,request.prefixes); }
    } else {
        if request.sha256 == "" { fetch_bounded(request.url,ctx.file,request.size); }
        else { download_exact(request.url,ctx.file,request.sha256,request.size); }
    }
    ctx.outputs.artifact=ctx.file; ctx
}
