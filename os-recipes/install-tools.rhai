// Build one install tool, a binary of the crate in an explicit offline source
// closure, as a static musl program: the same binary on every variant,
// whatever its rootfs's libc.
//   --input source=<standalone vendored tree of the tool's crate>
// BIN names the binary (default recstrap); INSTALL_PATH where it goes in the
// package tree (default usr/bin/<BIN>). recipe also packages the official source of recipes
// (the file that names it and a copy of it at the pinned commit) and the same
// pinned runtime image/launcher the builder selected, supplied as typed inputs.
// Input `program`, when given, is the already compiled binary: it is
// installed as it is and nothing compiles.
// recupdate (the release checker every installed system has) also installs its
// `dist` input (tools/recupdate/dist) for the init system the INIT define names:
// on systemd the daily notice's service and timer, the timer enabled; on OpenRC
// the notice as the periodic job /etc/periodic/daily/recupdate-notice (BusyBox's
// crond runs it); on both the per-user autostart entry. Its `keys` input holds
// the release public keys (`*.pub`, possibly none), installed at
// /usr/share/levitate/release-keys/; without one the system refuses every
// release (docs/release-engineering.md, sections 2 and 10).
// The named program output is published by Recipe for the consumer to install.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ source: "directory", catalog: "directory?", launcher: "file?", runtime_image: "file?", runtime_reference: "file?", program: "file?", dist: "directory?", keys: "directory?" },
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
    source: join_path(RUNTIME.paths.inputs, "source"),
    prebuilt: join_path(RUNTIME.paths.inputs, "program"),
    built_program: join_path(RUNTIME.paths.build, "target/" + rust_target + "/release/" + bin),
    program: join_path(RUNTIME.paths.output, "bin/" + bin),
    init: if is_def_var("INIT") { INIT } else { "" },
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
    if is_file(ctx.prebuilt) { return ready(ctx); }
    if !is_file(ctx.built_program) { return needs_work(ctx, ctx.name + " needs compilation"); }
    ready(ctx)
}

fn build(ctx) {
    // A program built elsewhere (tool-crate.rhai, one compile per crate) needs no compiling.
    if is_file(ctx.prebuilt) { return ctx; }
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
    if !is_file(join_path(inputs, "catalog/index.json")) {
        throw "the public catalog has no index.json; run 'recipe index' in recipes/ and commit it";
    }
    for name in ["launcher", "runtime_image", "runtime_reference"] {
        if !is_file(join_path(inputs, name)) { throw "Recipe packaging requires input " + name; }
    }
}

fn recipe_runtime(ctx) {
    recipe_inputs(ctx);
    let inputs = ctx.runtime.paths.inputs;
    // Recipe's `runtime stage` owns the runtime's layout and verifies the image against the reference's digest.
    exec_checked(ctx.program, ["runtime", "stage", "--root", ctx.rootfs,
        "--launcher", join_path(inputs, "launcher"),
        "--reference", trim(read_file(join_path(inputs, "runtime_reference"))),
        "--image", join_path(inputs, "runtime_image"),
        "--catalog", join_path(inputs, "catalog")]);
}

// recupdate's service files for ctx.init and the release keys, as [source, path, mode].
fn recupdate_files(ctx) {
    let inputs = ctx.runtime.paths.inputs;
    let dist = join_path(inputs, "dist");
    if !is_dir(dist) { throw "recupdate packaging requires the dist input (tools/recupdate/dist)"; }
    let files = [[join_path(dist, "recupdate-notify.desktop"), "etc/xdg/autostart/recupdate-notify.desktop", 0o644]];
    if ctx.init == "systemd" {
        files.push([join_path(dist, "recupdate-notice.service"), "usr/lib/systemd/system/recupdate-notice.service", 0o644]);
        files.push([join_path(dist, "recupdate-notice.timer"), "usr/lib/systemd/system/recupdate-notice.timer", 0o644]);
    } else if ctx.init == "openrc" {
        files.push([join_path(dist, "recupdate-notice.periodic"), "etc/periodic/daily/recupdate-notice", 0o755]);
    } else {
        throw "recupdate packaging requires INIT (systemd or openrc), not '" + ctx.init + "'";
    }
    let keys = join_path(inputs, "keys");
    if is_dir(keys) {
        for name in shell_output("ls '" + keys + "'").split("\n") {
            let name = trim(name);
            if name == "" { continue; }
            if !name.ends_with(".pub") { throw "the keys input holds " + name + ", not a .pub key"; }
            files.push([join_path(keys, name), "usr/share/levitate/release-keys/" + name, 0o644]);
        }
    }
    files
}

fn install_recupdate(ctx) {
    // A key removed from the repository leaves the image too.
    let keys = join_path(ctx.rootfs, "usr/share/levitate/release-keys");
    if is_dir(keys) { rm(keys); }
    for file in recupdate_files(ctx) {
        let dest = join_path(ctx.rootfs, file[1]);
        mkdir(dirname(dest));
        copy_file(file[0], dest);
        chmod(dest, file[2]);
        target_mode(ctx.rootfs, file[1], file[2]);
    }
    // The folder exists even without a key: recupdate then refuses every release.
    mkdir(join_path(ctx.rootfs, "usr/share/levitate/release-keys"));
    target_mode(ctx.rootfs, "usr/share/levitate/release-keys", 0o755);
    if ctx.init == "systemd" {
        ln_force("/usr/lib/systemd/system/recupdate-notice.timer",
            join_path(ctx.rootfs, "etc/systemd/system/timers.target.wants/recupdate-notice.timer"));
    }
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
        for path in ["usr/share/recipe/sources/levitate.toml", "usr/share/recipe/sources/levitate/index.json"] {
            if !is_file(join_path(ctx.rootfs, path)) { return needs_work(ctx, "Recipe's official source is missing " + path); }
        }
    }
    if ctx.name == "recupdate" {
        for file in recupdate_files(ctx) {
            if !is_file(join_path(ctx.rootfs, file[1])) { return needs_work(ctx, "recupdate is missing /" + file[1]); }
        }
    }
    ctx.outputs = #{program: ctx.program, rootfs: ctx.rootfs};
    ready(ctx)
}

fn install(ctx) {
    mkdir(dirname(ctx.program));
    copy_file(if is_file(ctx.prebuilt) { ctx.prebuilt } else { ctx.built_program }, ctx.program);
    chmod(ctx.program, 0o755);
    let installed = join_path(ctx.rootfs, ctx.install_path);
    mkdir(dirname(installed)); copy_file(ctx.program, installed); chmod(installed, 0o755);
    if ctx.name == "recipe" {
        recipe_state(ctx.rootfs);
        recipe_runtime(ctx);
    }
    if ctx.name == "recupdate" { install_recupdate(ctx); }
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
