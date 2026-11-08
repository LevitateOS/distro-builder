// Build one of the install tools (recpart from its crate; recstrap, recchroot,
// recfstab and recsetup from the recstrap crate; recab, the installed system's A/B
// slot manager, from its crate; install-split, the install screen's split
// pane, from install-split-pane) from an explicit offline source
// closure, selected by the caller:
//   --input source=<standalone vendored tree of the tool's crate>
// BIN selects the tool (default recstrap; distro-builder always passes it).
// RUST_TARGET follows the image's libc and INIT_SYSTEM its init system (the
// variant's profile). The named program output is published by Recipe for the
// consumer to install.
const RECIPE_API = 2;
let bin = if is_def_var("BIN") { BIN } else { "recstrap" };
if bin != "recpart" && bin != "recstrap" && bin != "recchroot" && bin != "recfstab" && bin != "recsetup"
    && bin != "recab" && bin != "install-split" {
    throw "Unknown install tool: " + bin;
}
let ctx = #{
    name: bin,
    rootfs: join_path(RUNTIME.paths.output, "rootfs"),
    install_path: if is_def_var("INSTALL_PATH") { INSTALL_PATH } else { "usr/bin/" + bin },
    init_system: if is_def_var("INIT_SYSTEM") { INIT_SYSTEM } else { "systemd" },
    rust_target: if is_def_var("RUST_TARGET") { RUST_TARGET } else { "x86_64-unknown-linux-gnu" },
    source: join_path(RUNTIME.paths.inputs, "source"),
    built_program: "",
    program: join_path(RUNTIME.paths.output, "bin/" + bin),
};

if ctx.rust_target != "x86_64-unknown-linux-gnu" && ctx.rust_target != "x86_64-unknown-linux-musl" {
    throw "Unsupported tool Rust target: " + ctx.rust_target;
}
let target_prefix = if ctx.rust_target == "x86_64-unknown-linux-gnu" { "target/release/" }
                    else { "target/" + ctx.rust_target + "/release/" };
ctx.built_program = join_path(RUNTIME.paths.build, target_prefix + bin);

fn _verify(ctx, program) {
    let actual = trim(exec_output(program, ["--version"]));
    if !actual.starts_with(ctx.name + " ") {
        throw "Unexpected " + ctx.name + " executable: " + actual;
    }
}

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
    _verify(ctx, ctx.built_program);
    ready(ctx)
}

fn build(ctx) {
    let work = ctx.runtime.paths.build;
    let jobs = ctx.runtime.options.jobs.to_string();
    let args = ["build", "--release", "--locked", "--offline", "--bin", ctx.name,
        "--manifest-path", join_path(ctx.source, "Cargo.toml"), "--target-dir", join_path(work, "target")];
    if ctx.rust_target == "x86_64-unknown-linux-gnu" {
        let compiler = exec_output("rustc", ["-vV"]);
        if !compiler.contains("\nhost: " + ctx.rust_target + "\n") {
            throw "Selected GNU tool target does not match the compiler image's native host";
        }
    } else {
        args.push("--target"); args.push(ctx.rust_target);
    }
    let flags = if ctx.rust_target == "x86_64-unknown-linux-musl" {
        "-C linker=/usr/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld -C linker-flavor=ld.lld -C link-arg=--threads=1"
    } else { "-C link-arg=-Wl,--threads=1" };
    exec_checked("cargo", args, #{
        cwd: ctx.source,
        env: #{CARGO_HOME: join_path(work, "cargo"), CARGO_BUILD_JOBS: jobs,
            RUSTFLAGS: flags, LD_LIBRARY_PATH: "/usr/lib"},
    });
    ctx
}

fn is_installed(ctx) {
    if !is_file(ctx.program) { return needs_work(ctx, ctx.name + " output is missing"); }
    _verify(ctx, ctx.program);
    if !is_file(join_path(ctx.rootfs, ctx.install_path)) {
        return needs_work(ctx, "installed package tree is missing");
    }
    ctx.outputs = #{program: ctx.program, rootfs: ctx.rootfs};
    ready(ctx)
}

fn install(ctx) {
    let source = ctx.built_program;
    _verify(ctx, source);
    mkdir(dirname(ctx.program));
    copy_file(source, ctx.program);
    chmod(ctx.program, 0o755);
    let installed = join_path(ctx.rootfs, ctx.install_path);
    mkdir(dirname(installed)); copy_file(ctx.program, installed); chmod(installed, 0o755);
    if ctx.name == "recab" {
        let services = join_path(ctx.source, "tools/recab/src/services");
        let dest = "";
        let service = "";
        let mode = 0o644;
        if ctx.init_system == "systemd" {
            service = "recab-trial.service";
            dest = "usr/lib/systemd/system/recab-trial.service";
        } else if ctx.init_system == "openrc" {
            service = "recab-trial.openrc";
            dest = "etc/init.d/recab-trial";
            mode = 0o755;
        } else { throw "unsupported recab init system"; }
        let service_source = join_path(services, service);
        let supplied_service = join_path(ctx.runtime.paths.inputs, "trial_service");
        if is_file(supplied_service) { service_source = supplied_service; }
        if !is_file(service_source) { throw "recab needs its source service definition or an explicit trial_service input"; }
        let output = join_path(ctx.rootfs, dest);
        mkdir(dirname(output)); copy_file(service_source, output); chmod(output, mode);
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
