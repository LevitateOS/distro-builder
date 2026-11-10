// Compile a TUI layer from its immutable JavaScript source closure. The entry
// and libc target are selected by the builder; package/toolchain facts and all
// acquisition, lock checking, compilation and publication belong to recipes.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ source: "directory", runtime_target: "file" },
    outputs: #{ program: "file" },
    network: ["registry.npmjs.org:443"],
    network_max_bytes: 1073741824,
    memory_mib: 8192,
    // The frozen graph plus its verified install cache contains about 20k
    // namespace entries; compilation/source scratch needs bounded headroom.
    storage_max_objects: 32768,
    timeout_secs: 900,
};
let build_deps = ["bun-toolchain"];
let ctx = #{
    name: "tui-binary",
    target: trim(read_file(join_path(RUNTIME.paths.inputs, "runtime_target"))),
    state: #{ lock_sha: "", program_sha: "" },
};
fn workspace(ctx) { join_path(ctx.runtime.paths.build, "workspace") }
fn compiler(ctx) { join_path(ctx.runtime.paths.dependencies, "bun-toolchain/compiler") }
fn runtime(ctx) { join_path(ctx.runtime.paths.dependencies, "bun-toolchain/runtime") }
fn built(ctx) { join_path(ctx.runtime.paths.build, "program") }
fn program(ctx) { join_path(ctx.runtime.paths.output, "program") }
fn sha(path) { trim(exec_output("sha256sum", [path])).split(" ")[0] }
fn source_lock(ctx) { join_path(ctx.runtime.paths.inputs, "source/bun.lock") }

fn is_acquired(ctx) {
    if ctx.state.lock_sha.len() != 64 || !is_file(join_path(workspace(ctx), ".acquired")) ||
        !is_dir(join_path(workspace(ctx), "node_modules")) {
        return needs_work(ctx, "install frozen TUI workspace dependencies");
    }
    verify_sha256(source_lock(ctx), ctx.state.lock_sha);
    verify_sha256(join_path(workspace(ctx), "bun.lock"), ctx.state.lock_sha);
    ready(ctx)
}
fn acquire(ctx) {
    let work = workspace(ctx);
    if exists(work) { rm(work); }
    mkdir(work);
    copy_tree_contents(join_path(ctx.runtime.paths.inputs, "source"), work);
    ctx.state.lock_sha = sha(source_lock(ctx));
    exec_checked(compiler(ctx), ["install", "--frozen-lockfile", "--filter", "@levitate/*",
        "--network-concurrency=4", "--concurrent-scripts=1", "--linker=hoisted", "--backend", "copyfile",
        "--cache-dir", join_path(ctx.runtime.paths.build, "npm-cache")], #{ cwd: work,
        // Bun's HTTP-thread ceiling is separate from the install CLI setting.
        env: #{ PWD: work, BUN_CONFIG_MAX_HTTP_REQUESTS: "4" } });
    verify_sha256(join_path(work, "bun.lock"), ctx.state.lock_sha);
    if !is_dir(join_path(work, "node_modules")) { throw "Bun did not install the frozen TUI graph"; }
    write_file(join_path(work, ".acquired"), ctx.state.lock_sha);
    ctx
}
fn is_built(ctx) {
    if ctx.state.program_sha.len() != 64 || !is_file(built(ctx)) {
        return needs_work(ctx, "compile the selected TUI layer");
    }
    verify_sha256(built(ctx), ctx.state.program_sha);
    ready(ctx)
}
fn build(ctx) {
    let work = workspace(ctx);
    verify_sha256(join_path(work, "bun.lock"), ctx.state.lock_sha);
    exec_checked(compiler(ctx), ["build", "entry.ts", "--compile", "--minify", "--target=" + ctx.target,
        "--compile-executable-path", runtime(ctx), "--outfile", built(ctx)], #{ cwd: work, env: #{ PWD: work } });
    if !is_file(built(ctx)) { throw "Bun wrote no TUI program"; }
    ctx.state.program_sha = sha(built(ctx));
    ctx
}
fn is_installed(ctx) {
    if ctx.state.program_sha.len() != 64 || !is_file(program(ctx)) {
        return needs_work(ctx, "publish the verified TUI program");
    }
    verify_sha256(program(ctx), ctx.state.program_sha);
    ctx.outputs.program = program(ctx);
    ready(ctx)
}
fn install(ctx) {
    verify_sha256(built(ctx), ctx.state.program_sha);
    copy_file(built(ctx), program(ctx));
    chmod(program(ctx), 0o755);
    ctx.outputs.program = program(ctx);
    ctx
}
