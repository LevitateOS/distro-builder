const RECIPE_API = 1;
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
// in the published tree. JOBS is how many compilers run at once.
const NEEDS = #{
    outputs: #{ rootfs: "directory" },
    network: ["github.com:443"],
    memory_mib: 8192,
    storage_max_objects: 262144,
    timeout_secs: 10800,
};
const PACKAGE = #{ name: "xlibre", description: "The XLibre X server and its libinput driver for AcornOS" };
const SOURCES = #{
    xserver: #{
        git: "https://github.com/X11Libre/xserver",
        tag: "xlibre-xserver-25.1.11",
        commit: "ec1f2d37daa1450d8452abb547de4546321d7bb9",
    },
    libinput: #{
        git: "https://github.com/X11Libre/xf86-input-libinput",
        tag: "xlibre-xf86-input-libinput-25.0.1",
        commit: "4eb6691efeef4969a05b1d6b77d980943fb9760e",
    },
};
// The server's ABI directory under the module path: XLibre loads modules
// from here first, and only modules built against its SDK.
const ABI = "xlibre-25";

fn server_build() { join_path(WORK, "build-xserver") }
fn driver_build() { join_path(WORK, "build-libinput") }
// The server installed with its SDK, for the driver's compile only.
fn sdk() { join_path(WORK, "sdk") }
fn sdk_pc() { join_path(WORK, "sdk-pkgconfig") }
fn modules() { "usr/lib/xorg/modules/" + ABI }

// Alpine's own flags (abuild's defaults) plus -z lazy, which Alpine's
// xorg-server build adds: Alpine's gcc links with -z now by default, and the
// server's modules resolve symbols of each other and of the server when they
// load, which fails under -z now.
fn tools_env() {
    #{
        HOME: WORK,
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

// What the two compiles leave in their build directories.
fn built_files() {
    [
        join_path(server_build(), "hw/xfree86/Xorg"),
        join_path(server_build(), "hw/xfree86/Xorg.wrap"),
        join_path(server_build(), "hw/xfree86/drivers/video/modesetting/modesetting_drv.so"),
        join_path(driver_build(), "libinput_drv.so"),
    ]
}

// What the image gets, with the mode each has in it. Xorg.wrap is setuid
// root as in Alpine's package; staging files keep ordinary modes and the mode
// travels in the target metadata.
fn installed_files() {
    let m = modules();
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

fn build(src, out) {
    if !is_file("/usr/bin/meson") {
        throw "this root has no meson: XLibre is compiled in the variant's apps-build root, not in Recipe's usual one";
    }
    let jobs = JOBS.to_string();
    let env = tools_env();
    for dir in [server_build(), driver_build(), sdk(), sdk_pc()] {
        if is_dir(dir) { rm(dir); }
    }

    progress(0, 4, "configuring the server (meson)");
    exec_checked("meson", setup_args(src.xserver, server_build(), server_options()), #{ env: env });
    progress(1, 4, "compiling the server");
    exec_checked("meson", ["compile", "-C", server_build(), "-j", jobs], #{ env: env });

    // The driver is compiled against the server just built, through its SDK
    // in a staging tree: the copied xorg-server.pc points its prefix there.
    let stage_env = tools_env();
    stage_env.DESTDIR = sdk();
    exec_checked("meson", ["install", "-C", server_build(), "--no-rebuild"], #{ env: stage_env });
    // meson may have set the wrapper's setuid bit; no staging file keeps one.
    let staged_wrapper = join_path(sdk(), "usr/libexec/Xorg.wrap");
    if is_file(staged_wrapper) { chmod(staged_wrapper, 0o755); }
    let pc = join_path(sdk(), "usr/lib/pkgconfig/xorg-server.pc");
    if !is_file(pc) { throw "the server installed no xorg-server.pc (its SDK)"; }
    mkdir(sdk_pc());
    let copied = join_path(sdk_pc(), "xorg-server.pc");
    copy_file(pc, copied);
    replace_in_file(copied, "prefix=/usr\n", "prefix=" + join_path(sdk(), "usr") + "\n");
    let driver_env = tools_env();
    driver_env.PKG_CONFIG_PATH = sdk_pc();

    progress(2, 4, "configuring the libinput driver (meson)");
    exec_checked("meson", setup_args(src.libinput, driver_build(), [
        "-Dsdkdir=/usr/include/xorg",
        "-Dxorg-module-dir=/usr/lib/xorg/modules/" + ABI + "/input",
        "-Dxorg-conf-dir=/usr/share/X11/xorg.conf.d",
    ]), #{ env: driver_env });
    progress(3, 4, "compiling the libinput driver");
    exec_checked("meson", ["compile", "-C", driver_build(), "-j", jobs], #{ env: driver_env });
    for file in built_files() {
        if !is_file(file) { throw "the build wrote no " + file; }
    }
    progress(4, 4, "compiled");

    let install_env = tools_env();
    install_env.DESTDIR = out;
    progress(0, 0, "installing into the tree (meson install)");
    exec_checked("meson", ["install", "-C", server_build(), "--no-rebuild", "--strip"], #{ env: install_env });
    exec_checked("meson", ["install", "-C", driver_build(), "--no-rebuild", "--strip"], #{ env: install_env });
    for path in pruned() {
        let full = join_path(out, path);
        if exists(full) { rm(full); }
    }
    // /usr/bin/X is the link SDDM starts; it must point at the wrapper script.
    let x = join_path(out, "usr/bin/X");
    if !exists(x) { throw "installing wrote no /usr/bin/X"; }
    let script = read_file(join_path(out, "usr/bin/Xorg"));
    if !script.contains("/usr/libexec") || !script.contains("Xorg.wrap") {
        throw "/usr/bin/Xorg is not the wrapper script that runs /usr/libexec/Xorg.wrap:\n" + script;
    }
    for file in installed_files() {
        let path = join_path(out, file[0]);
        if !is_file(path) { throw "installing wrote no /" + file[0]; }
        // meson may or may not have managed the setuid bit here; the staging
        // file never keeps it.
        chmod(path, if file[1] == 0o644 { 0o644 } else { 0o755 });
        target_mode(out, file[0], file[1]);
    }
    mkdir(join_path(out, xkb_output()));
    target_mode(out, xkb_output(), 0o755);
}

fn verify(out) {
    for file in installed_files() {
        if !is_file(join_path(out, file[0])) { throw "/" + file[0] + " is not installed"; }
    }
    if !exists(join_path(out, "usr/bin/X")) { throw "/usr/bin/X is not installed"; }
    if !is_dir(join_path(out, xkb_output())) { throw "/" + xkb_output() + " is not installed"; }
    for path in pruned() {
        if exists(join_path(out, path)) { throw "/" + path + " is in the tree"; }
    }
}
