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
    // Publisher download metadata: sourceforge.net/projects/sevenzip/files/7-Zip/25.01/7z2501-linux-x64.tar.xz/download
    sevenz_sha256: "4ca3b7c6f2f67866b92622818b58233dc70367be2f36b498eb0bdeaaa44b53f4",
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
    verify_sha256(join_path(work, "7z.tar.xz"), ctx.sevenz_sha256);
    ready(ctx)
}

fn acquire(ctx) {
    let work = ctx.runtime.paths.build;
    mkdir(work);

    let dest = join_path(work, "7z.tar.xz");
    if !is_file(dest) || ctx.runtime.options.force_acquire {
        download(ctx.sevenz_url, dest, ctx.sevenz_sha256);
    } else { verify_sha256(dest, ctx.sevenz_sha256); }

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
        exec_checked("tar", ["xf", join_path(work, "7z.tar.xz"), "-C", extract_dir]);
        copy_into_dir(join_path(extract_dir, "7zz"), bin_dir);
        chmod(join_path(bin_dir, "7zz"), 0o755);
        ln_force(join_path(bin_dir, "7zz"), join_path(bin_dir, "7z"));
    }

    ctx
}
