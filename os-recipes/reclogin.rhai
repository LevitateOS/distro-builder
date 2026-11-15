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
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ source: "directory" },
    outputs: #{ rootfs: "directory" },
    memory_mib: 8192,
    storage_max_bytes: 6442450944,
    storage_max_objects: 262144,
    timeout_secs: 7200,
};
let ctx = #{
    name: "reclogin",
    os_id: if is_def_var("OS_ID") { OS_ID } else { "" },
    init: if is_def_var("INIT") { INIT } else { "" },
    programs: ["reclogind", "reclogin-greeter"],
};

fn source(ctx) { join_path(ctx.runtime.paths.inputs, "source") }
// The crate inside the closure, at its path in the repository.
fn crate_dir(ctx) { join_path(source(ctx), "tools/reclogin") }
fn target_dir(ctx) { join_path(ctx.runtime.paths.build, "target") }
fn built(ctx, program) { join_path(target_dir(ctx), "release/" + program) }
fn tree(ctx) { join_path(ctx.runtime.paths.output, "rootfs") }

// cargo's own state in the build folder; the vendored crates are the only
// source (the closure's .cargo/config.toml names them at /inputs/source/vendor,
// and --config below names them where this run sees them).
fn cargo_env(ctx) {
    #{
        HOME: ctx.runtime.paths.build,
        LC_ALL: "C",
        CARGO_HOME: join_path(ctx.runtime.paths.build, "cargo"),
        CARGO_BUILD_JOBS: ctx.runtime.options.jobs.to_string(),
        CARGO_INCREMENTAL: "0",
        CARGO_PROFILE_RELEASE_STRIP: "true",
    }
}

fn check_defines(ctx) {
    if ctx.os_id != "levitateos" && ctx.os_id != "acornos" {
        throw "reclogin.rhai needs --define OS_ID=levitateos or acornos (the variant's identity.os_id), got '" + ctx.os_id + "'";
    }
    if ctx.init != "systemd" && ctx.init != "openrc" {
        throw "reclogin.rhai needs --define INIT=systemd or openrc (the variant's profile.init), got '" + ctx.init + "'";
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
fn installed_files(ctx) {
    let brand = "usr/share/reclogin/";
    let files = [
        ["usr/bin/reclogind", 0o755, "program:reclogind"],
        ["usr/bin/reclogin-greeter", 0o755, "program:reclogin-greeter"],
        ["etc/pam.d/reclogin", 0o644, "dist/pam/reclogin." + ctx.os_id],
        ["etc/pam.d/reclogin-autologin", 0o644, "dist/pam/reclogin-autologin." + ctx.os_id],
        ["etc/reclogin.conf", 0o644, "dist/reclogin.conf"],
        ["usr/lib/sysusers.d/reclogin.conf", 0o644, "dist/sysusers/reclogin.conf"],
        [brand + "brand.json", 0o644, "assets/" + ctx.os_id + "/brand.json"],
        [brand + "background.png", 0o644, "assets/" + ctx.os_id + "/background.png"],
    ];
    // The guest (stage 7): where tools/reclogin/README.md, "Files under dist/
    // for the guest", installs each file. The account comes with the
    // sysusers.d file above.
    for file in [
        ["etc/pam.d/reclogin-guest", 0o644, "dist/pam/reclogin-guest." + ctx.os_id],
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
    if ctx.init == "systemd" {
        files.push(["usr/lib/systemd/system/reclogin.service", 0o644, "dist/systemd/reclogin.service"]);
    } else {
        files.push(["etc/init.d/reclogin", 0o755, "dist/openrc/reclogin"]);
        files.push(["usr/libexec/reclogin/reclogind-run", 0o755, "dist/openrc/reclogind-run"]);
    }
    files
}

// Links in the tree: [path, target]. On systemd the unit's Alias=, the link
// `systemctl enable` makes; graphical.target wants display-manager.service.
// OpenRC's runlevel link is the group recipe's to make.
fn installed_links(ctx) {
    if ctx.init == "systemd" {
        [["etc/systemd/system/display-manager.service", "/usr/lib/systemd/system/reclogin.service"]]
    } else {
        []
    }
}

fn is_acquired(ctx) {
    check_defines(ctx);
    for path in [join_path(source(ctx), "Cargo.toml"), join_path(source(ctx), "Cargo.lock"), join_path(crate_dir(ctx), "Cargo.toml")] {
        if !is_file(path) { return needs_work(ctx, "the source closure lacks " + path); }
    }
    if !is_dir(join_path(source(ctx), "vendor")) { return needs_work(ctx, "the source closure has no vendored crates"); }
    ready(ctx)
}

fn acquire(ctx) {
    check_defines(ctx);
    if is_file(join_path(crate_dir(ctx), "Cargo.toml")) && is_dir(join_path(source(ctx), "vendor")) { return ctx; }
    throw "Import the offline source closure of tools/reclogin with --input source=PATH (the builder prepares it)";
}

fn is_built(ctx) {
    for program in ctx.programs {
        if !is_file(built(ctx, program)) { return needs_work(ctx, program + " needs compilation"); }
    }
    ready(ctx)
}

fn build(ctx) {
    check_defines(ctx);
    for tool in ["/usr/bin/cargo", "/usr/bin/rustc", "/usr/bin/cc"] {
        if !is_file(tool) {
            throw "this root has no " + tool + ": reclogin is compiled in the variant's apps-build root (desktop.apps_build_packages), not in Recipe's usual one";
        }
    }
    let env = cargo_env(ctx);
    // The workspace's rust-version is the floor cargo enforces; the log says
    // which toolchain this distro has, for when it refuses.
    log("toolchain: " + trim(exec_output("rustc", ["--version"], #{ env: env })) + ", " +
        trim(exec_output("cargo", ["--version"], #{ env: env })));
    if is_dir(target_dir(ctx)) { rm(target_dir(ctx)); }
    progress(0, 0, "compiling reclogind and reclogin-greeter (cargo build)");
    let args = ["build", "--release", "--locked", "--offline",
        "--manifest-path", join_path(source(ctx), "Cargo.toml"),
        "--target-dir", target_dir(ctx),
        "--config", "source.vendored-sources.directory=\"" + join_path(source(ctx), "vendor") + "\"",
        "-p", "reclogin"];
    for program in ctx.programs { args.push("--bin"); args.push(program); }
    exec_checked("cargo", args, #{ cwd: source(ctx), env: env });
    // Each program runs here, in the distro's own root, with the libraries it
    // links: `<program> --version` names it.
    let checked = 0;
    for program in ctx.programs {
        if !is_file(built(ctx, program)) { throw "the build wrote no " + program; }
        let version = trim(exec_output(built(ctx, program), ["--version"], #{ env: env }));
        if !version.starts_with(program + " ") { throw "unexpected " + program + " program: " + version; }
        checked += 1;
        progress(checked, ctx.programs.len(), "programs built");
    }
    ctx
}

fn tree_gap(ctx, root) {
    for file in installed_files(ctx) {
        if !is_file(join_path(root, file[0])) { return "/" + file[0] + " is not installed"; }
    }
    for link in installed_links(ctx) {
        if !exists(join_path(root, link[0])) { return "/" + link[0] + " is not installed"; }
    }
    ""
}

fn is_installed(ctx) {
    check_defines(ctx);
    let gap = tree_gap(ctx, tree(ctx));
    if gap != "" { return needs_work(ctx, gap); }
    ctx.outputs = #{ rootfs: tree(ctx) };
    ready(ctx)
}

fn install(ctx) {
    check_defines(ctx);
    let root = tree(ctx);
    if is_dir(root) { rm(root); }
    mkdir(root);
    progress(0, 0, "laying out the tree");
    for file in installed_files(ctx) {
        let from = if file[2].starts_with("program:") {
            built(ctx, file[2].sub_string(8))
        } else {
            join_path(crate_dir(ctx), file[2])
        };
        if !is_file(from) { throw "tools/reclogin has no " + file[2] + " for /" + file[0]; }
        let dest = join_path(root, file[0]);
        mkdir(dirname(dest));
        copy_file(from, dest);
        chmod(dest, file[1]);
        target_mode(root, file[0], file[1]);
    }
    for link in installed_links(ctx) {
        let dest = join_path(root, link[0]);
        mkdir(dirname(dest));
        ln_force(link[1], dest);
    }
    let gap = tree_gap(ctx, root);
    if gap != "" { throw gap; }
    ctx.outputs = #{ rootfs: root };
    ctx
}

fn remove(ctx) {
    if is_dir(tree(ctx)) { rm(tree(ctx)); }
    ctx.outputs = #{};
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" {
        for directory in [target_dir(ctx), join_path(ctx.runtime.paths.build, "cargo")] {
            if is_dir(directory) { rm(directory); }
        }
    }
    ctx
}
