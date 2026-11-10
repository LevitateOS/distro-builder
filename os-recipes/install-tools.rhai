// Build one install tool, a binary of the crate in an explicit offline source
// closure, as a static musl program: the same binary on every variant,
// whatever its rootfs's libc.
//   --input source=<standalone vendored tree of the tool's crate>
// BIN names the binary (default recstrap); INSTALL_PATH where it goes in the
// package tree (default usr/bin/<BIN>). recab also packages its trial service,
// which it writes itself for the image's init system (INIT_SYSTEM, the
// variant's profile). recipe also packages its public catalog and the same
// pinned runtime image/launcher the builder selected, supplied as typed inputs.
// The named program output is published by Recipe for the consumer to install.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ source: "directory", catalog: "directory?", launcher: "file?", runtime_image: "file?", runtime_reference: "file?" },
    outputs: #{ program: "file", rootfs: "directory" },
    // Packaging Recipe retains compiler outputs while copying its selected
    // runtime tar (the current compiler image is about 1.3 GiB).
    memory_mib: 8192,
    storage_max_bytes: 6442450944,
};
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
    if ctx.name == "recipe" { recipe_inputs(ctx); }
    if is_file(join_path(ctx.source, "Cargo.toml")) { return ready(ctx); }
    needs_work(ctx, ctx.name + " source input is missing")
}

fn acquire(ctx) {
    if ctx.name == "recipe" { recipe_inputs(ctx); }
    if is_file(join_path(ctx.source, "Cargo.toml")) { return ctx; }
    throw "Import an offline standalone source tree with --input source=PATH";
}

fn is_built(ctx) {
    if !is_file(ctx.built_program) { return needs_work(ctx, ctx.name + " needs compilation"); }
    ready(ctx)
}

fn build(ctx) {
    let work = ctx.runtime.paths.build;
    let rustflags = "-C linker=/usr/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld -C linker-flavor=ld.lld";
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

// Root's override catalog, retained state, active packages and image cache live
// on /var, shared by both A/B slots. Recipe's defaults point here from root's
// home, which the image replaces. Every
// directory of this tree replaces the rootfs's metadata when merged, so root's
// home keeps a private mode.
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

fn recipe_inputs(ctx) {
    let inputs = ctx.runtime.paths.inputs;
    if !is_dir(join_path(inputs, "catalog")) { throw "Recipe packaging requires the public catalog input"; }
    for name in ["launcher", "runtime_image", "runtime_reference"] {
        if !is_file(join_path(inputs, name)) { throw "Recipe packaging requires input " + name; }
    }
}

fn recipe_runtime(ctx) {
    recipe_inputs(ctx);
    let inputs = ctx.runtime.paths.inputs;
    let root = ctx.rootfs;
    let reference = trim(read_file(join_path(inputs, "runtime_reference")));
    let digest = reference.split("@sha256:")[1];
    // Check the exact imported tar that will ship, not just its pathname.
    verify_sha256(join_path(inputs, "runtime_image"), digest);
    let runtime = join_path(root, "usr/share/recipe/runtime");
    mkdir(runtime);
    copy_file(join_path(inputs, "runtime_image"), join_path(runtime, "rootfs.tar"));
    chmod(join_path(runtime, "rootfs.tar"), 0o644);
    write_file(join_path(runtime, "root-reference"), reference + "\n");
    chmod(join_path(runtime, "root-reference"), 0o644);
    let launcher = join_path(root, "usr/libexec/recipe/ginux");
    mkdir(dirname(launcher));
    copy_file(join_path(inputs, "launcher"), launcher);
    chmod(launcher, 0o755);
    // The image supplies defaults; an administrator's /var catalog stays intact.
    let catalog = join_path(root, "usr/share/recipe/recipes");
    mkdir(catalog);
    copy_tree_contents(join_path(inputs, "catalog"), catalog);
    let profile = join_path(root, "etc/profile.d/recipe.sh");
    mkdir(dirname(profile));
    write_file(profile,
        "# System software belongs to the booted generation; user profiles remain independent.\n" +
        "if [ -f /usr/lib/recab/immutable ]; then\n" +
        "    case \":$PATH:\" in *:/usr/lib/recipe/profile/bin:*) ;; *) PATH=\"/usr/lib/recipe/profile/bin:$PATH\" ;; esac\n" +
        "    recipe_user_bin=\n" +
        "elif [ \"$(id -u)\" -eq 0 ]; then\n" +
        "    recipe_user_bin=/var/lib/recipe/profile/bin\n" +
        "else\n" +
        "    recipe_user_bin=\"${XDG_DATA_HOME:-$HOME/.local/share}/recipe/profile/bin\"\n" +
        "fi\n" +
        "if [ \"$(id -u)\" -ne 0 ]; then recipe_user_bin=\"${XDG_DATA_HOME:-$HOME/.local/share}/recipe/profile/bin\"; fi\n" +
        "if [ -n \"$recipe_user_bin\" ]; then case \":$PATH:\" in *:\"$recipe_user_bin\":*) ;; *) PATH=\"$recipe_user_bin:$PATH\" ;; esac; fi\n" +
        "export PATH\nunset recipe_user_bin\n");
    chmod(profile, 0o644);
}

fn is_installed(ctx) {
    if !is_file(ctx.program) { return needs_work(ctx, ctx.name + " output is missing"); }
    if !is_file(join_path(ctx.rootfs, ctx.install_path)) {
        return needs_work(ctx, "installed package tree is missing");
    }
    if ctx.name == "recipe" {
        for path in ["usr/libexec/recipe/ginux", "usr/share/recipe/runtime/rootfs.tar",
                     "usr/share/recipe/runtime/root-reference", "etc/profile.d/recipe.sh"] {
            if !is_file(join_path(ctx.rootfs, path)) { return needs_work(ctx, "Recipe runtime is missing " + path); }
        }
        if !is_dir(join_path(ctx.rootfs, "usr/share/recipe/recipes")) {
            return needs_work(ctx, "Recipe's public catalog is missing");
        }
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
        exec_checked(ctx.program, ["install-unit", "--root", ctx.rootfs, "--init", ctx.init_system, "--bin", "/" + ctx.install_path]);
    }
    if ctx.name == "recipe" {
        recipe_state(ctx.rootfs);
        recipe_runtime(ctx);
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
