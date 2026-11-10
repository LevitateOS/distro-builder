// The preseed of a rootfs source: downloads every file of a base's lock into
// one output directory, which the rootfs recipe imports by that name. A lock
// recipe starts with `//! extends: locked-preseed.rhai` and sets `ctx.output`
// (the output's name) and `ctx.files` (its lock's [name, id, URL, SHA256]
// rows). A file's name is its URL's basename.
const RECIPE_API = 2;
const NEEDS = #{};

fn dir_of(ctx) { join_path(ctx.runtime.paths.output, ctx.output) }
fn file_of(ctx, row) { join_path(dir_of(ctx), basename(row[2])) }
fn with_outputs(ctx) {
    ctx.outputs = #{};
    ctx.outputs[ctx.output] = dir_of(ctx);
    ctx
}

fn is_acquired(ctx) {
    for row in ctx.files {
        let file = file_of(ctx, row);
        if !is_file(file) { return needs_work(ctx, "missing " + basename(file)); }
        verify_sha256(file, row[3]);
    }
    ready(with_outputs(ctx))
}

// Mirrors drop superseded builds, so a failed download usually means the lock
// is out of date. (A separate function keeps the nesting under Rhai's
// expression-depth limit, which is lower in a debug build.)
fn fetch(row, file) {
    try { download(row[2], file, row[3]); }
    catch (err) { throw "downloading " + row[1] + " failed (" + err + "); the lock may need refreshing"; }
}

fn acquire(ctx) {
    mkdir(dir_of(ctx));
    for row in ctx.files {
        let file = file_of(ctx, row);
        if ctx.runtime.options.force_acquire || !is_file(file) {
            fetch(row, file);
        } else {
            verify_sha256(file, row[3]);
        }
    }
    with_outputs(ctx)
}

// Nothing to build or install: the downloads are the product.
fn is_built(ctx) { is_acquired(ctx) }
fn build(ctx) { ctx }
fn is_installed(ctx) { is_acquired(ctx) }
fn install(ctx) { with_outputs(ctx) }

fn remove(ctx) {
    if is_dir(dir_of(ctx)) { rm(dir_of(ctx)); }
    ctx.outputs = #{};
    ctx
}
