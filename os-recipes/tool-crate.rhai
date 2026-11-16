const RECIPE_API = 1;
// Compile the binaries of one install-tool crate, once, as static musl
// programs: the same files on every variant, whatever its rootfs's libc.
//   --input source=<standalone vendored tree of the tool's crate>
//   --define BINS=<comma-separated binary names> (default recstrap)
// One cargo build covers every binary, so the crate compiles once however many
// programs it has and however many distros package them. The output `bins` is a
// directory with one executable per binary; install-tools.rhai takes each as
// its `program` input.
const NEEDS = #{
    inputs: #{ source: "directory" },
    defines: #{ BINS: "recstrap" },
    outputs: #{ bins: "directory" },
    // Same budget as install-tools.rhai, which compiled one binary the same way.
    memory_mib: 8192,
    storage_max_bytes: 6442450944,
};
const PACKAGE = #{ name: "tool-crate" };
const RUST_TARGET = "x86_64-unknown-linux-musl";

fn bins() { DEFINES.BINS.split(",") }

fn build(src, out) {
    if !is_file(join_path(INPUTS.source, "Cargo.toml")) {
        throw "Import an offline standalone source tree with --input source=PATH";
    }
    let bins = bins();
    let target_dir = join_path(WORK, "target/" + RUST_TARGET + "/release");
    let rustflags = "-C linker=/usr/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld -C linker-flavor=ld.lld";
    if bins.contains("recipe") { rustflags += " -C link-arg=" + renameat2_object(); }
    progress(0, 0, "compiling " + bins.len() + " binaries (cargo build)");
    let args = ["build", "--release", "--locked", "--offline"];
    for name in bins { args.push("--bin"); args.push(name); }
    for extra in ["--target", RUST_TARGET,
        "--manifest-path", join_path(INPUTS.source, "Cargo.toml"), "--target-dir", join_path(WORK, "target")] {
        args.push(extra);
    }
    exec_checked("cargo", args, #{
        cwd: INPUTS.source,
        // The C parts of a dependency (recipe's compression and TLS crates)
        // compile with the image's gcc; Rust's musl target links them.
        env: #{CARGO_HOME: join_path(WORK, "cargo"), CARGO_BUILD_JOBS: JOBS.to_string(),
            RUSTFLAGS: rustflags, CC_x86_64_unknown_linux_musl: "gcc", AR_x86_64_unknown_linux_musl: "ar",
            LD_LIBRARY_PATH: "/usr/lib"},
    });
    // Each program is the tool it is asked for: `<bin> --version` names it.
    let checked = 0;
    progress(0, bins.len(), "binaries checked");
    for name in bins {
        let version = trim(exec_output(join_path(target_dir, name), ["--version"]));
        if !version.starts_with(name + " ") {
            throw "Unexpected " + name + " executable: " + version;
        }
        checked += 1;
        progress(checked, bins.len(), "binaries checked");
    }
    progress(0, 0, "installing the binaries");
    for name in bins {
        let program = join_path(out, name);
        mv(join_path(target_dir, name), program);
        chmod(program, 0o755);
    }
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

fn verify(out) {
    for name in bins() {
        if !is_file(join_path(out, name)) { throw name + " output is missing"; }
    }
}
