// Linux kernel build dependencies
//
// Bundled dep recipe: downloads and extracts all RPMs needed to build
// the Linux kernel into TOOLS_PREFIX.

let ctx = #{
    name: "linux-deps",
    description: "Linux kernel build dependencies",
    packages: ["flex", "bison", "m4", "bc", "gcc", "cpp", "gcc-c++", "make",
               "binutils", "perl", "elfutils-libelf", "elfutils-libelf-devel",
               "openssl-libs", "openssl-devel", "zlib-ng-compat",
               "diffutils", "findutils"],
};

fn is_installed(ctx) {
    let bins = ["flex", "bison", "gcc", "make", "bc", "perl", "ld"];
    for bin in bins {
        if shell_status("which " + bin) != 0 {
            // Check all possible RPM install locations (keep expressions simple for Rhai).
            let found = false;
            for p in ["usr/bin", "usr/sbin", "bin", "sbin"] {
                let dir = join_path(TOOLS_PREFIX, p);
                let candidate = join_path(dir, bin);
                if is_file(candidate) {
                    found = true;
                    break;
                }
            }
            if !found { throw bin + " not available"; }
        }
    }
    ctx
}

// === CLEANUP ===
fn cleanup(ctx, reason) { ctx }

fn is_acquired(ctx) {
    let rpms = glob_list(join_path(BUILD_DIR, "*.rpm"));
    if rpms.len() == 0 {
        throw "no RPMs downloaded";
    }
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);
    let pkg_list = "";
    for pkg in ctx.packages {
        pkg_list += " " + pkg;
    }
    shell("dnf --USING-DNF-INSTEAD-OF-FIXING-RECIPE-MEANS-BLOODY-CUTS-ON-THE-USERS-ARM download -q --resolve --destdir=" + BUILD_DIR + " --arch x86_64 --arch noarch" + pkg_list);
    ctx
}

fn install(ctx) {
    mkdir(TOOLS_PREFIX);
    let rpms = glob_list(join_path(BUILD_DIR, "*.rpm"));
    for rpm in rpms {
        // Extract to a temp file first so rpm2cpio failures aren't swallowed by the pipe
        let cpio_file = rpm + ".cpio";
        shell("rpm2cpio '" + rpm + "' > '" + cpio_file + "'");
        shell_in(TOOLS_PREFIX, "cpio -idmu --quiet < '" + cpio_file + "'");
        shell("rm -f '" + cpio_file + "'");
    }

    // Remove RPM-extracted linker so the system ld (which knows standard lib
    // paths) is used instead. The RPM ld is gold and doesn't know about
    // .tools/usr/lib64, causing -lelf and similar failures.
    for ld_name in ["ld", "ld.bfd", "ld.gold"] {
        let p = join_path(TOOLS_PREFIX, "usr/bin/" + ld_name);
        if is_file(p) { rm(p); }
    }

    ctx
}
