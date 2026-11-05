const RECIPE_API = 2;
// Canonical RPM/DVD source recipe build dependencies
//
// Provides: 7zz (as 7z)
// All downloaded/built from source by recipe — no system package manager.
//
// - 7zz: static binary from 7-zip.org

let ctx = #{
    description: "Canonical RPM/DVD source build dependencies",
    name: "dvd-source-deps",
    sevenz_url: "https://7-zip.org/a/7z2501-linux-x64.tar.xz",
};

fn is_installed(ctx) {
    let tools = ctx.runtime.paths.tools;
    let bin = join_path(tools, "bin");
    if !is_file(join_path(bin, "7zz")) { return needs_work(ctx, "7zz not installed"); }
    ready(ctx)
}

fn is_acquired(ctx) {
    let work = ctx.runtime.paths.build;
    if !is_file(join_path(work, "7z.tar.xz")) { return needs_work(ctx, "7z not downloaded"); }
    ready(ctx)
}

fn acquire(ctx) {
    let work = ctx.runtime.paths.build;
    mkdir(work);

    let tarballs = [
        [ctx.sevenz_url, "7z.tar.xz"],
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
    mkdir(bin_dir);

    // Extract the static 7zz binary and expose the traditional 7z name too.
    if !is_file(join_path(bin_dir, "7zz")) {
        log("Installing 7zz...");
        let extract_dir = join_path(work, "7z");
        mkdir(extract_dir);
        shell("tar xf " + join_path(work, "7z.tar.xz") + " -C " + extract_dir);
        copy_into_dir(join_path(extract_dir, "7zz"), bin_dir);
        chmod(join_path(bin_dir, "7zz"), 0o755);
        ln_force(join_path(bin_dir, "7zz"), join_path(bin_dir, "7z"));
    }

    ctx
}
