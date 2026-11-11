// Secret-free operational check of explicit public metadata, through the same
// scoped HTTPS helper used by recab. This does not validate an installed update.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ request: "file" }, outputs: #{ metadata: "directory" },
    network: ["levitateos.github.io:443"], timeout_secs: 300, memory_mib: 8192,
    storage_max_bytes: 4294967296, storage_max_objects: 512, network_max_bytes: 4194304
};
let ctx = #{ name: "recab-public-metadata-check", version: "1", metadata: RUNTIME.paths.output + "/metadata" };
fn is_acquired(ctx) { ready(ctx) }
fn is_built(ctx) { ready(ctx) }
fn is_installed(ctx) {
    let request=parse_json(read_file(join_path(RUNTIME.paths.inputs,"request")));
    for member in request.members {
        let file=join_path(ctx.metadata,member.name);
        if !is_file(file) { return needs_work(ctx,"public metadata check is incomplete"); }
        verify_sha256(file,member.sha256);
    }
    ctx.outputs.metadata=ctx.metadata; ready(ctx)
}
fn install(ctx) {
    mkdir(ctx.metadata);
    let request=parse_json(read_file(join_path(RUNTIME.paths.inputs,"request")));
    for member in request.members {
        download_exact(member.url,join_path(ctx.metadata,member.name),member.sha256,member.size,
            ["https://levitateos.github.io/LevitateOS/updates/"]);
    }
    ctx.outputs.metadata=ctx.metadata; ctx
}
