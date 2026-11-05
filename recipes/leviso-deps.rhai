const RECIPE_API = 2;
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
    let work = ctx.runtime.paths.build;
    let tools = ctx.runtime.paths.tools;
    let bin = join_path(tools, "bin");
    if !is_file(join_path(bin, "mkfs.erofs")) { return needs_work(ctx, "mkfs.erofs not installed"); }
    if !is_file(join_path(bin, "xorriso")) { return needs_work(ctx, "xorriso not installed"); }
    if !is_file(join_path(bin, "mkfs.fat")) { return needs_work(ctx, "mkfs.fat not installed"); }
    if !is_file(join_path(bin, "mmd")) { return needs_work(ctx, "mmd not installed"); }
    if !is_file(join_path(bin, "mcopy")) { return needs_work(ctx, "mcopy not installed"); }
    if !is_file(join_path(bin, "ukify")) { return needs_work(ctx, "ukify not installed"); }
    if !is_file(join_path(bin, "isoinfo")) { return needs_work(ctx, "isoinfo not installed"); }
    ready(ctx)
}

// Keep acquisition caches in work for reuse.
fn is_acquired(ctx) {
    let work = ctx.runtime.paths.build;
    if !is_file(join_path(work, "m4.tar.xz")) { return needs_work(ctx, "m4 not downloaded"); }
    if !is_file(join_path(work, "autoconf.tar.xz")) { return needs_work(ctx, "autoconf not downloaded"); }
    if !is_file(join_path(work, "automake.tar.xz")) { return needs_work(ctx, "automake not downloaded"); }
    if !is_file(join_path(work, "libtool.tar.xz")) { return needs_work(ctx, "libtool not downloaded"); }
    if !is_file(join_path(work, "zstd.tar.gz")) { return needs_work(ctx, "zstd not downloaded"); }
    if !is_file(join_path(work, "lz4.tar.gz")) { return needs_work(ctx, "lz4 not downloaded"); }
    if !is_file(join_path(work, "erofs-utils.tar.gz")) { return needs_work(ctx, "erofs-utils not downloaded"); }
    if !is_file(join_path(work, "xorriso.tar.gz")) { return needs_work(ctx, "xorriso not downloaded"); }
    if !is_file(join_path(work, "mtools.tar.bz2")) { return needs_work(ctx, "mtools not downloaded"); }
    if !is_file(join_path(work, "dosfstools.tar.gz")) { return needs_work(ctx, "dosfstools not downloaded"); }
    if !is_file(join_path(work, "systemd.tar.gz")) { return needs_work(ctx, "systemd not downloaded"); }
    if !is_file(join_path(work, "cdrkit.tar.gz")) { return needs_work(ctx, "cdrkit not downloaded"); }
    ready(ctx)
}

fn acquire(ctx) {
    let work = ctx.runtime.paths.build;
    mkdir(work);

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
        let dest = join_path(work, t[1]);
        if !is_file(dest) {
            log("Downloading " + t[1] + "...");
            download(t[0], dest);
        }
    }

    ctx
}

fn install(ctx) {
    let work = ctx.runtime.paths.build;
    let tools = ctx.runtime.paths.tools;
    let bin_dir = join_path(tools, "bin");
    let lib_dir = join_path(tools, "lib");
    let inc_dir = join_path(tools, "include");
    mkdir(bin_dir);
    mkdir(lib_dir);
    mkdir(inc_dir);

    let jobs = ctx.runtime.options.jobs.to_string();

    // Helper: extract tarball if source dir doesn't exist
    // Note: each tool checks its own binary before building

    // =========================================================================
    // Phase 1: Autotools bootstrap (m4 → autoconf → automake → libtool)
    // Needed to run autoreconf for erofs-utils
    // =========================================================================

    // --- m4 ---
    if !is_file(join_path(bin_dir, "m4")) {
        log("Building m4...");
        let src = join_path(work, ctx.m4_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "m4.tar.xz") + " -C " + work);
        }
        shell_in(src, "./configure --prefix=" + tools);
        shell_in(src, "make -j" + jobs);
        shell_in(src, "make install");
    }

    // --- autoconf ---
    if !is_file(join_path(bin_dir, "autoconf")) {
        log("Building autoconf...");
        let src = join_path(work, ctx.autoconf_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "autoconf.tar.xz") + " -C " + work);
        }
        shell_in(src, "M4=" + join_path(bin_dir, "m4") + " ./configure --prefix=" + tools);
        shell_in(src, "make -j" + jobs);
        shell_in(src, "make install");
    }

    // --- automake ---
    if !is_file(join_path(bin_dir, "automake")) {
        log("Building automake...");
        let src = join_path(work, ctx.automake_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "automake.tar.xz") + " -C " + work);
        }
        shell_in(src, "PATH=" + bin_dir + ":$PATH ./configure --prefix=" + tools);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make -j" + jobs);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make install");
    }

    // --- libtool ---
    if !is_file(join_path(bin_dir, "libtool")) {
        log("Building libtool...");
        let src = join_path(work, ctx.libtool_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "libtool.tar.xz") + " -C " + work);
        }
        shell_in(src, "PATH=" + bin_dir + ":$PATH ./configure --prefix=" + tools);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make -j" + jobs);
        shell_in(src, "PATH=" + bin_dir + ":$PATH make install");
    }

    // =========================================================================
    // Phase 2: Libraries (zstd, lz4) for erofs-utils compression
    // =========================================================================

    // --- zstd (uses simple Makefile, no autotools) ---
    if !is_file(join_path(lib_dir, "libzstd.a")) {
        log("Building zstd...");
        let src = join_path(work, ctx.zstd_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "zstd.tar.gz") + " -C " + work);
        }
        shell_in(join_path(src, "lib"), "make -j" + jobs + " PREFIX=" + tools + " install-static install-includes install-pc");
    }

    // --- lz4 (uses simple Makefile, no autotools) ---
    if !is_file(join_path(lib_dir, "liblz4.a")) {
        log("Building lz4...");
        let src = join_path(work, ctx.lz4_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "lz4.tar.gz") + " -C " + work);
        }
        shell_in(src, "make -j" + jobs + " PREFIX=" + tools + " BUILD_SHARED=no lib-release");
        shell_in(src, "make PREFIX=" + tools + " BUILD_SHARED=no install");
    }

    // =========================================================================
    // Phase 3: erofs-utils (mkfs.erofs)
    // Requires: autotools (for autoreconf), zstd, lz4, pkg-config
    // =========================================================================
    if !is_file(join_path(bin_dir, "mkfs.erofs")) {
        log("Building erofs-utils (mkfs.erofs)...");
        let src = join_path(work, ctx.erofs_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "erofs-utils.tar.gz") + " -C " + work);
        }
        let path_env = "PATH=" + bin_dir + ":$PATH";
        let pkg_env = "PKG_CONFIG_PATH=" + lib_dir + "/pkgconfig";
        let env = path_env + " " + pkg_env;
        // Run autoreconf to generate configure
        shell_in(src, env + " autoreconf -fi");
        // Configure with static linking to our built libs
        shell_in(src, env + " LDFLAGS=-L" + lib_dir + " CFLAGS=-I" + inc_dir + " ./configure --prefix=" + tools + " --with-zstd --with-lz4 --without-fuse --without-selinux --disable-shared --enable-static");
        shell_in(src, env + " make -j" + jobs);
        copy_into_dir(join_path(src, "mkfs/mkfs.erofs"), bin_dir);
        chmod(join_path(bin_dir, "mkfs.erofs"), 0o755);
    }

    // =========================================================================
    // Phase 4: xorriso (standalone GNU tarball bundles libburn+libisofs)
    // =========================================================================
    if !is_file(join_path(bin_dir, "xorriso")) {
        log("Building xorriso...");
        let src = join_path(work, ctx.xorriso_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "xorriso.tar.gz") + " -C " + work);
        }
        shell_in(src, "./configure --prefix=" + tools + " --disable-shared");
        shell_in(src, "make -j" + jobs);
        copy_into_dir(join_path(src, "xorriso/xorriso"), bin_dir);
        chmod(join_path(bin_dir, "xorriso"), 0o755);
    }

    // =========================================================================
    // Phase 5: mtools (mmd, mcopy for EFI FAT image)
    // =========================================================================
    if !is_file(join_path(bin_dir, "mmd")) {
        log("Building mtools...");
        let src = join_path(work, ctx.mtools_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "mtools.tar.bz2") + " -C " + work);
        }
        shell_in(src, "./configure --prefix=" + tools);
        shell_in(src, "make -j" + jobs);
        // Copy just the tools we need
        copy_into_dir(join_path(src, "mmd"), bin_dir);
        copy_into_dir(join_path(src, "mcopy"), bin_dir);
        copy_into_dir(join_path(src, "mformat"), bin_dir);
        chmod(join_path(bin_dir, "mmd"), 0o755);
        chmod(join_path(bin_dir, "mcopy"), 0o755);
        chmod(join_path(bin_dir, "mformat"), 0o755);
    }

    // =========================================================================
    // Phase 6: dosfstools (mkfs.fat)
    // =========================================================================
    if !is_file(join_path(bin_dir, "mkfs.fat")) {
        log("Building dosfstools...");
        let src = join_path(work, ctx.dosfstools_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "dosfstools.tar.gz") + " -C " + work);
        }
        shell_in(src, "./configure --prefix=" + tools + " --enable-compat-symlinks");
        shell_in(src, "make -j" + jobs);
        copy_into_dir(join_path(src, "src/mkfs.fat"), bin_dir);
        chmod(join_path(bin_dir, "mkfs.fat"), 0o755);
    }

    // =========================================================================
    // Phase 7: ukify (Python script from systemd source + pefile)
    // =========================================================================
    if !is_file(join_path(bin_dir, "ukify")) {
        log("Installing ukify from systemd source...");
        let src = join_path(work, ctx.systemd_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "systemd.tar.gz") + " -C " + work);
        }
        // ukify is a standalone Python script
        let ukify_src = join_path(src, "src/ukify/ukify.py");
        let ukify_dest = join_path(bin_dir, "ukify");
        copy_into_dir(ukify_src, bin_dir);
        chmod(ukify_dest, 0o755);

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
        let src = join_path(work, ctx.cdrkit_dir);
        if !is_dir(src) {
            shell("tar xf " + join_path(work, "cdrkit.tar.gz") + " -C " + work);
        }
        let build_dir = join_path(src, "build");
        mkdir(build_dir);
        shell_in(build_dir, "cmake -DCMAKE_INSTALL_PREFIX=" + tools + " ..");
        shell_in(build_dir, "make -j" + jobs + " isoinfo genisoimage");
        // Copy just the tools we need
        let isoinfo = join_path(build_dir, "3rd-party/zlib/contrib/minizip/isoinfo");
        // cdrkit puts isoinfo in genisoimage/ dir
        copy_first_existing([
            join_path(build_dir, "genisoimage/isoinfo"),
            join_path(build_dir, "isoinfo/isoinfo")
        ], join_path(bin_dir, "isoinfo"));
        let genisoimage_src = join_path(build_dir, "genisoimage/genisoimage");
        if is_file(genisoimage_src) {
            copy_file(genisoimage_src, join_path(bin_dir, "genisoimage"));
        }
        shell("chmod +x " + join_path(bin_dir, "isoinfo"));
    }

    ctx
}
