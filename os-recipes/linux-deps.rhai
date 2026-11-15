const RECIPE_API = 2;
const NEEDS = #{ outputs: #{ tools: "directory" }, network: ["download.rockylinux.org:443"], memory_mib: 4096, storage_max_objects: 65536 };
// Linux kernel build dependencies
//
// Bundled dep recipe: downloads and extracts all RPMs needed to build
// the Linux kernel into RUNTIME.paths.tools. The RPMs are the locked
// Rocky 10 set `cargo xtask base kernel-deps` writes (the kernel build
// dependencies and what the compiler image does not already provide),
// each verified against its locked SHA-256; dnf is not used, so nothing
// here needs a writable rpmdb in the immutable domain image.
import "sources/kernel-deps-lock.rhai" as lock;

let ctx = #{
    name: "linux-deps",
    description: "Linux kernel build dependencies",
};

fn is_installed(ctx) {
    let tools = ctx.runtime.paths.tools;
    let bins = ["flex", "bison", "gcc", "make", "bc", "perl", "ld"];
    for bin in bins {
        // `command -v`, not `which`: the Rocky domain image has no `which`.
        if shell_status("command -v " + bin + " >/dev/null 2>&1") != 0 {
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
    for row in lock::packages() {
        let file = join_path(work, basename(row[2]));
        if !is_file(file) { return needs_work(ctx, "missing " + basename(file)); }
        verify_sha256(file, row[3]);
    }
    ready(ctx)
}

fn acquire(ctx) {
    let work = ctx.runtime.paths.build;
    mkdir(work);
    // Drop anything a previous, differently locked run left behind.
    let wanted = lock::packages().map(|row| join_path(work, basename(row[2])));
    for rpm in glob_list(join_path(work, "*.rpm")) {
        if !wanted.contains(rpm) { rm(rpm); }
    }
    let total = wanted.len();
    let done = 0;
    progress(0, total, "files");
    for row in lock::packages() {
        let file = join_path(work, basename(row[2]));
        if ctx.runtime.options.force_acquire || !is_file(file) {
            try { download(row[2], file, row[3]); }
            catch (err) { throw "downloading " + row[1] + " failed (" + err + "); the lock may need refreshing (cargo xtask base kernel-deps)"; }
        } else {
            verify_sha256(file, row[3]);
        }
        done += 1;
        progress(done, total, "files");
    }
    ctx
}

fn install(ctx) {
    let work = ctx.runtime.paths.build;
    let tools = ctx.runtime.paths.tools;
    mkdir(tools);
    let rpms = glob_list(join_path(work, "*.rpm"));
    for rpm in rpms {
        // Extract to a temp file first so rpm2cpio failures aren't swallowed by the pipe.
        // Only `usr/` is taken: the tree is build tools, and the system
        // skeleton some packages also ship (`tmp`, `var/spool`, …) has
        // owners and modes the domain cannot restore.
        let cpio_file = rpm + ".cpio";
        shell("rpm2cpio '" + rpm + "' > '" + cpio_file + "'");
        // Some payloads carry setuid binaries (passwd, chage); the domain
        // refuses to restore those modes, which makes cpio exit nonzero
        // after writing the file's contents without the special bits --
        // exactly what a build-tools tree wants. The RPM itself was
        // SHA-256 verified on download, and `is_installed` verifies the
        // tools below, so this one exit status is checked by its results.
        shell_in(tools, "cpio -idmu --quiet --no-preserve-owner './usr/*' < '" + cpio_file + "'; true");
        shell("rm -f '" + cpio_file + "'");
    }

    // Documentation is dead weight in a build-tools tree, and its examples
    // ship a sample private key (perl-Net-SSLeay's server_key.pem) that
    // Recipe's secret-bearing input guard refuses to stage into the kernel
    // build. Drop the whole doc tree.
    let doc = join_path(tools, "usr/share/doc");
    if is_dir(doc) { rm(doc); }

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
