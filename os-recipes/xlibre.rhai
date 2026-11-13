// XLibre, the X server AcornOS's desktop runs (the fork of the X.Org server,
// https://github.com/X11Libre), with XLibre's libinput input driver, compiled
// from pinned source and published as one tree laid out like Alpine's
// xorg-server package, so SDDM's /usr/bin/X keeps working:
//   usr/bin/X -> Xorg (a sh script) -> usr/libexec/Xorg.wrap (setuid root,
//   through the target metadata) -> usr/libexec/Xorg; modules under
//   usr/lib/xorg/modules/xlibre-25 (the modesetting video driver is built in);
//   usr/share/X11/xorg.conf.d/{10-quirks,40-libinput}.conf; var/lib/xkb (empty).
// Alpine has no XLibre package. Keeping it current is this project's job:
// XLibre announces fixes only in its release notes, so treat every point
// release of the 25.1 line as a security release and move both pins below.
//
// This recipe does not run in Recipe's usual root. The builder runs it in the
// variant's apps-build root (as for desktop-apps.rhai), so meson, the compiler
// and the -dev packages it finds are the distro's own and the server links the
// libraries the desktop system ships. The server's SDK (headers, .pc files) is
// only installed into a staging tree the driver is compiled against; it is not
// in the published tree. JOBS is how many compilers run at once (default 1).
const RECIPE_API = 2;
const NEEDS = #{
    outputs: #{ rootfs: "directory" },
    network: ["github.com:443"],
    memory_mib: 8192,
    storage_max_objects: 262144,
    timeout_secs: 10800,
};
let ctx = #{
    name: "xlibre",
    description: "The XLibre X server and its libinput driver for AcornOS",
    jobs: if is_def_var("JOBS") { JOBS } else { "1" },
    // [name, git URL, tag (informational), full commit]
    sources: [
        ["xserver", "https://github.com/X11Libre/xserver",
            "xlibre-xserver-25.1.11", "ec1f2d37daa1450d8452abb547de4546321d7bb9"],
        ["xf86-input-libinput", "https://github.com/X11Libre/xf86-input-libinput",
            "xlibre-xf86-input-libinput-25.0.1", "4eb6691efeef4969a05b1d6b77d980943fb9760e"],
    ],
    // The server's ABI directory under the module path: XLibre loads modules
    // from here first, and only modules built against its SDK.
    abi: "xlibre-25",
};

fn src(ctx, name) { join_path(ctx.runtime.paths.build, "src/" + name) }
fn server_build(ctx) { join_path(ctx.runtime.paths.build, "build-xserver") }
fn driver_build(ctx) { join_path(ctx.runtime.paths.build, "build-libinput") }
// The server installed with its SDK, for the driver's compile only.
fn sdk(ctx) { join_path(ctx.runtime.paths.build, "sdk") }
fn sdk_pc(ctx) { join_path(ctx.runtime.paths.build, "sdk-pkgconfig") }
fn tree(ctx) { join_path(ctx.runtime.paths.output, "rootfs") }
fn modules(ctx) { "usr/lib/xorg/modules/" + ctx.abi }

// Alpine's own flags (abuild's defaults) plus -z lazy, which Alpine's
// xorg-server build adds: Alpine's gcc links with -z now by default, and the
// server's modules resolve symbols of each other and of the server when they
// load, which fails under -z now.
fn tools_env(ctx) {
    #{
        HOME: ctx.runtime.paths.build,
        LC_ALL: "C",
        CFLAGS: "-Os -fstack-clash-protection -Wformat -Werror=format-security",
        LDFLAGS: "-Wl,--as-needed,-O1,--sort-common -Wl,-z,lazy",
    }
}

// The server's meson options: Alpine 3.24's xorg-server build, without the
// nested and virtual servers and the tests, and with the choices XLibre needs
// stated rather than probed (no libseat: rootful through the setuid wrapper,
// elogind as today; SHA-1 from nettle, which Alpine's probe also finds; the
// keymap compiler's directory, which the probe would read from xkbcomp-dev;
// the inputtest driver Alpine also ships is a test aid).
fn server_options() {
    [
        "-Dxorg=true", "-Dxephyr=false", "-Dxnest=false", "-Dxvfb=false", "-Dxfbdev=false",
        "-Dxwin=false", "-Dxquartz=false", "-Dtests=false", "-Ddocs=false", "-Ddevel-docs=false",
        "-Dglamor=true", "-Dglx=true", "-Dxdmcp=true", "-Dxdm-auth-1=true", "-Dxcsecurity=true",
        "-Dsuid_wrapper=true", "-Dudev=true", "-Dudev_kms=true", "-Dsystemd_logind=false",
        "-Dsystemd_notify=false", "-Dseatd_libseat=false", "-Dhal=false", "-Ddri2=true",
        "-Ddri3=true", "-Dxf86-input-inputtest=false", "-Dlibunwind=false",
        "-Dxkb_dir=/usr/share/X11/xkb", "-Dxkb_output_dir=/var/lib/xkb", "-Dxkb_bin_dir=/usr/bin",
        "-Dlisten_tcp=false", "-Dsha1=libnettle",
        "-Ddefault_font_path=/usr/share/fonts/misc,/usr/share/fonts/100dpi:unscaled,/usr/share/fonts/75dpi:unscaled,/usr/share/fonts/TTF,/usr/share/fonts/Type1",
    ]
}

fn setup_args(source, dir, extra) {
    let args = ["setup", dir, source, "--prefix=/usr", "--libdir=lib", "--libexecdir=libexec",
        "--sysconfdir=/etc", "--localstatedir=/var", "--buildtype=plain", "--wrap-mode=nofallback",
        "-Db_staticpic=true"];
    for option in extra { args.push(option); }
    args
}

fn checkout_at(dir, commit) {
    if !is_file(join_path(dir, "meson.build")) { return false; }
    trim(exec_output("git", ["-C", dir, "rev-parse", "HEAD"])) == commit
}

fn is_acquired(ctx) {
    for source in ctx.sources {
        if !checkout_at(src(ctx, source[0]), source[3]) {
            return needs_work(ctx, source[0] + " is not checked out at " + source[2]);
        }
    }
    ready(ctx)
}

fn acquire(ctx) {
    mkdir(join_path(ctx.runtime.paths.build, "src"));
    for source in ctx.sources {
        let dir = src(ctx, source[0]);
        if is_dir(dir) && !checkout_at(dir, source[3]) { rm(dir); }
        progress(0, 0, "fetching " + source[2]);
        git_checkout(source[1], dir, source[3]);
    }
    ctx
}

// What the two compiles leave in their build directories.
fn built_files(ctx) {
    [
        join_path(server_build(ctx), "hw/xfree86/Xorg"),
        join_path(server_build(ctx), "hw/xfree86/Xorg.wrap"),
        join_path(server_build(ctx), "hw/xfree86/drivers/video/modesetting/modesetting_drv.so"),
        join_path(driver_build(ctx), "libinput_drv.so"),
    ]
}

fn is_built(ctx) {
    for file in built_files(ctx) {
        if !is_file(file) { return needs_work(ctx, file + " is not compiled"); }
    }
    ready(ctx)
}

fn build(ctx) {
    if !is_file("/usr/bin/meson") {
        throw "this root has no meson: XLibre is compiled in the variant's apps-build root, not in Recipe's usual one";
    }
    let env = tools_env(ctx);
    for dir in [server_build(ctx), driver_build(ctx), sdk(ctx), sdk_pc(ctx)] {
        if is_dir(dir) { rm(dir); }
    }

    progress(0, 4, "configuring the server (meson)");
    exec_checked("meson", setup_args(src(ctx, "xserver"), server_build(ctx), server_options()), #{ env: env });
    progress(1, 4, "compiling the server");
    exec_checked("meson", ["compile", "-C", server_build(ctx), "-j", ctx.jobs], #{ env: env });

    // The driver is compiled against the server just built, through its SDK
    // in a staging tree: the copied xorg-server.pc points its prefix there.
    let stage_env = tools_env(ctx);
    stage_env.DESTDIR = sdk(ctx);
    exec_checked("meson", ["install", "-C", server_build(ctx), "--no-rebuild"], #{ env: stage_env });
    // meson may have set the wrapper's setuid bit; no staging file keeps one.
    let staged_wrapper = join_path(sdk(ctx), "usr/libexec/Xorg.wrap");
    if is_file(staged_wrapper) { chmod(staged_wrapper, 0o755); }
    let pc = join_path(sdk(ctx), "usr/lib/pkgconfig/xorg-server.pc");
    if !is_file(pc) { throw "the server installed no xorg-server.pc (its SDK)"; }
    mkdir(sdk_pc(ctx));
    let copied = join_path(sdk_pc(ctx), "xorg-server.pc");
    copy_file(pc, copied);
    replace_in_file(copied, "prefix=/usr\n", "prefix=" + join_path(sdk(ctx), "usr") + "\n");
    let driver_env = tools_env(ctx);
    driver_env.PKG_CONFIG_PATH = sdk_pc(ctx);

    progress(2, 4, "configuring the libinput driver (meson)");
    exec_checked("meson", setup_args(src(ctx, "xf86-input-libinput"), driver_build(ctx), [
        "-Dsdkdir=/usr/include/xorg",
        "-Dxorg-module-dir=/usr/lib/xorg/modules/" + ctx.abi + "/input",
        "-Dxorg-conf-dir=/usr/share/X11/xorg.conf.d",
    ]), #{ env: driver_env });
    progress(3, 4, "compiling the libinput driver");
    exec_checked("meson", ["compile", "-C", driver_build(ctx), "-j", ctx.jobs], #{ env: driver_env });
    for file in built_files(ctx) {
        if !is_file(file) { throw "the build wrote no " + file; }
    }
    progress(4, 4, "compiled");
    ctx
}

// What the image gets, with the mode each has in it. Xorg.wrap is setuid
// root as in Alpine's package; staging files keep ordinary modes and the mode
// travels in the target metadata.
fn installed_files(ctx) {
    let m = modules(ctx);
    [
        ["usr/bin/Xorg", 0o755],
        ["usr/bin/gtf", 0o755],
        ["usr/libexec/Xorg", 0o755],
        ["usr/libexec/Xorg.wrap", 0o4555],
        ["usr/lib/xorg/protocol.txt", 0o644],
        [m + "/drivers/modesetting_drv.so", 0o755],
        [m + "/input/libinput_drv.so", 0o755],
        [m + "/extensions/libglx.so", 0o755],
        [m + "/libglamoregl.so", 0o755],
        [m + "/libexa.so", 0o755],
        [m + "/libfbdevhw.so", 0o755],
        [m + "/libint10.so", 0o755],
        [m + "/libshadow.so", 0o755],
        [m + "/libshadowfb.so", 0o755],
        [m + "/libvgahw.so", 0o755],
        [m + "/libwfb.so", 0o755],
        ["usr/share/X11/xorg.conf.d/10-quirks.conf", 0o644],
        ["usr/share/X11/xorg.conf.d/40-libinput.conf", 0o644],
    ]
}

// Only drivers' compiles and a manual reader need these; the NVIDIA snippets
// configure the proprietary driver, which is glibc-only and not on AcornOS.
fn pruned() {
    ["usr/include", "usr/lib/pkgconfig", "usr/share/aclocal", "usr/share/man", "etc",
     "usr/share/X11/xorg.conf.d/10-nvidia.conf", "usr/share/X11/xorg.conf.d/10-nvidia-modules.conf"]
}

// Where the server writes the keymaps xkbcomp compiles (-Dxkb_output_dir);
// meson installs nothing there, Alpine's package ships it empty but for a note.
fn xkb_output() { "var/lib/xkb" }

fn tree_gap(ctx, root) {
    for file in installed_files(ctx) {
        if !is_file(join_path(root, file[0])) { return "/" + file[0] + " is not installed"; }
    }
    if !exists(join_path(root, "usr/bin/X")) { return "/usr/bin/X is not installed"; }
    if !is_dir(join_path(root, xkb_output())) { return "/" + xkb_output() + " is not installed"; }
    for path in pruned() {
        if exists(join_path(root, path)) { return "/" + path + " is in the tree"; }
    }
    ""
}

fn is_installed(ctx) {
    let gap = tree_gap(ctx, tree(ctx));
    if gap != "" { return needs_work(ctx, gap); }
    ctx.outputs = #{ rootfs: tree(ctx) };
    ready(ctx)
}

fn install(ctx) {
    let root = tree(ctx);
    if is_dir(root) { rm(root); }
    mkdir(root);
    let env = tools_env(ctx);
    env.DESTDIR = root;
    progress(0, 0, "installing into the tree (meson install)");
    exec_checked("meson", ["install", "-C", server_build(ctx), "--no-rebuild", "--strip"], #{ env: env });
    exec_checked("meson", ["install", "-C", driver_build(ctx), "--no-rebuild", "--strip"], #{ env: env });
    for path in pruned() {
        let full = join_path(root, path);
        if exists(full) { rm(full); }
    }
    // /usr/bin/X is the link SDDM starts; it must point at the wrapper script.
    let x = join_path(root, "usr/bin/X");
    if !exists(x) { throw "installing wrote no /usr/bin/X"; }
    let script = read_file(join_path(root, "usr/bin/Xorg"));
    if !script.contains("/usr/libexec") || !script.contains("Xorg.wrap") {
        throw "/usr/bin/Xorg is not the wrapper script that runs /usr/libexec/Xorg.wrap:\n" + script;
    }
    for file in installed_files(ctx) {
        let path = join_path(root, file[0]);
        if !is_file(path) { throw "installing wrote no /" + file[0]; }
        // meson may or may not have managed the setuid bit here; the staging
        // file never keeps it.
        chmod(path, if file[1] == 0o644 { 0o644 } else { 0o755 });
        target_mode(root, file[0], file[1]);
    }
    mkdir(join_path(root, xkb_output()));
    target_mode(root, xkb_output(), 0o755);
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
