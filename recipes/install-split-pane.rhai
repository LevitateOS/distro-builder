// Build the installation tool from an explicit offline source closure, or use
// an explicitly supplied executable. Host paths are selected by the caller:
//   --input source=<standalone vendored levitate-install-docs-split tree>
//   --input program=<levitate-install-docs-split executable>
// The named program output is published by Recipe for the consumer to install.
const RECIPE_API = 2;
let ctx = #{
    name: "install-split-pane",
    version: "0.1.0",
    rust_target: if is_def_var("RUST_TARGET") { RUST_TARGET } else { "x86_64-unknown-linux-gnu" },
    source: "/inputs/source",
    supplied_program: "/inputs/program",
    built_program: "",
    program: join_path(RUNTIME.paths.output, "bin/levitate-install-docs-split"),
};

if ctx.rust_target != "x86_64-unknown-linux-gnu" && ctx.rust_target != "x86_64-unknown-linux-musl" {
    throw "Unsupported tool Rust target: " + ctx.rust_target;
}
let target_prefix = if ctx.rust_target == "x86_64-unknown-linux-gnu" { "target/release/" }
                    else { "target/" + ctx.rust_target + "/release/" };
ctx.built_program = join_path(RUNTIME.paths.build, target_prefix + "levitate-install-docs-split");

fn _verify(ctx, program) {
    let help = exec_output(program, ["--help"]);
    if !help.contains("levitate-install-docs-split --smoke") {
        throw "Unexpected split-pane executable: missing its declared smoke entry";
    }
}

fn is_acquired(ctx) {
    if is_file(join_path(ctx.source, "Cargo.toml")) || is_file(ctx.supplied_program) {
        return ready(ctx);
    }
    needs_work(ctx, "levitate-install-docs-split source or executable input is missing")
}

fn acquire(ctx) {
    if is_file(join_path(ctx.source, "Cargo.toml")) || is_file(ctx.supplied_program) { return ctx; }
    throw "Import an offline standalone source tree with --input source=PATH, or an executable with --input program=PATH";
}

fn is_built(ctx) {
    let program = if is_file(join_path(ctx.source, "Cargo.toml")) { ctx.built_program } else { ctx.supplied_program };
    if !is_file(program) { return needs_work(ctx, "levitate-install-docs-split needs compilation"); }
    _verify(ctx, program);
    ready(ctx)
}

fn build(ctx) {
    let work = ctx.runtime.paths.build;
    let jobs = ctx.runtime.options.jobs.to_string();
    if !is_file(join_path(ctx.source, "Cargo.toml")) {
        _verify(ctx, ctx.supplied_program);
        return ctx;
    }
    let args = ["build", "--release", "--locked", "--offline", "--bin", "levitate-install-docs-split",
        "--manifest-path", join_path(ctx.source, "Cargo.toml"), "--target-dir", join_path(work, "target")];
    if ctx.rust_target == "x86_64-unknown-linux-gnu" {
        let compiler = exec_output("rustc", ["-vV"]);
        if !compiler.contains("\nhost: " + ctx.rust_target + "\n") {
            throw "Selected GNU tool target does not match the compiler image's native host";
        }
    } else {
        args.push("--target"); args.push(ctx.rust_target);
    }
    exec_checked("cargo", args, #{
        cwd: ctx.source,
        env: #{CARGO_HOME: join_path(work, "cargo"), CARGO_BUILD_JOBS: jobs,
            RUSTFLAGS: "-C link-arg=-Wl,--threads=1", LD_LIBRARY_PATH: "/usr/lib"},
    });
    ctx
}

fn is_installed(ctx) {
    if !is_file(ctx.program) { return needs_work(ctx, "levitate-install-docs-split output is missing"); }
    _verify(ctx, ctx.program);
    ctx.outputs.program = ctx.program;
    ready(ctx)
}

fn install(ctx) {
    let source = if is_file(join_path(ctx.source, "Cargo.toml")) { ctx.built_program } else { ctx.supplied_program };
    _verify(ctx, source);
    mkdir(dirname(ctx.program));
    copy_file(source, ctx.program);
    chmod(ctx.program, 0o755);
    ctx.outputs.program = ctx.program;
    ctx
}

fn remove(ctx) {
    if is_file(ctx.program) { rm(ctx.program); }
    ctx
}

fn cleanup(ctx, reason) {
    let work = ctx.runtime.paths.build;
    if reason == "manual" {
        for directory in [join_path(work, "target"), join_path(work, "cargo")] {
            if is_dir(directory) { rm(directory); }
        }
        if is_file(ctx.program) { rm(ctx.program); }
    }
    ctx
}
