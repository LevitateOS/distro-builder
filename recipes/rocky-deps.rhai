// Rocky Linux recipe build dependencies
//
// Provides: 7zz (as 7z), unsquashfs
// All downloaded/built from source by recipe — no system package manager.
//
// - 7zz: static binary from 7-zip.org
// - unsquashfs: built from source with xz/zlib built from source

let ctx = #{
    description: "Rocky Linux ISO build dependencies",
    name: "rocky-deps",
    sevenz_url: "https://7-zip.org/a/7z2501-linux-x64.tar.xz",
    sqfs_url: "https://github.com/plougher/squashfs-tools/archive/refs/tags/4.6.1.tar.gz",
    sqfs_version: "4.6.1",
    xz_url: "https://github.com/tukaani-project/xz/releases/download/v5.6.2/xz-5.6.2.tar.xz",
    zlib_url: "https://github.com/madler/zlib/releases/download/v1.3.1/zlib-1.3.1.tar.gz",
};

fn is_installed(ctx) {
    let bin = join_path(TOOLS_PREFIX, "bin");
    if !is_file(join_path(bin, "7zz")) { throw "7zz not installed"; }
    if !is_file(join_path(bin, "unsquashfs")) { throw "unsquashfs not installed"; }
    ctx
}

// === CLEANUP ===
fn cleanup(ctx, reason) { ctx }

fn is_acquired(ctx) {
    if !is_file(join_path(BUILD_DIR, "7z.tar.xz")) { throw "7z not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "squashfs-tools.tar.gz")) { throw "squashfs-tools not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "xz.tar.xz")) { throw "xz not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "zlib.tar.gz")) { throw "zlib not downloaded"; }
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);

    let tarballs = [
        [ctx.sevenz_url, "7z.tar.xz"],
        [ctx.sqfs_url, "squashfs-tools.tar.gz"],
        [ctx.xz_url, "xz.tar.xz"],
        [ctx.zlib_url, "zlib.tar.gz"],
    ];

    for t in tarballs {
        let dest = join_path(BUILD_DIR, t[1]);
        if !is_file(dest) {
            log("Downloading " + t[1] + "...");
            download(t[0], dest);
        }
    }

    ctx
}

fn install(ctx) {
    let bin_dir = join_path(TOOLS_PREFIX, "bin");
    let lib_dir = join_path(TOOLS_PREFIX, "lib");
    let inc_dir = join_path(TOOLS_PREFIX, "include");
    mkdir(bin_dir);
    mkdir(lib_dir);
    mkdir(inc_dir);

    let jobs = trim(shell_output("nproc"));

    // --- 7zz: extract static binary ---
    if !is_file(join_path(bin_dir, "7zz")) {
        log("Installing 7zz...");
        let extract_dir = join_path(BUILD_DIR, "7z");
        mkdir(extract_dir);
        shell("tar xf " + join_path(BUILD_DIR, "7z.tar.xz") + " -C " + extract_dir);
        shell("cp " + join_path(extract_dir, "7zz") + " " + join_path(bin_dir, "7zz"));
        shell("chmod +x " + join_path(bin_dir, "7zz"));
        ln(join_path(bin_dir, "7zz"), join_path(bin_dir, "7z"));
    }

    // --- zlib: build from source (needed by unsquashfs) ---
    if !is_file(join_path(lib_dir, "libz.a")) {
        log("Building zlib from source...");
        let src_dir = join_path(BUILD_DIR, "zlib-1.3.1");
        if !is_dir(src_dir) {
            shell("tar xf " + join_path(BUILD_DIR, "zlib.tar.gz") + " -C " + BUILD_DIR);
        }
        shell_in(src_dir, "./configure --prefix=" + TOOLS_PREFIX + " --static");
        shell_in(src_dir, "make -j" + jobs);
        shell_in(src_dir, "make install");
    }

    // --- xz/liblzma: build from source (needed by unsquashfs) ---
    if !is_file(join_path(lib_dir, "liblzma.a")) {
        log("Building xz/liblzma from source...");
        let src_dir = join_path(BUILD_DIR, "xz-5.6.2");
        if !is_dir(src_dir) {
            shell("tar xf " + join_path(BUILD_DIR, "xz.tar.xz") + " -C " + BUILD_DIR);
        }
        shell_in(src_dir, "./configure --prefix=" + TOOLS_PREFIX + " --enable-static --disable-shared --disable-doc");
        shell_in(src_dir, "make -j" + jobs);
        shell_in(src_dir, "make install");
    }

    // --- unsquashfs: build from source ---
    if !is_file(join_path(bin_dir, "unsquashfs")) {
        log("Building unsquashfs from source...");
        let src_dir = join_path(BUILD_DIR, "squashfs-tools-" + ctx.sqfs_version);
        if !is_dir(src_dir) {
            shell("tar xf " + join_path(BUILD_DIR, "squashfs-tools.tar.gz") + " -C " + BUILD_DIR);
        }
        let build_dir = join_path(src_dir, "squashfs-tools");
        shell_in(build_dir, "make -j" + jobs + " INCLUDEDIR=-I" + inc_dir + " EXTRA_CFLAGS=-D_GNU_SOURCE LDFLAGS=-L" + lib_dir + " XZ_SUPPORT=1 GZIP_SUPPORT=1 COMP_DEFAULT=xz LZO_SUPPORT=0 LZ4_SUPPORT=0 ZSTD_SUPPORT=0 XATTR_SUPPORT=0 unsquashfs");
        shell("cp " + join_path(build_dir, "unsquashfs") + " " + join_path(bin_dir, "unsquashfs"));
        shell("chmod +x " + join_path(bin_dir, "unsquashfs"));
    }

    ctx
}
