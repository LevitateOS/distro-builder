const RECIPE_API = 1;
// Compiles the login screen, reclogin (tools/reclogin: reclogind and
// reclogin-greeter), and publishes the tree the `login-screen` group recipe
// installs: the two programs, the PAM services, the settings file, the
// accounts, the init system's service files, the brand folder and the
// guest's files.
//   --input source=<the offline source closure the builder prepares:
//                   tools/reclogin, a one-member workspace, the pruned
//                   Cargo.lock and every crates.io crate vendored>
//   --define OS_ID=<levitateos|acornos>  which brand and PAM file
//   --define INIT=<systemd|openrc>       which service files
// This recipe does not run in Recipe's usual root. The builder runs it in the
// variant's apps-build root (as desktop-apps.rhai), so cargo, rustc and the
// linker are the distro's own, and the programs link the C library, libpam,
// libinput and libxkbcommon the system ships, dynamically. It declares no
// network: cargo runs --locked --offline against the vendored crates.
const NEEDS = #{
    inputs: #{ source: "directory" },
    defines: #{ OS_ID: "", INIT: "" },
    outputs: #{ rootfs: "directory" },
    memory_mib: 8192,
    storage_max_bytes: 6442450944,
    storage_max_objects: 262144,
    timeout_secs: 7200,
};
const PACKAGE = #{ name: "reclogin" };
const PROGRAMS = ["reclogind", "reclogin-greeter"];

// The crate inside the closure, at its path in the repository.
fn crate_dir() { join_path(INPUTS.source, "tools/reclogin") }
fn target_dir() { join_path(WORK, "target") }
fn built(program) { join_path(target_dir(), "release/" + program) }

// cargo's own state in the build folder; the vendored crates are the only
// source (the closure's .cargo/config.toml names them at /inputs/source/vendor,
// and --config below names them where this run sees them).
fn cargo_env() {
    #{
        HOME: WORK,
        LC_ALL: "C",
        CARGO_HOME: join_path(WORK, "cargo"),
        CARGO_BUILD_JOBS: JOBS.to_string(),
        CARGO_INCREMENTAL: "0",
        CARGO_PROFILE_RELEASE_STRIP: "true",
    }
}

// WORKAROUND for a Ginux gap, musl roots only (AcornOS). musl's fcntl() adds
// O_LARGEFILE (0o100000) to every F_SETFL argument; Linux ignores it on
// x86-64, but Ginux answers F_SETFL on a pipe with any bit outside
// O_NONBLOCK|O_APPEND|O_ACCMODE with EOPNOTSUPP (tools/ginux,
// crates/ginux-init/src/channel_hooks.rs, the `72 => match r.rsi` arm; its
// native-descriptor path in descriptors.rs accepts the bit). rustc then cannot
// create its jobserver (jobserver's Client::new sets O_NONBLOCK on the pipe
// with F_SETFL: "failed to create jobserver ... code: 95"), and cargo's output
// reader (cargo-util read2) fails the same way. This preload answers F_SETFL
// with the raw system call and without O_LARGEFILE (implied on x86-64, so the
// call means the same) and leaves every other command to musl. It changes no
// build output. Remove it once Ginux ignores the bits Linux's F_SETFL ignores.
const FCNTL_SHIM_C = `#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdarg.h>
#include <sys/syscall.h>
#include <unistd.h>

int fcntl(int fd, int cmd, ...)
{
    static int (*real)(int, int, ...);
    va_list ap;
    va_start(ap, cmd);
    unsigned long arg = va_arg(ap, unsigned long);
    va_end(ap);
    if (cmd == F_SETFL)
        return syscall(SYS_fcntl, fd, cmd, arg & ~(unsigned long)O_LARGEFILE);
    if (!real)
        real = (int (*)(int, int, ...))dlsym(RTLD_NEXT, "fcntl");
    return real(fd, cmd, arg);
}
`;

// The preload for cargo and rustc on a musl root, built with the root's own cc
// into the work folder (never into the tree); "" on a glibc root, whose
// fcntl() passes F_SETFL's argument through unchanged.
fn fcntl_shim() {
    if !is_file("/lib/ld-musl-x86_64.so.1") { return ""; }
    let src = join_path(WORK, "fcntl-setfl-shim.c");
    let lib = join_path(WORK, "fcntl-setfl-shim.so");
    write_file(src, FCNTL_SHIM_C);
    exec_checked("cc", ["-shared", "-fPIC", "-O2", "-o", lib, src], #{ env: cargo_env() });
    lib
}

fn check_defines() {
    if DEFINES.OS_ID != "levitateos" && DEFINES.OS_ID != "acornos" {
        throw "reclogin.rhai needs --define OS_ID=levitateos or acornos (the variant's identity.os_id), got '" + DEFINES.OS_ID + "'";
    }
    if DEFINES.INIT != "systemd" && DEFINES.INIT != "openrc" {
        throw "reclogin.rhai needs --define INIT=systemd or openrc (the variant's profile.init), got '" + DEFINES.INIT + "'";
    }
}

// The source closure the builder prepares.
fn check_source() {
    for path in [join_path(INPUTS.source, "Cargo.toml"), join_path(INPUTS.source, "Cargo.lock"), join_path(crate_dir(), "Cargo.toml")] {
        if !is_file(path) {
            throw "the source closure lacks " + path + "; import the offline source closure of tools/reclogin with --input source=PATH (the builder prepares it)";
        }
    }
    if !is_dir(join_path(INPUTS.source, "vendor")) {
        throw "the source closure has no vendored crates; import the offline source closure of tools/reclogin with --input source=PATH (the builder prepares it)";
    }
}

// The tree, in one place: [path in the tree, mode, where it comes from]. The
// source is a built program ("program:<name>") or a file of the crate,
// relative to tools/reclogin. reclogind finds the greeter next to itself
// (/usr/bin) and the brand folder at /usr/share/reclogin (src/brand.rs), the
// fonts in its fonts/ folder; the systemd unit and the OpenRC wrapper start
// /usr/bin/reclogind; the OpenRC service starts the wrapper at
// /usr/libexec/reclogin/reclogind-run. Recipe makes the accounts `reclogin`
// and `guest` from usr/lib/sysusers.d (on both systems), and the group recipe
// declares them again.
fn installed_files() {
    let os_id = DEFINES.OS_ID;
    let brand = "usr/share/reclogin/";
    let files = [
        ["usr/bin/reclogind", 0o755, "program:reclogind"],
        ["usr/bin/reclogin-greeter", 0o755, "program:reclogin-greeter"],
        ["etc/pam.d/reclogin", 0o644, "dist/pam/reclogin." + os_id],
        ["etc/pam.d/reclogin-autologin", 0o644, "dist/pam/reclogin-autologin." + os_id],
        ["etc/reclogin.conf", 0o644, "dist/reclogin.conf"],
        ["usr/lib/sysusers.d/reclogin.conf", 0o644, "dist/sysusers/reclogin.conf"],
        [brand + "brand.json", 0o644, "assets/" + os_id + "/brand.json"],
        [brand + "background.png", 0o644, "assets/" + os_id + "/background.png"],
    ];
    // The guest (stage 7): where tools/reclogin/README.md, "Files under dist/
    // for the guest", installs each file. The account comes with the
    // sysusers.d file above.
    for file in [
        ["etc/pam.d/reclogin-guest", 0o644, "dist/pam/reclogin-guest." + os_id],
        ["etc/security/limits.d/50-reclogin-guest.conf", 0o644, "dist/guest/50-reclogin-guest.limits.conf"],
        ["etc/ssh/sshd_config.d/50-reclogin-guest.conf", 0o644, "dist/guest/50-reclogin-guest.sshd.conf"],
        ["usr/share/polkit-1/rules.d/50-reclogin-guest.rules", 0o644, "dist/guest/50-reclogin-guest.rules"],
        ["usr/libexec/reclogin/reclogin-guest-notice", 0o755, "dist/guest/reclogin-guest-notice"],
        [brand + "guest-home/.config/autostart/reclogin-guest-notice.desktop", 0o644,
            "dist/guest/guest-home/.config/autostart/reclogin-guest-notice.desktop"],
    ] { files.push(file); }
    for font in ["Newsreader.ttf", "Newsreader-Italic.ttf", "NewsreaderDisplay.ttf",
                 "NewsreaderDisplay-Italic.ttf", "InstrumentSans.ttf",
                 "OFL-Newsreader.txt", "OFL-InstrumentSans.txt"] {
        files.push([brand + "fonts/" + font, 0o644, "assets/fonts/" + font]);
    }
    if DEFINES.INIT == "systemd" {
        files.push(["usr/lib/systemd/system/reclogin.service", 0o644, "dist/systemd/reclogin.service"]);
        // The guest's own systemd --user and what it starts are not children of
        // the desktop process, so reclogin's kernel filter does not reach them.
        files.push(["usr/lib/systemd/system/user@60600.service.d/50-reclogin-guest.conf", 0o644,
            "dist/guest/user@60600.service.d/50-reclogin-guest.conf"]);
    } else {
        files.push(["etc/init.d/reclogin", 0o755, "dist/openrc/reclogin"]);
        files.push(["usr/libexec/reclogin/reclogind-run", 0o755, "dist/openrc/reclogind-run"]);
    }
    files
}

// Links in the tree: [path, target]. On systemd the unit's Alias=, the link
// `systemctl enable` makes; graphical.target wants display-manager.service.
// OpenRC's runlevel link is the group recipe's to make.
fn installed_links() {
    if DEFINES.INIT == "systemd" {
        [["etc/systemd/system/display-manager.service", "/usr/lib/systemd/system/reclogin.service"]]
    } else {
        []
    }
}

fn build(src, out) {
    check_defines();
    check_source();
    for tool in ["/usr/bin/cargo", "/usr/bin/rustc", "/usr/bin/cc"] {
        if !is_file(tool) {
            throw "this root has no " + tool + ": reclogin is compiled in the variant's apps-build root (desktop.apps_build_packages), not in Recipe's usual one";
        }
    }
    let env = cargo_env();
    let shim = fcntl_shim();
    if shim != "" { env.LD_PRELOAD = shim; }
    // The workspace's rust-version is the floor cargo enforces; the log says
    // which toolchain this distro has, for when it refuses.
    log("toolchain: " + trim(exec_output("rustc", ["--version"], #{ env: env })) + ", " +
        trim(exec_output("cargo", ["--version"], #{ env: env })));
    if is_dir(target_dir()) { rm(target_dir()); }
    progress(0, 0, "compiling reclogind and reclogin-greeter (cargo build)");
    let args = ["build", "--release", "--locked", "--offline",
        "--manifest-path", join_path(INPUTS.source, "Cargo.toml"),
        "--target-dir", target_dir(),
        "--config", "source.vendored-sources.directory=\"" + join_path(INPUTS.source, "vendor") + "\"",
        "-p", "reclogin"];
    for program in PROGRAMS { args.push("--bin"); args.push(program); }
    exec_checked("cargo", args, #{ cwd: INPUTS.source, env: env });
    // Each program runs here, in the distro's own root, with the libraries it
    // links: `<program> --version` names it.
    let checked = 0;
    for program in PROGRAMS {
        if !is_file(built(program)) { throw "the build wrote no " + program; }
        let version = trim(exec_output(built(program), ["--version"], #{ env: env }));
        if !version.starts_with(program + " ") { throw "unexpected " + program + " program: " + version; }
        checked += 1;
        progress(checked, PROGRAMS.len(), "programs built");
    }

    progress(0, 0, "laying out the tree");
    for file in installed_files() {
        let from = if file[2].starts_with("program:") {
            built(file[2].sub_string(8))
        } else {
            join_path(crate_dir(), file[2])
        };
        if !is_file(from) { throw "tools/reclogin has no " + file[2] + " for /" + file[0]; }
        let dest = join_path(out, file[0]);
        mkdir(dirname(dest));
        copy_file(from, dest);
        chmod(dest, file[1]);
        target_mode(out, file[0], file[1]);
    }
    for link in installed_links() {
        let dest = join_path(out, link[0]);
        mkdir(dirname(dest));
        ln_force(link[1], dest);
    }
}

fn verify(out) {
    for file in installed_files() {
        if !is_file(join_path(out, file[0])) { throw "/" + file[0] + " is not installed"; }
    }
    for link in installed_links() {
        if !exists(join_path(out, link[0])) { throw "/" + link[0] + " is not installed"; }
    }
}
