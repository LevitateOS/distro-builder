// Rocky Linux recipe build dependencies
//
// Provides: 7zz (as 7z)
// All downloaded/built from source by recipe — no system package manager.
//
// - 7zz: static binary from 7-zip.org

let ctx = #{
    description: "Rocky Linux ISO build dependencies",
    name: "rocky-deps",
    sevenz_url: "https://7-zip.org/a/7z2501-linux-x64.tar.xz",
};

fn is_installed(ctx) {
    let bin = join_path(TOOLS_PREFIX, "bin");
    if !is_file(join_path(bin, "7zz")) { throw "7zz not installed"; }
    ctx
}

// === CLEANUP ===
fn cleanup(ctx, reason) { ctx }

fn is_acquired(ctx) {
    if !is_file(join_path(BUILD_DIR, "7z.tar.xz")) { throw "7z not downloaded"; }
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);

    let tarballs = [
        [ctx.sevenz_url, "7z.tar.xz"],
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
    mkdir(bin_dir);

    // --- 7zz: extract static binary ---
    if !is_file(join_path(bin_dir, "7zz")) {
        log("Installing 7zz...");
        let extract_dir = join_path(BUILD_DIR, "7z");
        mkdir(extract_dir);
        shell("tar xf " + join_path(BUILD_DIR, "7z.tar.xz") + " -C " + extract_dir);
        copy_into_dir(join_path(extract_dir, "7zz"), bin_dir);
        chmod(join_path(bin_dir, "7zz"), 0o755);
        ln_force(join_path(bin_dir, "7zz"), join_path(bin_dir, "7z"));
    }

    ctx
}
