// Compile the binaries of one install-tool crate, once, as static musl
// programs: the same files on every variant, whatever its rootfs's libc.
//   --input source=<standalone vendored tree of the tool's crate>
//   --define BINS=<comma-separated binary names> (default recstrap)
// One cargo build covers every binary, so the crate compiles once however many
// programs it has and however many distros package them. The output `bins` is a
// directory with one executable per binary; install-tools.rhai takes each as
// its `program` input.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ source: "directory" },
    outputs: #{ bins: "directory" },
    // Same budget as install-tools.rhai, which compiled one binary the same way.
    memory_mib: 8192,
    storage_max_bytes: 6442450944,
};
let bins = (if is_def_var("BINS") { BINS } else { "recstrap" }).split(",");
let rust_target = "x86_64-unknown-linux-musl";
let ctx = #{
    bins: bins,
    rust_target: rust_target,
    source: join_path(RUNTIME.paths.inputs, "source"),
    target_dir: join_path(RUNTIME.paths.build, "target/" + rust_target + "/release"),
    output: join_path(RUNTIME.paths.output, "bins"),
};

fn is_acquired(ctx) {
    if is_file(join_path(ctx.source, "Cargo.toml")) { return ready(ctx); }
    needs_work(ctx, "crate source input is missing")
}

fn acquire(ctx) {
    if is_file(join_path(ctx.source, "Cargo.toml")) { return ctx; }
    throw "Import an offline standalone source tree with --input source=PATH";
}

fn is_built(ctx) {
    for name in ctx.bins {
        if !is_file(join_path(ctx.target_dir, name)) { return needs_work(ctx, name + " needs compilation"); }
    }
    ready(ctx)
}

fn build(ctx) {
    let work = ctx.runtime.paths.build;
    let rustflags = "-C linker=/usr/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld -C linker-flavor=ld.lld";
    if ctx.bins.contains("recipe") { rustflags += " -C link-arg=" + renameat2_object(work); }
    progress(0, 0, "compiling " + ctx.bins.len() + " binaries (cargo build)");
    let args = ["build", "--release", "--locked", "--offline"];
    for name in ctx.bins { args.push("--bin"); args.push(name); }
    for extra in ["--target", ctx.rust_target,
        "--manifest-path", join_path(ctx.source, "Cargo.toml"), "--target-dir", join_path(work, "target")] {
        args.push(extra);
    }
    exec_checked("cargo", args, #{
        cwd: ctx.source,
        // The C parts of a dependency (recipe's compression and TLS crates)
        // compile with the image's gcc; Rust's musl target links them.
        env: #{CARGO_HOME: join_path(work, "cargo"), CARGO_BUILD_JOBS: ctx.runtime.options.jobs.to_string(),
            RUSTFLAGS: rustflags, CC_x86_64_unknown_linux_musl: "gcc", AR_x86_64_unknown_linux_musl: "ar",
            LD_LIBRARY_PATH: "/usr/lib"},
    });
    // Each program is the tool it is asked for: `<bin> --version` names it.
    let checked = 0;
    progress(0, ctx.bins.len(), "binaries checked");
    for name in ctx.bins {
        let version = trim(exec_output(join_path(ctx.target_dir, name), ["--version"]));
        if !version.starts_with(name + " ") {
            throw "Unexpected " + name + " executable: " + version;
        }
        checked += 1;
        progress(checked, ctx.bins.len(), "binaries checked");
    }
    ctx
}

// recipe calls libc's renameat2, which the musl that Rust's musl target links
// (1.2.5) lacks; musl 1.2.6 has it. This object is that wrapper, the plain
// system call. Drop it when recipe makes the call itself or Rust's musl is 1.2.6.
fn renameat2_object(work) {
    let source = join_path(work, "renameat2.c");
    let object = join_path(work, "renameat2.o");
    write_file(source, "#include <unistd.h>\n#include <sys/syscall.h>\n" +
        "int renameat2(int olddirfd, const char *oldpath, int newdirfd, const char *newpath, unsigned int flags)\n" +
        "{ return syscall(SYS_renameat2, olddirfd, oldpath, newdirfd, newpath, flags); }\n");
    exec_checked("gcc", ["-O2", "-fPIE", "-fno-stack-protector", "-c", source, "-o", object]);
    object
}

fn is_installed(ctx) {
    for name in ctx.bins {
        if !is_file(join_path(ctx.output, name)) { return needs_work(ctx, name + " output is missing"); }
    }
    ctx.outputs = #{bins: ctx.output};
    ready(ctx)
}

fn install(ctx) {
    mkdir(ctx.output);
    progress(0, 0, "installing the binaries");
    for name in ctx.bins {
        let program = join_path(ctx.output, name);
        copy_file(join_path(ctx.target_dir, name), program);
        chmod(program, 0o755);
    }
    ctx.outputs = #{bins: ctx.output};
    ctx
}

fn remove(ctx) {
    if is_dir(ctx.output) { rm(ctx.output); }
    ctx
}

fn cleanup(ctx, reason) {
    let work = ctx.runtime.paths.build;
    if reason == "manual" {
        for directory in [join_path(work, "target"), join_path(work, "cargo")] {
            if is_dir(directory) { rm(directory); }
        }
        if is_dir(ctx.output) { rm(ctx.output); }
    }
    ctx
}
