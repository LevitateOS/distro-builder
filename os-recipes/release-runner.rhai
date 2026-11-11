// Operational x86_64 GitHub runner. Recipe owns acquisition and extraction;
// the service installer consumes this output without activating host packages.
const RECIPE_API = 2;
const NEEDS = #{
    outputs: #{ runner: "directory" },
    network: ["github.com:443", "release-assets.githubusercontent.com:443"],
    memory_mib: 8192, storage_max_bytes: 4294967296,
    storage_max_objects: 32768, network_max_bytes: 268435456, timeout_secs: 900
};
let ctx = #{
    name: "recab-release-runner", version: "2.338.2",
    archive: RUNTIME.paths.build + "/runner.tar.gz",
    runner: RUNTIME.paths.output + "/runner"
};
fn is_acquired(ctx) { ready(ctx) }
fn is_built(ctx) { ready(ctx) }
fn is_installed(ctx) {
    for name in ["config.sh", "run.sh", "bin/Runner.Listener"] {
        if !is_file(join_path(ctx.runner,name)) { return needs_work(ctx,"runner output is incomplete"); }
    }
    ctx.outputs.runner=ctx.runner; ready(ctx)
}
fn install(ctx) {
    download_exact(
        "https://github.com/actions/runner/releases/download/v2.338.2/actions-runner-linux-x64-2.338.2.tar.gz",
        ctx.archive, "baebf204307bac6193480852732f0aba2ae97b6c5597c244f4d5558390147c40", 228071396,
        ["https://github.com/actions/runner/releases/download/", "https://release-assets.githubusercontent.com/"]
    );
    rm(ctx.runner); mkdir(ctx.runner); extract(ctx.archive,ctx.runner);
    ctx.outputs.runner=ctx.runner; ctx
}
