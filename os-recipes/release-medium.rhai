// What the release medium's overlay carries so the installer installs the
// default software set offline (recsetup's software step), and what never
// reaches an installed system (recstrap extracts the system image, not the
// overlay):
//
// - the package cache at CACHE_DIR (`sha256/<hex>`, one file per payload,
//   named by its SHA-256), where Recipe looks before the network: every file
//   the default set's group recipes and their deps pin for this system's C
//   library (the builder lists them in the `pins` input, `<sha256> <URL>` per
//   line);
// - those group recipes (the `recipes` input) under DEFAULT_SET_DIR/recipes,
//   which recsetup hands Recipe as its recipes folder;
// - the default set itself (the `list` input, one recipe name per line) at
//   DEFAULT_SET_DIR/list.
//
// CACHE_DIR and DEFAULT_SET_DIR are defines the builder passes from the
// install tools' constants. Each pinned file comes from the builder's download
// store (the optional `store` input: files named by their SHA-256, as the
// desktop lock's preseed left them), else from its URL, and is checked
// against its SHA-256 either way.
const RECIPE_API = 2;
const NEEDS = #{
    inputs: #{ pins: "file", recipes: "directory", list: "file", store: "directory?" },
    outputs: #{ medium: "directory" },
    network: ["dl.fedoraproject.org:443", "dl-cdn.alpinelinux.org:443", "github.com:443",
              "release-assets.githubusercontent.com:443", "objects.githubusercontent.com:443"],
    memory_mib: 8192,
    storage_max_objects: 262144,
    timeout_secs: 14400,
};

let ctx = #{
    name: "release-medium",
    description: "The default software set, its recipes and its package cache, for the release medium",
    medium: join_path(RUNTIME.paths.output, "medium"),
    downloads: join_path(RUNTIME.paths.build, "downloads"),
    cache_dir: CACHE_DIR,
    set_dir: DEFAULT_SET_DIR,
};

fn input(ctx, name) { join_path(ctx.runtime.paths.inputs, name) }

// The `pins` input as [sha256, URL] pairs.
fn pins(ctx) {
    let rows = [];
    for line in read_file(input(ctx, "pins")).split("\n") {
        let line = trim(line);
        if line == "" { continue; }
        let parts = line.split(" ");
        if parts.len() != 2 || parts[0].len() != 64 { throw "malformed pins line: " + line; }
        rows.push([parts[0].to_lower(), parts[1]]);
    }
    rows
}

fn download_of(ctx, pin) { join_path(ctx.downloads, pin[0]) }

fn is_acquired(ctx) {
    for pin in pins(ctx) {
        let file = download_of(ctx, pin);
        if !is_file(file) { return needs_work(ctx, "fetch " + pin[1]); }
        verify_sha256(file, pin[0]);
    }
    ready(ctx)
}

fn acquire(ctx) {
    mkdir(ctx.downloads);
    let rows = pins(ctx);
    let done = 0;
    progress(0, rows.len(), "files");
    for pin in rows {
        let file = download_of(ctx, pin);
        if !is_file(file) {
            let stored = join_path(input(ctx, "store"), pin[0]);
            if is_file(stored) { copy_file(stored, file); verify_sha256(file, pin[0]); }
            else { download(pin[1], file, pin[0]); }
        } else {
            verify_sha256(file, pin[0]);
        }
        done += 1;
        progress(done, rows.len(), "files");
    }
    ctx
}

// The medium tree: the cache, the recipes and the list, all read-only files.
fn put_file(ctx, from, path) {
    let dest = join_path(ctx.medium, path);
    mkdir(dirname(dest));
    copy_file(from, dest);
    target_mode(ctx.medium, path, 0o644);
}

fn is_built(ctx) {
    if !is_file(join_path(ctx.medium, ctx.set_dir.sub_string(1) + "/list")) { return needs_work(ctx, "assemble the medium tree"); }
    ready(ctx)
}

fn build(ctx) {
    if exists(ctx.medium) { rm(ctx.medium); }
    mkdir(ctx.medium);
    let cache = ctx.cache_dir.sub_string(1) + "/sha256/";
    for pin in pins(ctx) {
        verify_sha256(download_of(ctx, pin), pin[0]);
        put_file(ctx, download_of(ctx, pin), cache + pin[0]);
    }
    let set = ctx.set_dir.sub_string(1);
    let names = [];
    for line in read_file(input(ctx, "list")).split("\n") {
        if trim(line) != "" { names.push(trim(line)); }
    }
    if names.is_empty() { throw "the default set lists no recipe"; }
    for name in shell_output("ls '" + input(ctx, "recipes") + "'").split("\n") {
        let name = trim(name);
        if name == "" { continue; }
        if !name.ends_with(".rhai") { throw "the recipes input holds " + name + ", not a recipe"; }
        put_file(ctx, join_path(input(ctx, "recipes"), name), set + "/recipes/" + name);
    }
    for name in names {
        if !is_file(join_path(ctx.medium, set + "/recipes/" + name + ".rhai")) {
            throw "the default set names " + name + ", which the recipes input lacks";
        }
    }
    put_file(ctx, input(ctx, "list"), set + "/list");
    target_finalize(ctx.medium);
    ctx
}

fn is_installed(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { return status; }
    ctx = status.ctx;
    ctx.outputs = #{ medium: ctx.medium };
    ready(ctx)
}

fn install(ctx) {
    ctx.outputs = #{ medium: ctx.medium };
    ctx
}

fn remove(ctx) {
    for dir in [ctx.medium, ctx.downloads] { if exists(dir) { rm(dir); } }
    ctx.outputs = #{};
    ctx
}
