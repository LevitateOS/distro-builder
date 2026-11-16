const RECIPE_API = 1;
// Compiles the desktop apps (tools/gui/: recipe-gui, recab-gui, disk-gui) and
// publishes the tree they install: the programs in usr/bin, the three apps'
// menu entries and their polkit actions.
//   --input source=<the tools/gui/ folder>
// This recipe does not run in Recipe's usual root. The builder runs it in the
// variant's apps-build root (the `*-apps-build-root.rhai` recipe's tree, as a
// root image), so `cmake` and the compiler it finds are the distro's own and
// the programs link the C library, C++ runtime and Qt the desktop system
// ships. JOBS is how many compilers run at once.
const NEEDS = #{
    inputs: #{ source: "directory" },
    outputs: #{ rootfs: "directory" },
    memory_mib: 8192,
    timeout_secs: 7200,
};
const PACKAGE = #{ name: "desktop-apps" };
const PROGRAMS = ["recipe-gui", "recab-gui", "disk-gui"];

fn work() { join_path(WORK, "cmake") }
// Where the one build of all three puts each program: in the folder of its
// first name part.
fn built(program) {
    let folder = program.split("-")[0];
    join_path(work(), folder + "/" + program)
}
fn tools_env() { #{ HOME: WORK, LC_ALL: "C" } }

// Configure; when CMake cannot find its build tools, say what this root's
// PATH and tools look like before failing, since CMake only reports "not set".
fn cmake_configure(dir) {
    try {
        exec_checked("cmake", ["-S", INPUTS.source, "-B", dir, "-G", "Unix Makefiles",
            "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_INSTALL_PREFIX=/usr"], #{ env: tools_env() });
    } catch (err) {
        report_tools();
        throw err;
    }
}

fn report_tools() {
    let probe = "echo \"PATH=$PATH\"; for t in make gmake cc gcc c++ g++; do " +
        "printf '%s: ' \"$t\"; command -v \"$t\" || echo missing; done; " +
        "ls -l /usr/bin/make /usr/bin/gcc /usr/bin/g++ /usr/bin/cc 2>&1; " +
        "for t in /usr/bin/make /usr/bin/gcc /usr/bin/g++; do " +
        "test -x \"$t\" && echo \"$t: executable\" || echo \"$t: not executable\"; done; " +
        "/usr/bin/make --version 2>&1 | head -1";
    try { log("tools in this root:\n" + exec_output("sh", ["-c", probe], #{ env: tools_env() })); }
    catch (err) { log("could not inspect this root's tools: " + err); }
    // CMake's own account of where it looked for the build program, and why
    // each candidate was refused.
    let trace = "cmake -S \"$1\" -B \"$2\" -G 'Unix Makefiles' --debug-find-var=CMAKE_MAKE_PROGRAM 2>&1 | " +
        "grep -v '^$' | tail -60";
    try {
        log("cmake's search for the build program:\n" + exec_output("sh",
            ["-c", trace, "trace", INPUTS.source, join_path(WORK, "cmake-trace")],
            #{ env: tools_env() }));
    } catch (err) { log("could not trace cmake's search: " + err); }
}

// What installing puts into the tree, with the mode each file gets.
fn installed_files() {
    let files = [];
    for program in PROGRAMS { files.push(["usr/bin/" + program, 0o755]); }
    for name in ["recipe", "recab", "disk"] { files.push(["usr/share/applications/org.levitateos." + name + ".desktop", 0o644]); }
    for name in ["recipe", "recab", "disk"] { files.push(["usr/share/icons/hicolor/scalable/apps/org.levitateos." + name + ".svg", 0o644]); }
    for name in ["recipe", "recab"] { files.push(["usr/share/polkit-1/actions/org.levitateos." + name + ".policy", 0o644]); }
    files
}

fn build(src, out) {
    if !is_file(join_path(INPUTS.source, "CMakeLists.txt")) {
        throw "Import the tools/gui/ folder with --input source=PATH";
    }
    if !is_file("/usr/bin/cmake") {
        throw "this root has no cmake: the desktop apps are compiled in the variant's apps-build root, not in Recipe's usual one";
    }
    let dir = work();
    if is_dir(dir) { rm(dir); }
    progress(0, 0, "configuring (cmake)");
    cmake_configure(dir);
    progress(0, PROGRAMS.len(), "compiling the apps (cmake --build)");
    exec_checked("cmake", ["--build", dir, "--parallel", JOBS.to_string()], #{ env: tools_env() });
    let found = 0;
    for program in PROGRAMS {
        if !is_file(built(program)) { throw "the build wrote no " + program; }
        found += 1;
        progress(found, PROGRAMS.len(), "programs built");
    }

    let env = tools_env();
    env.DESTDIR = out;
    progress(0, 0, "installing into the tree (cmake --install)");
    exec_checked("cmake", ["--install", dir], #{ env: env });
    for file in installed_files() {
        if !is_file(join_path(out, file[0])) { throw "installing wrote no /" + file[0]; }
        target_mode(out, file[0], file[1]);
    }
}

fn verify(out) {
    for file in installed_files() {
        if !is_file(join_path(out, file[0])) { throw "/" + file[0] + " is not installed"; }
    }
}
