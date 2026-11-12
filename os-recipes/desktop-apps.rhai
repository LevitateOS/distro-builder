// Compiles the desktop apps (gui/: recipe-gui, recab-gui, disk-gui and the
// helper levitate-agent) and publishes the tree they install: the programs in
// usr/bin, the three apps' menu entries and their polkit actions.
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
    timeout_secs: 7200,
};
let ctx = #{
    name: "desktop-apps",
    programs: ["recipe-gui", "recab-gui", "disk-gui", "levitate-agent"],
    jobs: if is_def_var("JOBS") { JOBS } else { "1" },
};

fn source(ctx) { join_path(ctx.runtime.paths.inputs, "source") }
fn work(ctx) { join_path(ctx.runtime.paths.build, "cmake") }
fn tree(ctx) { join_path(ctx.runtime.paths.output, "rootfs") }
// Where the one build of all four puts each program: the apps in the folder of
// their first name part, the helper in agent/.
fn built(ctx, program) {
    let folder = if program == "levitate-agent" { "agent" } else { program.split("-")[0] };
    join_path(work(ctx), folder + "/" + program)
}
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

// Configure; when CMake cannot find its build tools, say what this root's
// PATH and tools look like before failing, since CMake only reports "not set".
fn configure(ctx, dir) {
    try {
        exec_checked("cmake", ["-S", source(ctx), "-B", dir, "-G", "Unix Makefiles",
            "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_INSTALL_PREFIX=/usr"], #{ env: tools_env(ctx) });
    } catch (err) {
        report_tools(ctx);
        throw err;
    }
}

fn report_tools(ctx) {
    let probe = "echo \"PATH=$PATH\"; for t in make gmake cc gcc c++ g++; do " +
        "printf '%s: ' \"$t\"; command -v \"$t\" || echo missing; done; " +
        "ls -l /usr/bin/make /usr/bin/gcc /usr/bin/g++ /usr/bin/cc 2>&1; " +
        "for t in /usr/bin/make /usr/bin/gcc /usr/bin/g++; do " +
        "test -x \"$t\" && echo \"$t: executable\" || echo \"$t: not executable\"; done; " +
        "/usr/bin/make --version 2>&1 | head -1";
    try { log("tools in this root:\n" + exec_output("sh", ["-c", probe], #{ env: tools_env(ctx) })); }
    catch (err) { log("could not inspect this root's tools: " + err); }
    // CMake's own account of where it looked for the build program, and why
    // each candidate was refused.
    let trace = "cmake -S \"$1\" -B \"$2\" -G 'Unix Makefiles' --debug-find-var=CMAKE_MAKE_PROGRAM 2>&1 | " +
        "grep -v '^$' | tail -60";
    try {
        log("cmake's search for the build program:\n" + exec_output("sh",
            ["-c", trace, "trace", source(ctx), join_path(ctx.runtime.paths.build, "cmake-trace")],
            #{ env: tools_env(ctx) }));
    } catch (err) { log("could not trace cmake's search: " + err); }
}

fn build(ctx) {
    if !is_file("/usr/bin/cmake") {
        throw "this root has no cmake: the desktop apps are compiled in the variant's apps-build root, not in Recipe's usual one";
    }
    let dir = work(ctx);
    if is_dir(dir) { rm(dir); }
    progress(0, 0, "configuring (cmake)");
    configure(ctx, dir);
    progress(0, ctx.programs.len(), "compiling the apps (cmake --build)");
    exec_checked("cmake", ["--build", dir, "--parallel", ctx.jobs], #{ env: tools_env(ctx) });
    let found = 0;
    for program in ctx.programs {
        if !is_file(built(ctx, program)) { throw "the build wrote no " + program; }
        found += 1;
        progress(found, ctx.programs.len(), "programs built");
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
    progress(0, 0, "installing into the tree (cmake --install)");
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
