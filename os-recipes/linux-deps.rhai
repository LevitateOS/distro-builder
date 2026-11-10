const RECIPE_API = 2;
const NEEDS = #{ outputs: #{ tools: "directory" } };
// Linux kernel build dependencies
//
// Bundled dep recipe: downloads and extracts all RPMs needed to build
// the Linux kernel into RUNTIME.paths.tools.

let ctx = #{
    name: "linux-deps",
    description: "Linux kernel build dependencies",
    packages: ["flex", "bison", "m4", "bc", "gcc", "cpp", "gcc-c++", "make",
               "binutils", "perl", "elfutils-libelf", "elfutils-libelf-devel",
               "openssl-libs", "openssl-devel", "zlib-ng-compat",
               "diffutils", "findutils"],
};

fn is_installed(ctx) {
    let tools = ctx.runtime.paths.tools;
    let bins = ["flex", "bison", "gcc", "make", "bc", "perl", "ld"];
    for bin in bins {
        if shell_status("which " + bin) != 0 {
            // Check all possible RPM install locations (keep expressions simple for Rhai).
            let found = false;
            for p in ["usr/bin", "usr/sbin", "bin", "sbin"] {
                let dir = join_path(tools, p);
                let candidate = join_path(dir, bin);
                if is_file(candidate) {
                    found = true;
                    break;
                }
            }
            if !found { return needs_work(ctx, bin + " not available"); }
        }
    }
    mkdir(tools);
    ctx.outputs = #{ tools: tools };
    ready(ctx)
}

fn is_acquired(ctx) {
    let work = ctx.runtime.paths.build;
    let rpms = glob_list(join_path(work, "*.rpm"));
    if rpms.len() == 0 {
        return needs_work(ctx, "no RPMs downloaded");
    }
    ready(ctx)
}

fn acquire(ctx) {
    let work = ctx.runtime.paths.build;
    mkdir(work);
    dnf_download(ctx.packages, work, ["x86_64", "noarch"]);
    ctx
}

fn install(ctx) {
    let work = ctx.runtime.paths.build;
    let tools = ctx.runtime.paths.tools;
    mkdir(tools);
    let rpms = glob_list(join_path(work, "*.rpm"));
    for rpm in rpms {
        // Extract to a temp file first so rpm2cpio failures aren't swallowed by the pipe
        let cpio_file = rpm + ".cpio";
        shell("rpm2cpio '" + rpm + "' > '" + cpio_file + "'");
        shell_in(tools, "cpio -idmu --quiet < '" + cpio_file + "'");
        shell("rm -f '" + cpio_file + "'");
    }

    // Remove RPM-extracted linker so the system ld (which knows standard lib
    // paths) is used instead. The RPM ld is gold and doesn't know about
    // .tools/usr/lib64, causing -lelf and similar failures.
    for ld_name in ["ld", "ld.bfd", "ld.gold"] {
        let p = join_path(tools, "usr/bin/" + ld_name);
        if is_file(p) { rm(p); }
    }

    ctx.outputs = #{ tools: tools };
    ctx
}
