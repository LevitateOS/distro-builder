const RECIPE_API = 1;
// What the release medium needs for the default software set, in two outputs:
//
// - `medium`, the tree the builder puts on the medium's overlay (nothing of it
//   reaches an installed system: recstrap extracts the system image, not the
//   overlay): the default set's group recipes (the `recipes` input) under
//   DEFAULT_SET_DIR/recipes, which recsetup hands Recipe as its recipes
//   folder, and the default set itself (the `list` input, one recipe name per
//   line) at DEFAULT_SET_DIR/list;
// - `cache`, the package cache as files `sha256/<hex>`, one per payload, named
//   by its SHA-256: every file the default set's group recipes and their deps
//   pin for this system's C library (the builder lists them in the `pins`
//   input, `<sha256> <URL>` per line). It is not on the medium: the builder
//   cuts it into the release's parts (`parts-<category>.tar.zst`), which the
//   installer downloads into the recipe cache directory (`recupdate
//   fetch-parts`), where Recipe looks before the network.
//
// DEFAULT_SET_DIR is a required define the builder passes from the install
// tools' constant. Each pinned file comes from the builder's download store
// (the optional `store` input: files named by their SHA-256, as the desktop
// lock's preseed left them), else from its URL, and is checked against its
// SHA-256 either way. The URLs are known only from the `pins` input, so they
// are downloaded in build, not declared as SOURCES.
const NEEDS = #{
    inputs: #{ pins: "file", recipes: "directory", list: "file", store: "directory?" },
    defines: #{ DEFAULT_SET_DIR: () },
    outputs: #{ medium: "directory", cache: "directory" },
    network: ["dl.fedoraproject.org:443", "kojipkgs.fedoraproject.org:443", "dl-cdn.alpinelinux.org:443", "github.com:443",
              "release-assets.githubusercontent.com:443", "objects.githubusercontent.com:443"],
    memory_mib: 8192,
    storage_max_objects: 262144,
    timeout_secs: 14400,
    runtime: "minimal",
};
const PACKAGE = #{ name: "release-medium", description: "The default software set's recipes and list for the release medium, and its package cache for the release parts" };

// The `pins` input as [sha256, URL] pairs.
fn pins() {
    let rows = [];
    for line in read_file(INPUTS.pins).split("\n") {
        let line = trim(line);
        if line == "" { continue; }
        let parts = line.split(" ");
        if parts.len() != 2 || parts[0].len() != 64 { throw "malformed pins line: " + line; }
        rows.push([parts[0].to_lower(), parts[1]]);
    }
    rows
}

// The medium tree: the recipes and the list, all read-only files.
fn put_file(medium, from, path) {
    let dest = join_path(medium, path);
    mkdir(dirname(dest));
    copy_file(from, dest);
    target_mode(medium, path, 0o644);
}

fn build(src, out) {
    // Each pinned file straight into the cache: from the store, else its URL.
    let rows = pins();
    let done = 0;
    progress(0, rows.len(), "files");
    for pin in rows {
        let file = join_path(out.cache, "sha256/" + pin[0]);
        mkdir(dirname(file));
        let stored = if INPUTS.store == () { "" } else { join_path(INPUTS.store, pin[0]) };
        if stored != "" && is_file(stored) { copy_file(stored, file); verify_sha256(file, pin[0]); }
        else { download(pin[1], file, pin[0]); }
        done += 1;
        progress(done, rows.len(), "files");
    }
    let set = DEFINES.DEFAULT_SET_DIR.sub_string(1);
    let names = [];
    for line in read_file(INPUTS.list).split("\n") {
        if trim(line) != "" { names.push(trim(line)); }
    }
    if names.is_empty() { throw "the default set lists no recipe"; }
    for name in shell_output("ls '" + INPUTS.recipes + "'").split("\n") {
        let name = trim(name);
        if name == "" { continue; }
        if !name.ends_with(".rhai") && !name.ends_with(".pins") { throw "the recipes input holds " + name + ", neither a recipe, a module nor a pins file"; }
        put_file(out.medium, join_path(INPUTS.recipes, name), set + "/recipes/" + name);
    }
    for name in names {
        if !is_file(join_path(out.medium, set + "/recipes/" + name + ".rhai")) {
            throw "the default set names " + name + ", which the recipes input lacks";
        }
    }
    put_file(out.medium, INPUTS.list, set + "/list");
    target_finalize(out.medium);
}

fn verify(out) {
    if !is_file(join_path(out.medium, DEFINES.DEFAULT_SET_DIR.sub_string(1) + "/list")) { throw "the medium tree has no default set list"; }
}
