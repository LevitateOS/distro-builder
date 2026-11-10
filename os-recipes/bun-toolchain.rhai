// Pinned compiler and target runtime for the TUI producer. Acquisition and
// extraction belong here; the builder only selects the runtime via a typed input.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ runtime_target: "file" },
    outputs: #{ compiler: "file", runtime: "file" },
    network: ["github.com:443", "release-assets.githubusercontent.com:443"],
    network_max_bytes: 536870912,
    memory_mib: 4096,
    timeout_secs: 600,
};

let ctx = #{
    name: "bun-toolchain",
    version: "1.4.2",
    target: trim(read_file(join_path(RUNTIME.paths.inputs, "runtime_target"))),
    state: #{ compiler_sha: "", runtime_sha: "" },
};
if ctx.target != "bun-linux-x64" && ctx.target != "bun-linux-x64-musl" {
    throw "unsupported Bun target: " + ctx.target;
}

fn digest(target) {
    if target == "bun-linux-x64" { return "36368faef7527875d5ffa52e53cd48021741f2a83eb6208a8dd64068d422a913"; }
    if target == "bun-linux-x64-musl" { return "4835eca59d6da70f4674f5642f6e459dcadab773695b2ed9922d131057989742"; }
    throw "unsupported Bun target: " + target;
}
fn archive(ctx, target) { join_path(ctx.runtime.paths.build, target + ".zip") }
fn payload(ctx, target) { join_path(ctx.runtime.paths.build, "unpacked/" + target + "/bun") }
fn compiler(ctx) { join_path(ctx.runtime.paths.output, "bin/bun") }
fn runtime(ctx) { join_path(ctx.runtime.paths.output, "bin/runtime") }
fn targets(ctx) {
    if ctx.target == "bun-linux-x64" { return ["bun-linux-x64"]; }
    ["bun-linux-x64", ctx.target]
}
fn sha(path) { trim(exec_output("sha256sum", [path])).split(" ")[0] }

fn is_acquired(ctx) {
    for target in targets(ctx) {
        let file = archive(ctx, target);
        if !is_file(file) { return needs_work(ctx, "missing pinned " + target + " archive"); }
        verify_sha256(file, digest(target));
    }
    ready(ctx)
}
fn acquire(ctx) {
    for target in targets(ctx) {
        download("https://github.com/oven-sh/bun/releases/download/bun-v" + ctx.version + "/" + target + ".zip",
            archive(ctx, target), digest(target));
    }
    ctx
}
fn is_built(ctx) {
    if ctx.state.compiler_sha.len() != 64 || ctx.state.runtime_sha.len() != 64 ||
        !is_file(payload(ctx, "bun-linux-x64")) || !is_file(payload(ctx, ctx.target)) {
        return needs_work(ctx, "extract the pinned Bun archives");
    }
    verify_sha256(payload(ctx, "bun-linux-x64"), ctx.state.compiler_sha);
    verify_sha256(payload(ctx, ctx.target), ctx.state.runtime_sha);
    ready(ctx)
}
fn build(ctx) {
    let dir = join_path(ctx.runtime.paths.build, "unpacked");
    if exists(dir) { rm(dir); }
    mkdir(dir);
    for target in targets(ctx) {
        verify_sha256(archive(ctx, target), digest(target));
        extract_with_format(archive(ctx, target), dir, "zip");
        if !is_file(payload(ctx, target)) { throw "Bun archive lacks " + target + "/bun"; }
    }
    ctx.state.compiler_sha = sha(payload(ctx, "bun-linux-x64"));
    ctx.state.runtime_sha = sha(payload(ctx, ctx.target));
    ctx
}
fn is_installed(ctx) {
    if ctx.state.compiler_sha.len() != 64 || ctx.state.runtime_sha.len() != 64 ||
        !is_file(compiler(ctx)) || !is_file(runtime(ctx)) {
        return needs_work(ctx, "publish verified Bun compiler and runtime");
    }
    verify_sha256(compiler(ctx), ctx.state.compiler_sha);
    verify_sha256(runtime(ctx), ctx.state.runtime_sha);
    ctx.outputs.compiler = compiler(ctx);
    ctx.outputs.runtime = runtime(ctx);
    ready(ctx)
}
fn install(ctx) {
    mkdir(dirname(compiler(ctx)));
    copy_file(payload(ctx, "bun-linux-x64"), compiler(ctx));
    copy_file(payload(ctx, ctx.target), runtime(ctx));
    chmod(compiler(ctx), 0o755);
    chmod(runtime(ctx), 0o755);
    ctx.outputs.compiler = compiler(ctx);
    ctx.outputs.runtime = runtime(ctx);
    ctx
}
