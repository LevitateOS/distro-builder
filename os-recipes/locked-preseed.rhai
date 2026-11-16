const RECIPE_API = 1;
// The preseed of a rootfs source: downloads every file of a base's lock into
// the output directory, which the rootfs recipe imports by that output's
// name. A lock recipe starts with `//! extends: locked-preseed.rhai`, declares
// its NEEDS with its output, imports its lock and defines rows(): the lock's
// [name, id, URL, SHA256] rows. A file's name is its URL's basename.
//
// The rows come from a lock module, so they are not SOURCES: the files are
// downloaded here, in build (docs/recipe-format.md, section 3.1).
const NEEDS = #{};

// The builder's content-addressed store, as the optional `store` input: files
// named by their lowercase SHA-256, holding only what this lock needs. A copy
// from it is verified like a download; a missing file means download.
fn stored(row) {
    if INPUTS.store == () { return ""; }
    let file = join_path(INPUTS.store, row[3].to_lower());
    if is_file(file) { file } else { "" }
}

// Mirrors drop superseded builds, so a failed download usually means the lock
// is out of date. (A separate function keeps the nesting under Rhai's
// expression-depth limit, which is lower in a debug build.)
fn fetch(row, file) {
    try { download(row[2], file, row[3]); }
    catch (err) { throw "downloading " + row[1] + " failed (" + err + "); the lock may need refreshing"; }
}

// Take a row's file from the store when it holds it, otherwise download it.
fn obtain(row, file) {
    let kept = stored(row);
    if kept == "" { return fetch(row, file); }
    copy_file(kept, file);
    verify_sha256(file, row[3]);
}

fn build(src, out) {
    let rows = rows();
    let total = rows.len();
    let done = 0;
    progress(0, total, "files");
    for row in rows {
        obtain(row, join_path(out, basename(row[2])));
        done += 1;
        progress(done, total, "files");
    }
}
