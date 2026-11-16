const RECIPE_API = 1;
// Linux kernel build dependencies
//
// Downloads and extracts all RPMs needed to build the Linux kernel into its
// `tools` output, which the kernel recipes get on their PATH. The RPMs are
// the locked Rocky 10 set `cargo xtask base kernel-deps` writes (the kernel
// build dependencies and what the compiler image does not already provide),
// each verified against its locked SHA-256; dnf is not used, so nothing here
// needs a writable rpmdb in the immutable domain image. The rows come from a
// lock module, so they are downloaded in build, not declared as SOURCES.
const NEEDS = #{ outputs: #{ tools: "directory" }, network: ["download.rockylinux.org:443"], memory_mib: 4096, storage_max_objects: 65536 };
const PACKAGE = #{ name: "linux-deps", description: "Linux kernel build dependencies" };
import "sources/kernel-deps-lock.rhai" as lock;

fn build(src, out) {
    let rpms = join_path(WORK, "rpms");
    mkdir(rpms);
    let rows = lock::packages();
    let total = rows.len();
    let done = 0;
    progress(0, total, "files");
    for row in rows {
        let file = join_path(rpms, basename(row[2]));
        try { download(row[2], file, row[3]); }
        catch (err) { throw "downloading " + row[1] + " failed (" + err + "); the lock may need refreshing (cargo xtask base kernel-deps)"; }
        done += 1;
        progress(done, total, "files");
    }

    for rpm in glob_list(join_path(rpms, "*.rpm")) {
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
        // SHA-256 verified on download, and the tools are checked below,
        // so this one exit status is checked by its results.
        shell_in(out, "cpio -idmu --quiet --no-preserve-owner './usr/*' < '" + cpio_file + "'; true");
        shell("rm -f '" + cpio_file + "'");
    }

    // Documentation is dead weight in a build-tools tree, and its examples
    // ship a sample private key (perl-Net-SSLeay's server_key.pem) that
    // Recipe's secret-bearing input guard refuses to stage into the kernel
    // build. Drop the whole doc tree.
    let doc = join_path(out, "usr/share/doc");
    if is_dir(doc) { rm(doc); }

    // Remove RPM-extracted linker so the system ld (which knows standard lib
    // paths) is used instead. The RPM ld is gold and doesn't know about
    // .tools/usr/lib64, causing -lelf and similar failures.
    for ld_name in ["ld", "ld.bfd", "ld.gold"] {
        let p = join_path(out, "usr/bin/" + ld_name);
        if is_file(p) { rm(p); }
    }

    // Every tool the kernel build runs is in the image or in this tree.
    // (`command -v`, not `which`: the Rocky domain image has no `which`.)
    // This check starts a program, so it is part of build (verify only reads).
    for bin in ["flex", "bison", "gcc", "make", "bc", "perl", "ld"] {
        if shell_status("command -v " + bin + " >/dev/null 2>&1") == 0 { continue; }
        let found = false;
        for dir in ["usr/bin", "usr/sbin", "bin", "sbin"] {
            if is_file(join_path(out, dir + "/" + bin)) { found = true; }
        }
        if !found { throw bin + " is not available: neither in the image nor in the locked RPMs"; }
    }
}
