// Build one install tool, a binary of the crate in an explicit offline source
// closure, as a static musl program: the same binary on every variant,
// whatever its rootfs's libc.
//   --input source=<standalone vendored tree of the tool's crate>
// BIN names the binary (default recstrap); INSTALL_PATH where it goes in the
// package tree (default usr/bin/<BIN>). recab also packages its trial service,
// which it writes itself for the image's init system (INIT_SYSTEM, the
// variant's profile). The named program output is published by Recipe for the
// consumer to install.
const RECIPE_API = 2;
let bin = if is_def_var("BIN") { BIN } else { "recstrap" };
let rust_target = "x86_64-unknown-linux-musl";
let ctx = #{
    name: bin,
    rust_target: rust_target,
    rootfs: join_path(RUNTIME.paths.output, "rootfs"),
    install_path: if is_def_var("INSTALL_PATH") { INSTALL_PATH } else { "usr/bin/" + bin },
    init_system: if is_def_var("INIT_SYSTEM") { INIT_SYSTEM } else { "" },
    source: join_path(RUNTIME.paths.inputs, "source"),
    built_program: join_path(RUNTIME.paths.build, "target/" + rust_target + "/release/" + bin),
    program: join_path(RUNTIME.paths.output, "bin/" + bin),
};

fn is_acquired(ctx) {
    if is_file(join_path(ctx.source, "Cargo.toml")) { return ready(ctx); }
    needs_work(ctx, ctx.name + " source input is missing")
}

fn acquire(ctx) {
    if is_file(join_path(ctx.source, "Cargo.toml")) { return ctx; }
    throw "Import an offline standalone source tree with --input source=PATH";
}

fn is_built(ctx) {
    if !is_file(ctx.built_program) { return needs_work(ctx, ctx.name + " needs compilation"); }
    ready(ctx)
}

fn build(ctx) {
    let work = ctx.runtime.paths.build;
    exec_checked("cargo", ["build", "--release", "--locked", "--offline", "--bin", ctx.name,
        "--target", ctx.rust_target,
        "--manifest-path", join_path(ctx.source, "Cargo.toml"), "--target-dir", join_path(work, "target")], #{
        cwd: ctx.source,
        env: #{CARGO_HOME: join_path(work, "cargo"), CARGO_BUILD_JOBS: ctx.runtime.options.jobs.to_string(),
            RUSTFLAGS: "-C linker=/usr/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld -C linker-flavor=ld.lld -C link-arg=--threads=1",
            LD_LIBRARY_PATH: "/usr/lib"},
    });
    // The program is the tool it is asked for: `<bin> --version` names it.
    let version = trim(exec_output(ctx.built_program, ["--version"]));
    if !version.starts_with(ctx.name + " ") {
        throw "Unexpected " + ctx.name + " executable: " + version;
    }
    ctx
}

fn is_installed(ctx) {
    if !is_file(ctx.program) { return needs_work(ctx, ctx.name + " output is missing"); }
    if !is_file(join_path(ctx.rootfs, ctx.install_path)) {
        return needs_work(ctx, "installed package tree is missing");
    }
    ctx.outputs = #{program: ctx.program, rootfs: ctx.rootfs};
    ready(ctx)
}

fn install(ctx) {
    mkdir(dirname(ctx.program));
    copy_file(ctx.built_program, ctx.program);
    chmod(ctx.program, 0o755);
    let installed = join_path(ctx.rootfs, ctx.install_path);
    mkdir(dirname(installed)); copy_file(ctx.program, installed); chmod(installed, 0o755);
    if ctx.name == "recab" {
        if ctx.init_system == "" { throw "recab packages its trial service for an init system: pass INIT_SYSTEM"; }
        exec_checked(ctx.program, ["install-unit", "--root", ctx.rootfs, "--package", ctx.init_system]);
    }
    ctx.outputs = #{program: ctx.program, rootfs: ctx.rootfs};
    ctx
}

fn remove(ctx) {
    if is_dir(ctx.rootfs) { rm(ctx.rootfs); }
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
