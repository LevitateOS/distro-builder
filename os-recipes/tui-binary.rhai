const RECIPE_API = 1;
// Compile a TUI layer from its immutable JavaScript source closure. The entry
// and libc target are selected by the builder; package/toolchain facts and all
// acquisition, lock checking, compilation and publication belong to recipes.
// `bun install` fetches the frozen package graph itself, so nothing is
// declared as SOURCES.
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
    runtime: "minimal",
};
const PACKAGE = #{ name: "tui-binary", description: "A TUI layer compiled from its JavaScript source closure" };
let build_deps = ["bun-toolchain"];

fn sha(path) { trim(exec_output("sha256sum", [path])).split(" ")[0] }

// docs/content's page index (src/generated/index.ts) is generated and
// gitignored, so the source snapshot never carries it: make it here, from the
// same sources, before the TUI that imports it is compiled. (Bun's JavaScript
// needs /dev/urandom, which Recipe assigns where the launcher serves it.)
fn generate_docs_index(compiler, work) {
    let docs = join_path(work, "docs/content");
    if !is_file(join_path(docs, "scripts/build.ts")) { return; }
    exec_checked(compiler, ["scripts/build.ts"], #{ cwd: docs, env: #{ PWD: docs } });
    if !is_file(join_path(docs, "src/generated/index.ts")) {
        throw "docs/content/scripts/build.ts wrote no src/generated/index.ts";
    }
}

fn build(src, out) {
    let target = trim(read_file(INPUTS.runtime_target));
    let compiler = DEPS["bun-toolchain"].compiler;
    let runtime = DEPS["bun-toolchain"].runtime;

    // Install the frozen workspace dependencies into a copy of the source.
    let work = join_path(WORK, "workspace");
    mkdir(work);
    copy_tree_contents(INPUTS.source, work);
    let lock_sha = sha(join_path(INPUTS.source, "bun.lock"));
    exec_checked(compiler, ["install", "--frozen-lockfile", "--filter", "@levitate/*",
        "--network-concurrency=4", "--concurrent-scripts=1", "--linker=hoisted", "--backend", "copyfile",
        "--cache-dir", join_path(WORK, "npm-cache")], #{ cwd: work,
        // Bun's HTTP-thread ceiling is separate from the install CLI setting.
        env: #{ PWD: work, BUN_CONFIG_MAX_HTTP_REQUESTS: "4" } });
    verify_sha256(join_path(work, "bun.lock"), lock_sha);
    if !is_dir(join_path(work, "node_modules")) { throw "Bun did not install the frozen TUI graph"; }

    // Compile the selected TUI layer.
    generate_docs_index(compiler, work);
    let built = join_path(WORK, "program");
    exec_checked(compiler, ["build", "entry.ts", "--compile", "--minify", "--target=" + target,
        "--compile-executable-path", runtime, "--outfile", built], #{ cwd: work, env: #{ PWD: work } });
    if !is_file(built) { throw "Bun wrote no TUI program"; }
    mv(built, out);
    chmod(out, 0o755);
}
