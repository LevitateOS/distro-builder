// Build one install tool, a binary of the crate in an explicit offline source
// closure, as a static musl program: the same binary on every variant,
// whatever its rootfs's libc.
//   --input source=<standalone vendored tree of the tool's crate>
// BIN names the binary (default recstrap); INSTALL_PATH where it goes in the
// package tree (default usr/bin/<BIN>). recab also packages its trial service,
// which it writes itself for the image's init system (INIT_SYSTEM, the
// variant's profile). recipe, the package manager, also packages the
// directories its state lives in. The named program output is published by
// Recipe for the consumer to install.
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
    let rustflags = "-C linker=/usr/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld -C linker-flavor=ld.lld -C link-arg=--threads=1";
    if ctx.name == "recipe" { rustflags += " -C link-arg=" + renameat2_object(work); }
    exec_checked("cargo", ["build", "--release", "--locked", "--offline", "--bin", ctx.name,
        "--target", ctx.rust_target,
        "--manifest-path", join_path(ctx.source, "Cargo.toml"), "--target-dir", join_path(work, "target")], #{
        cwd: ctx.source,
        // The C parts of a dependency (recipe's compression and TLS crates)
        // compile with the image's gcc; Rust's musl target links them.
        env: #{CARGO_HOME: join_path(work, "cargo"), CARGO_BUILD_JOBS: ctx.runtime.options.jobs.to_string(),
            RUSTFLAGS: rustflags, CC_x86_64_unknown_linux_musl: "gcc", AR_x86_64_unknown_linux_musl: "ar",
            LD_LIBRARY_PATH: "/usr/lib"},
    });
    // The program is the tool it is asked for: `<bin> --version` names it.
    let version = trim(exec_output(ctx.built_program, ["--version"]));
    if !version.starts_with(ctx.name + " ") {
        throw "Unexpected " + ctx.name + " executable: " + version;
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

// Recipe's state, as root runs it: its recipes, retained state and image
// cache live on /var, which both A/B slots share, so what it installed
// survives an update. Recipe's defaults are under root's home, which the
// image replaces, so they point there. Every directory of this tree replaces
// the rootfs's metadata when merged, so root's home keeps a private mode.
fn recipe_state(root) {
    for dir in [["var/lib/recipe", 0o755], ["var/lib/recipe/recipes", 0o755],
                ["var/lib/recipe/state", 0o700], ["var/cache/recipe", 0o700],
                ["root", 0o700], ["root/.local", 0o755], ["root/.local/share", 0o755],
                ["root/.local/state", 0o700], ["root/.cache", 0o700]] {
        mkdir(join_path(root, dir[0]));
        target_mode(root, dir[0], dir[1]);
    }
    ln_force("/var/lib/recipe", join_path(root, "root/.local/share/recipe"));
    ln_force("/var/lib/recipe/state", join_path(root, "root/.local/state/recipe"));
    ln_force("/var/cache/recipe", join_path(root, "root/.cache/recipe"));
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
    if ctx.name == "recipe" { recipe_state(ctx.rootfs); }
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
