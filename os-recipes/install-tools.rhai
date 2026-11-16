const RECIPE_API = 1;
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
// The program output is published by Recipe for the consumer to install; the
// rootfs output is the tree it installs.
const NEEDS = #{
    inputs: #{ source: "directory", catalog: "directory?", launcher: "file?", runtime_image: "file?",
               runtime_reference: "file?", program: "file?", dist: "directory?", keys: "directory?" },
    defines: #{ BIN: "recstrap", INSTALL_PATH: "", INIT: "" },
    outputs: #{ program: "file", rootfs: "directory" },
    // Packaging Recipe retains compiler outputs while copying its selected
    // runtime tar (the current compiler image is about 1.3 GiB).
    memory_mib: 8192,
    storage_max_bytes: 6442450944,
};
const PACKAGE = #{ name: "install-tools", description: "One install tool as a static musl program, and the tree it installs" };
const RUST_TARGET = "x86_64-unknown-linux-musl";

fn build(src, out) {
    let bin = DEFINES.BIN;
    if bin == "recipe" { recipe_inputs(); }
    if INPUTS.program != () {
        // A program built elsewhere (tool-crate.rhai, one compile per crate) needs no compiling.
        copy_file(INPUTS.program, out.program);        // an input is read-only: copied
    } else {
        if !is_file(join_path(INPUTS.source, "Cargo.toml")) {
            throw "the source input has no Cargo.toml; import an offline standalone source tree with --input source=PATH";
        }
        mv(compile(bin), out.program);
    }
    chmod(out.program, 0o755);
    let installed = join_path(out.rootfs, install_path());
    mkdir(dirname(installed));
    copy_file(out.program, installed);
    chmod(installed, 0o755);
    if bin == "recipe" {
        recipe_state(out.rootfs);
        recipe_runtime(out);
    }
    if bin == "recupdate" { install_recupdate(out.rootfs); }
}

// Where the program goes in the package tree.
fn install_path() {
    if DEFINES.INSTALL_PATH == "" { "usr/bin/" + DEFINES.BIN } else { DEFINES.INSTALL_PATH }
}

fn compile(bin) {
    let rustflags = "-C linker=/usr/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld -C linker-flavor=ld.lld";
    if bin == "recipe" { rustflags += " -C link-arg=" + renameat2_object(); }
    exec_checked("cargo", ["build", "--release", "--locked", "--offline", "--bin", bin,
        "--target", RUST_TARGET,
        "--manifest-path", join_path(INPUTS.source, "Cargo.toml"), "--target-dir", join_path(WORK, "target")], #{
        cwd: INPUTS.source,
        // The C parts of a dependency (recipe's compression and TLS crates)
        // compile with the image's gcc; Rust's musl target links them.
        env: #{CARGO_HOME: join_path(WORK, "cargo"), CARGO_BUILD_JOBS: JOBS.to_string(),
            RUSTFLAGS: rustflags, CC_x86_64_unknown_linux_musl: "gcc", AR_x86_64_unknown_linux_musl: "ar",
            LD_LIBRARY_PATH: "/usr/lib"},
    });
    let built = join_path(WORK, "target/" + RUST_TARGET + "/release/" + bin);
    // The program is the tool it is asked for: `<bin> --version` names it.
    let version = trim(exec_output(built, ["--version"]));
    if !version.starts_with(bin + " ") {
        throw "Unexpected " + bin + " executable: " + version;
    }
    built
}

// recipe calls libc's renameat2, which the musl that Rust's musl target links
// (1.2.5) lacks; musl 1.2.6 has it. This object is that wrapper, the plain
// system call. Drop it when recipe makes the call itself or Rust's musl is 1.2.6.
fn renameat2_object() {
    let source = join_path(WORK, "renameat2.c");
    let object = join_path(WORK, "renameat2.o");
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

fn recipe_inputs() {
    if INPUTS.catalog == () { throw "Recipe packaging requires the public catalog input"; }
    if !is_file(join_path(INPUTS.catalog, "index.json")) {
        throw "the public catalog has no index.json; run 'recipe index' in recipes/ and commit it";
    }
    for name in ["launcher", "runtime_image", "runtime_reference"] {
        if INPUTS[name] == () { throw "Recipe packaging requires input " + name; }
    }
}

fn recipe_runtime(out) {
    recipe_inputs();
    // Recipe's `runtime stage` owns the runtime's layout and verifies the image against the reference's digest.
    exec_checked(out.program, ["runtime", "stage", "--root", out.rootfs,
        "--launcher", INPUTS.launcher,
        "--reference", trim(read_file(INPUTS.runtime_reference)),
        "--image", INPUTS.runtime_image,
        "--catalog", INPUTS.catalog]);
}

// recupdate's service files for the INIT define and the release keys, as [source, path, mode].
fn recupdate_files() {
    if INPUTS.dist == () { throw "recupdate packaging requires the dist input (tools/recupdate/dist)"; }
    let dist = INPUTS.dist;
    let init = DEFINES.INIT;
    let files = [[join_path(dist, "recupdate-notify.desktop"), "etc/xdg/autostart/recupdate-notify.desktop", 0o644]];
    if init == "systemd" {
        files.push([join_path(dist, "recupdate-notice.service"), "usr/lib/systemd/system/recupdate-notice.service", 0o644]);
        files.push([join_path(dist, "recupdate-notice.timer"), "usr/lib/systemd/system/recupdate-notice.timer", 0o644]);
    } else if init == "openrc" {
        files.push([join_path(dist, "recupdate-notice.periodic"), "etc/periodic/daily/recupdate-notice", 0o755]);
    } else {
        throw "recupdate packaging requires INIT (systemd or openrc), not '" + init + "'";
    }
    if INPUTS.keys != () {
        for key in glob_list(join_path(INPUTS.keys, "*")) {
            let name = basename(key);
            if !name.ends_with(".pub") { throw "the keys input holds " + name + ", not a .pub key"; }
            files.push([key, "usr/share/levitate/release-keys/" + name, 0o644]);
        }
    }
    files
}

fn install_recupdate(root) {
    // A key removed from the repository leaves the image too.
    let keys = join_path(root, "usr/share/levitate/release-keys");
    if is_dir(keys) { rm(keys); }
    for file in recupdate_files() {
        let dest = join_path(root, file[1]);
        mkdir(dirname(dest));
        copy_file(file[0], dest);
        chmod(dest, file[2]);
        target_mode(root, file[1], file[2]);
    }
    // The folder exists even without a key: recupdate then refuses every release.
    mkdir(join_path(root, "usr/share/levitate/release-keys"));
    target_mode(root, "usr/share/levitate/release-keys", 0o755);
    if DEFINES.INIT == "systemd" {
        ln_force("/usr/lib/systemd/system/recupdate-notice.timer",
            join_path(root, "etc/systemd/system/timers.target.wants/recupdate-notice.timer"));
    }
}

fn verify(out) {
    if !is_file(out.program) { throw DEFINES.BIN + " program is missing"; }
    if !is_file(join_path(out.rootfs, install_path())) { throw "the installed package tree lacks /" + install_path(); }
    if DEFINES.BIN == "recipe" {
        for path in ["usr/libexec/recipe/ginux", "usr/share/recipe/runtime/rootfs.tar",
                     "usr/share/recipe/runtime/root-reference", "etc/profile.d/recipe.sh"] {
            if !is_file(join_path(out.rootfs, path)) { throw "Recipe runtime is missing " + path; }
        }
        for path in ["usr/share/recipe/sources/levitate.toml", "usr/share/recipe/sources/levitate/index.json"] {
            if !is_file(join_path(out.rootfs, path)) { throw "Recipe's official source is missing " + path; }
        }
    }
    if DEFINES.BIN == "recupdate" {
        for file in recupdate_files() {
            if !is_file(join_path(out.rootfs, file[1])) { throw "recupdate is missing /" + file[1]; }
        }
    }
}
