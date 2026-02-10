// LevitateOS ISO build host dependencies
//
// Provides: mkfs.erofs, xorriso, mkfs.fat, mmd, mcopy, ukify, isoinfo
// All downloaded/built from source by recipe — no system package manager.
//
// Build chain:
// 1. Autotools bootstrap (m4, autoconf, automake, libtool) — needed for erofs-utils
// 2. Libraries (zstd, lz4) — needed for erofs-utils compression
// 3. mkfs.erofs (erofs-utils) — EROFS filesystem creation
// 4. xorriso (standalone GNU tarball) — ISO creation
// 5. mtools (mmd, mcopy) — EFI FAT image manipulation
// 6. dosfstools (mkfs.fat) — FAT filesystem creation
// 7. ukify (systemd python script) — UKI creation
// 8. genisoimage (cdrkit) — ISO verification (isoinfo)
//
// Assumes host has: gcc, make, perl, python3, pkg-config

let ctx = #{
    name: "leviso-deps",
    description: "LevitateOS ISO build host dependencies",

    // Autotools bootstrap
    m4_url: "https://ftp.gnu.org/gnu/m4/m4-1.4.19.tar.xz",
    m4_dir: "m4-1.4.19",
    autoconf_url: "https://ftp.gnu.org/gnu/autoconf/autoconf-2.72.tar.xz",
    autoconf_dir: "autoconf-2.72",
    automake_url: "https://ftp.gnu.org/gnu/automake/automake-1.17.tar.xz",
    automake_dir: "automake-1.17",
    libtool_url: "https://ftp.gnu.org/gnu/libtool/libtool-2.5.4.tar.xz",
    libtool_dir: "libtool-2.5.4",

    // Libraries for erofs-utils
    zstd_url: "https://github.com/facebook/zstd/releases/download/v1.5.6/zstd-1.5.6.tar.gz",
    zstd_dir: "zstd-1.5.6",
    lz4_url: "https://github.com/lz4/lz4/releases/download/v1.10.0/lz4-1.10.0.tar.gz",
    lz4_dir: "lz4-1.10.0",

    // Tools
    erofs_url: "https://github.com/erofs/erofs-utils/archive/refs/tags/v1.8.6.tar.gz",
    erofs_dir: "erofs-utils-1.8.6",
    xorriso_url: "https://ftp.gnu.org/gnu/xorriso/xorriso-1.5.6.pl02.tar.gz",
    xorriso_dir: "xorriso-1.5.6",
    mtools_url: "https://ftp.gnu.org/gnu/mtools/mtools-4.0.48.tar.bz2",
    mtools_dir: "mtools-4.0.48",
    dosfstools_url: "https://github.com/dosfstools/dosfstools/releases/download/v4.2/dosfstools-4.2.tar.gz",
    dosfstools_dir: "dosfstools-4.2",
    // systemd source for ukify
    systemd_url: "https://github.com/systemd/systemd/archive/refs/tags/v257.tar.gz",
    systemd_dir: "systemd-257",
    // cdrkit for isoinfo
    cdrkit_url: "https://deb.debian.org/debian/pool/main/c/cdrkit/cdrkit_1.1.11.orig.tar.gz",
    cdrkit_dir: "cdrkit-1.1.11",
};

fn is_installed(ctx) {
    let bin = join_path(TOOLS_PREFIX, "bin");
    if !is_file(join_path(bin, "mkfs.erofs")) { throw "mkfs.erofs not installed"; }
    if !is_file(join_path(bin, "xorriso")) { throw "xorriso not installed"; }
    if !is_file(join_path(bin, "mkfs.fat")) { throw "mkfs.fat not installed"; }
    if !is_file(join_path(bin, "mmd")) { throw "mmd not installed"; }
    if !is_file(join_path(bin, "mcopy")) { throw "mcopy not installed"; }
    if !is_file(join_path(bin, "ukify")) { throw "ukify not installed"; }
    if !is_file(join_path(bin, "isoinfo")) { throw "isoinfo not installed"; }
    ctx
}

fn is_acquired(ctx) {
    if !is_file(join_path(BUILD_DIR, "m4.tar.xz")) { throw "m4 not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "autoconf.tar.xz")) { throw "autoconf not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "automake.tar.xz")) { throw "automake not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "libtool.tar.xz")) { throw "libtool not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "zstd.tar.gz")) { throw "zstd not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "lz4.tar.gz")) { throw "lz4 not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "erofs-utils.tar.gz")) { throw "erofs-utils not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "xorriso.tar.gz")) { throw "xorriso not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "mtools.tar.bz2")) { throw "mtools not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "dosfstools.tar.gz")) { throw "dosfstools not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "systemd.tar.gz")) { throw "systemd not downloaded"; }
    if !is_file(join_path(BUILD_DIR, "cdrkit.tar.gz")) { throw "cdrkit not downloaded"; }
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);

    let tarballs = [
        [ctx.m4_url, "m4.tar.xz"],
        [ctx.autoconf_url, "autoconf.tar.xz"],
        [ctx.automake_url, "automake.tar.xz"],
        [ctx.libtool_url, "libtool.tar.xz"],
        [ctx.zstd_url, "zstd.tar.gz"],
        [ctx.lz4_url, "lz4.tar.gz"],
        [ctx.erofs_url, "erofs-utils.tar.gz"],
        [ctx.xorriso_url, "xorriso.tar.gz"],
        [ctx.mtools_url, "mtools.tar.bz2"],
        [ctx.dosfstools_url, "dosfstools.tar.gz"],
        [ctx.systemd_url, "systemd.tar.gz"],
        [ctx.cdrkit_url, "cdrkit.tar.gz"],
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

    // Helper: extract tarball if source dir doesn't exist
    // Note: each tool checks its own binary before building

    // =========================================================================
    // Phase 1: Autotools bootstrap (m4 → autoconf → automake → libtool)
    // Needed to run autoreconf for erofs-utils
    // =========================================================================

    // --- m4 ---
    if !is_file(join_path(bin_dir, "m4")) {
        log("Building m4...");
        let src = join_path(BUILD_DIR, ctx.m4_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "m4.tar.xz") + " -C " + BUILD_DIR);
        }
        shell_in(src, "./configure --prefix=" + TOOLS_PREFIX);
        shell_in(src, "make -j" + jobs);
        shell_in(src, "make install");
    }

    // --- autoconf ---
    if !is_file(join_path(bin_dir, "autoconf")) {
        log("Building autoconf...");
        let src = join_path(BUILD_DIR, ctx.autoconf_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "autoconf.tar.xz") + " -C " + BUILD_DIR);
        }
        shell_in(src, "M4=" + join_path(bin_dir, "m4") + " ./configure --prefix=" + TOOLS_PREFIX);
        shell_in(src, "make -j" + jobs);
        shell_in(src, "make install");
    }

    // --- automake ---
    if !is_file(join_path(bin_dir, "automake")) {
        log("Building automake...");
        let src = join_path(BUILD_DIR, ctx.automake_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "automake.tar.xz") + " -C " + BUILD_DIR);
        }
        shell_in(src, "PATH=" + bin_dir + ":$PATH ./configure --prefix=" + TOOLS_PREFIX);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make -j" + jobs);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make install");
    }

    // --- libtool ---
    if !is_file(join_path(bin_dir, "libtool")) {
        log("Building libtool...");
        let src = join_path(BUILD_DIR, ctx.libtool_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "libtool.tar.xz") + " -C " + BUILD_DIR);
        }
        shell_in(src, "PATH=" + bin_dir + ":$PATH ./configure --prefix=" + TOOLS_PREFIX);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make -j" + jobs);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make install");
    }

    // =========================================================================
    // Phase 2: Libraries (zstd, lz4) for erofs-utils compression
    // =========================================================================

    // --- zstd (uses simple Makefile, no autotools) ---
    if !is_file(join_path(lib_dir, "libzstd.a")) {
        log("Building zstd...");
        let src = join_path(BUILD_DIR, ctx.zstd_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "zstd.tar.gz") + " -C " + BUILD_DIR);
        }
        shell_in(join_path(src, "lib"), "make -j" + jobs + " PREFIX=" + TOOLS_PREFIX + " install-static install-includes install-pc");
    }

    // --- lz4 (uses simple Makefile, no autotools) ---
    if !is_file(join_path(lib_dir, "liblz4.a")) {
        log("Building lz4...");
        let src = join_path(BUILD_DIR, ctx.lz4_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "lz4.tar.gz") + " -C " + BUILD_DIR);
        }
        shell_in(src, "make -j" + jobs + " PREFIX=" + TOOLS_PREFIX + " BUILD_SHARED=no lib-release");
        shell_in(src, "make PREFIX=" + TOOLS_PREFIX + " BUILD_SHARED=no install");
    }

    // =========================================================================
    // Phase 3: erofs-utils (mkfs.erofs)
    // Requires: autotools (for autoreconf), zstd, lz4, pkg-config
    // =========================================================================
    if !is_file(join_path(bin_dir, "mkfs.erofs")) {
        log("Building erofs-utils (mkfs.erofs)...");
        let src = join_path(BUILD_DIR, ctx.erofs_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "erofs-utils.tar.gz") + " -C " + BUILD_DIR);
        }
        let path_env = "PATH=" + bin_dir + ":$PATH";
        let pkg_env = "PKG_CONFIG_PATH=" + lib_dir + "/pkgconfig";
        let env = path_env + " " + pkg_env;
        // Run autoreconf to generate configure
        shell_in(src, env + " autoreconf -fi");
        // Configure with static linking to our built libs
        shell_in(src, env + " LDFLAGS=-L" + lib_dir + " CFLAGS=-I" + inc_dir + " ./configure --prefix=" + TOOLS_PREFIX + " --with-zstd --with-lz4 --without-fuse --without-selinux --disable-shared --enable-static");
        shell_in(src, env + " make -j" + jobs);
        shell("cp " + join_path(src, "mkfs/mkfs.erofs") + " " + join_path(bin_dir, "mkfs.erofs"));
        shell("chmod +x " + join_path(bin_dir, "mkfs.erofs"));
    }

    // =========================================================================
    // Phase 4: xorriso (standalone GNU tarball bundles libburn+libisofs)
    // =========================================================================
    if !is_file(join_path(bin_dir, "xorriso")) {
        log("Building xorriso...");
        let src = join_path(BUILD_DIR, ctx.xorriso_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "xorriso.tar.gz") + " -C " + BUILD_DIR);
        }
        shell_in(src, "./configure --prefix=" + TOOLS_PREFIX + " --disable-shared");
        shell_in(src, "make -j" + jobs);
        shell("cp " + join_path(src, "xorriso/xorriso") + " " + join_path(bin_dir, "xorriso"));
        shell("chmod +x " + join_path(bin_dir, "xorriso"));
    }

    // =========================================================================
    // Phase 5: mtools (mmd, mcopy for EFI FAT image)
    // =========================================================================
    if !is_file(join_path(bin_dir, "mmd")) {
        log("Building mtools...");
        let src = join_path(BUILD_DIR, ctx.mtools_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "mtools.tar.bz2") + " -C " + BUILD_DIR);
        }
        shell_in(src, "./configure --prefix=" + TOOLS_PREFIX);
        shell_in(src, "make -j" + jobs);
        // Copy just the tools we need
        shell("cp " + join_path(src, "mmd") + " " + join_path(bin_dir, "mmd"));
        shell("cp " + join_path(src, "mcopy") + " " + join_path(bin_dir, "mcopy"));
        shell("cp " + join_path(src, "mformat") + " " + join_path(bin_dir, "mformat"));
        shell("chmod +x " + join_path(bin_dir, "mmd") + " " + join_path(bin_dir, "mcopy") + " " + join_path(bin_dir, "mformat"));
    }

    // =========================================================================
    // Phase 6: dosfstools (mkfs.fat)
    // =========================================================================
    if !is_file(join_path(bin_dir, "mkfs.fat")) {
        log("Building dosfstools...");
        let src = join_path(BUILD_DIR, ctx.dosfstools_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "dosfstools.tar.gz") + " -C " + BUILD_DIR);
        }
        shell_in(src, "./configure --prefix=" + TOOLS_PREFIX + " --enable-compat-symlinks");
        shell_in(src, "make -j" + jobs);
        shell("cp " + join_path(src, "src/mkfs.fat") + " " + join_path(bin_dir, "mkfs.fat"));
        shell("chmod +x " + join_path(bin_dir, "mkfs.fat"));
    }

    // =========================================================================
    // Phase 7: ukify (Python script from systemd source + pefile)
    // =========================================================================
    if !is_file(join_path(bin_dir, "ukify")) {
        log("Installing ukify from systemd source...");
        let src = join_path(BUILD_DIR, ctx.systemd_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "systemd.tar.gz") + " -C " + BUILD_DIR);
        }
        // ukify is a standalone Python script
        let ukify_src = join_path(src, "src/ukify/ukify.py");
        let ukify_dest = join_path(bin_dir, "ukify");
        shell("cp " + ukify_src + " " + ukify_dest);
        shell("chmod +x " + ukify_dest);

        // Install pefile if not already available
        if shell_status("python3 -c 'import pefile'") != 0 {
            log("Installing pefile Python module...");
            shell("python3 -m pip install --user pefile");
        }
    }

    // =========================================================================
    // Phase 8: cdrkit (isoinfo for ISO verification)
    // =========================================================================
    if !is_file(join_path(bin_dir, "isoinfo")) {
        log("Building cdrkit (isoinfo)...");
        let src = join_path(BUILD_DIR, ctx.cdrkit_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(BUILD_DIR, "cdrkit.tar.gz") + " -C " + BUILD_DIR);
        }
        let build_dir = join_path(src, "build");
        mkdir(build_dir);
        shell_in(build_dir, "cmake -DCMAKE_INSTALL_PREFIX=" + TOOLS_PREFIX + " ..");
        shell_in(build_dir, "make -j" + jobs + " isoinfo genisoimage");
        // Copy just the tools we need
        let isoinfo = join_path(build_dir, "3rd-party/zlib/contrib/minizip/isoinfo");
        // cdrkit puts isoinfo in genisoimage/ dir
        shell("cp " + join_path(build_dir, "genisoimage/isoinfo") + " " + join_path(bin_dir, "isoinfo") + " 2>/dev/null || cp " + join_path(build_dir, "isoinfo/isoinfo") + " " + join_path(bin_dir, "isoinfo"));
        shell("cp " + join_path(build_dir, "genisoimage/genisoimage") + " " + join_path(bin_dir, "genisoimage") + " 2>/dev/null || true");
        shell("chmod +x " + join_path(bin_dir, "isoinfo"));
    }

    ctx
}
