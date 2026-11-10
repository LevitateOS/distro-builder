// Compiles the desktop apps (gui/: recipe-gui, recab-gui, disk-gui) and
// publishes the tree they install: the programs in usr/bin, their menu
// entries and their polkit actions.
//   --input source=<the gui/ folder>
// This recipe does not run in Recipe's usual root. The builder runs it in the
// variant's apps-build root (the `*-apps-build-root.rhai` recipe's tree, as a
// root image), so `cmake` and the compiler it finds are the distro's own and
// the programs link the C library, C++ runtime and Qt the desktop system
// ships. JOBS is how many compilers run at once (default 1).
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ source: "directory" },
    outputs: #{ rootfs: "directory" },
    memory_mib: 8192,
    timeout_secs: 3600,
};
let ctx = #{
    name: "desktop-apps",
    programs: ["recipe-gui", "recab-gui", "disk-gui"],
    jobs: if is_def_var("JOBS") { JOBS } else { "1" },
};

fn source(ctx) { join_path(ctx.runtime.paths.inputs, "source") }
fn work(ctx) { join_path(ctx.runtime.paths.build, "cmake") }
fn tree(ctx) { join_path(ctx.runtime.paths.output, "rootfs") }
// Where the one build of all three puts each program.
fn built(ctx, program) { join_path(work(ctx), program.split("-")[0] + "/" + program) }
fn tools_env(ctx) { #{ HOME: ctx.runtime.paths.build, LC_ALL: "C" } }

fn is_acquired(ctx) {
    if is_file(join_path(source(ctx), "CMakeLists.txt")) { return ready(ctx); }
    needs_work(ctx, "the desktop apps' source input is missing")
}

fn acquire(ctx) {
    if is_file(join_path(source(ctx), "CMakeLists.txt")) { return ctx; }
    throw "Import the gui/ folder with --input source=PATH";
}

fn is_built(ctx) {
    for program in ctx.programs {
        if !is_file(built(ctx, program)) { return needs_work(ctx, program + " needs compilation"); }
    }
    ready(ctx)
}

fn build(ctx) {
    if !is_file("/usr/bin/cmake") {
        throw "this root has no cmake: the desktop apps are compiled in the variant's apps-build root, not in Recipe's usual one";
    }
    let dir = work(ctx);
    if is_dir(dir) { rm(dir); }
    exec_checked("cmake", ["-S", source(ctx), "-B", dir, "-G", "Unix Makefiles",
        "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_INSTALL_PREFIX=/usr"], #{ env: tools_env(ctx) });
    exec_checked("cmake", ["--build", dir, "--parallel", ctx.jobs], #{ env: tools_env(ctx) });
    for program in ctx.programs {
        if !is_file(built(ctx, program)) { throw "the build wrote no " + program; }
    }
    ctx
}

// What installing puts into the tree, with the mode each file gets.
fn installed_files(ctx) {
    let files = [];
    for program in ctx.programs { files.push(["usr/bin/" + program, 0o755]); }
    for name in ["recipe", "recab", "disk"] { files.push(["usr/share/applications/org.levitateos." + name + ".desktop", 0o644]); }
    for name in ["recipe", "recab"] { files.push(["usr/share/polkit-1/actions/org.levitateos." + name + ".policy", 0o644]); }
    files
}

fn is_installed(ctx) {
    for file in installed_files(ctx) {
        if !is_file(join_path(tree(ctx), file[0])) { return needs_work(ctx, "/" + file[0] + " is not installed"); }
    }
    ctx.outputs = #{ rootfs: tree(ctx) };
    ready(ctx)
}

fn install(ctx) {
    let root = tree(ctx);
    if is_dir(root) { rm(root); }
    mkdir(root);
    let env = tools_env(ctx);
    env.DESTDIR = root;
    exec_checked("cmake", ["--install", work(ctx)], #{ env: env });
    for file in installed_files(ctx) {
        if !is_file(join_path(root, file[0])) { throw "installing wrote no /" + file[0]; }
        target_mode(root, file[0], file[1]);
    }
    ctx.outputs = #{ rootfs: root };
    ctx
}

fn remove(ctx) {
    if is_dir(tree(ctx)) { rm(tree(ctx)); }
    ctx.outputs = #{};
    ctx
}
