const RECIPE_API = 1;
// The root effects recipe of LevitateOS and AcornOS. The base ships it as
// /usr/lib/recipe/effects.rhai (installed-system.rhai, `root_effects`), and
// Recipe runs it after every system change to a root, with that root as the
// read-only input `root` (tools/recipe README, "Root effects"). It rebuilds
// the whole-root caches the packages' scripts and triggers would have
// rebuilt, which never run: GSettings schemas, GIO modules, GdkPixbuf
// loaders, GTK input methods, the MIME database, the font caches and the
// X server's font indexes, icon caches and the desktop database. Each runs
// only where the root has its tool and its files.
//
// Every tool is the root's own, run through the root's own dynamic loader
// with the root's libraries first (the libc's loader names which: glibc on
// LevitateOS, musl on AcornOS), so one recipe serves both systems. The root
// is read-only: a tool that writes beside its input runs on a scratch
// directory of links into the root, and only the files it generates are
// kept. The `system` output holds those generated files and nothing else;
// Recipe records it as the reserved package system-effects, replacing that
// package's files at each run, and finishes it as a system tree.
//
// ldconfig is not among them: /etc/ld.so.cache must be written inside the
// root, which this recipe sees only read-only, so Recipe itself runs the
// root's own ldconfig after the effects, on a glibc root (tools/recipe
// README, "Library cache"). musl has no cache.
//
// The caches follow the root, which changed: the recipe runs at every change
// (RERUN) and makes them again, except an icon cache whose inputs did not
// change, which it keeps as it is (icon_caches).
const NEEDS = #{
    inputs: #{ root: "directory" },
    outputs: #{ system: "directory" },
    memory_mib: 4096,
    storage_max_objects: 262144,
    timeout_secs: 3600,
};
// No name here: the image installs this file as /usr/lib/recipe/effects.rhai, and Recipe takes the name from the file name.
const PACKAGE = #{ version: "1", description: "The whole-root caches of a LevitateOS or AcornOS root" };
const RERUN = true;

// The root's C library, by its dynamic loader: [loader, library directories,
// the library directory the caches live under].
fn libc_of(root) {
    if is_file(join_path(root, "usr/lib64/ld-linux-x86-64.so.2")) {
        return ["usr/lib64/ld-linux-x86-64.so.2", ["usr/lib64"], "usr/lib64"];
    }
    if is_file(join_path(root, "lib/ld-musl-x86_64.so.1")) {
        return ["lib/ld-musl-x86_64.so.1", ["lib", "usr/lib"], "usr/lib"];
    }
    throw "the root has neither the glibc nor the musl dynamic loader; it is not a LevitateOS or AcornOS root";
}

// The names in directory `dir` (absolute), one per entry.
fn entries(dir) {
    let names = [];
    if !is_dir(dir) { return names; }
    for name in shell_output("ls -A '" + dir + "'").split("\n") {
        if trim(name) != "" { names.push(trim(name)); }
    }
    names
}

// `job` is this run's #{ root, system, scratch }: the root input, the output
// tree and the scratch folder under WORK.

// The first of the root's programs `names` (under usr/bin) it has, or "".
fn program(job, names) {
    for name in names {
        let path = "usr/bin/" + name;
        if is_file(join_path(job.root, path)) { return path; }
    }
    ""
}

// Runs the root's `program` (relative to the root) with `args` through the
// root's loader and libraries, and returns what it prints.
fn run(job, program, args) {
    let libc = libc_of(job.root);
    let path = "";
    for dir in libc[1] { path += (if path == "" { "" } else { ":" }) + join_path(job.root, dir); }
    log("  " + basename(program));
    exec_output(join_path(job.root, libc[0]), ["--library-path", path, join_path(job.root, program)] + args)
}

// A scratch directory `name` holding a link to every entry of the root's
// directory `dir` (absolute): a tool that writes its cache beside its input
// writes it here.
fn farm(job, name, dir) {
    let farm = join_path(job.scratch, name);
    if exists(farm) { rm(farm); }
    mkdir(farm);
    for entry in entries(dir) { ln_force(join_path(dir, entry), join_path(farm, entry)); }
    farm
}

// `from` (a generated file) at `path` in the output tree.
fn keep(job, from, path) {
    if !is_file(from) { throw "the cache tool made no " + basename(from); }
    let dest = join_path(job.system, path);
    mkdir(dirname(dest));
    if exists(dest) { rm(dest); }
    copy_file(from, dest);
    target_mode(job.system, path, 0o644);
}

// `text` at `path` in the output tree.
fn keep_text(job, path, text) {
    let dest = join_path(job.system, path);
    mkdir(dirname(dest));
    if exists(dest) { rm(dest); }
    write_file(dest, text);
    target_mode(job.system, path, 0o644);
}

fn gsettings_schemas(job) {
    let schemas = "usr/share/glib-2.0/schemas";
    let tool = program(job, ["glib-compile-schemas"]);
    if tool == "" || !is_dir(join_path(job.root, schemas)) { return; }
    let out = join_path(job.scratch, "schemas");
    mkdir(out);
    run(job, tool, ["--targetdir=" + out, join_path(job.root, schemas)]);
    keep(job, join_path(out, "gschemas.compiled"), schemas + "/gschemas.compiled");
}

fn gio_modules(job, lib) {
    let modules = lib + "/gio/modules";
    let tool = program(job, ["gio-querymodules-64", "gio-querymodules"]);
    if tool == "" || !is_dir(join_path(job.root, modules)) { return; }
    let farm = farm(job, "gio-modules", join_path(job.root, modules));
    run(job, tool, [farm]);
    // With no module that registers anything, gio-querymodules writes no
    // cache (and removes an old one): nothing to keep.
    let cache = join_path(farm, "giomodule.cache");
    if !is_file(cache) { return; }
    keep(job, cache, modules + "/giomodule.cache");
}

// A program that prints its cache from the modules it is given, which it
// names by their paths: written without the root's prefix.
fn module_cache(job, names, modules, cache) {
    let tool = program(job, names);
    if tool == "" { return; }
    let files = [];
    for entry in entries(join_path(job.root, modules)) {
        if entry.ends_with(".so") { files.push(join_path(join_path(job.root, modules), entry)); }
    }
    if files.is_empty() { return; }
    let text = run(job, tool, files);
    keep_text(job, cache, replace(text, job.root, ""));
}

fn mime_database(job) {
    let mime = "usr/share/mime";
    let tool = program(job, ["update-mime-database"]);
    if tool == "" || !is_dir(join_path(job.root, mime + "/packages")) { return; }
    let work = join_path(job.scratch, "mime");
    if exists(work) { rm(work); }
    mkdir(work);
    ln_force(join_path(job.root, mime + "/packages"), join_path(work, "packages"));
    run(job, tool, [work]);
    // Everything it wrote: the caches and the per-type files, not the
    // packages' own sources.
    for entry in entries(work) {
        if entry == "packages" { continue; }
        let from = join_path(work, entry);
        let dest = join_path(job.system, mime + "/" + entry);
        mkdir(dirname(dest));
        if exists(dest) { rm(dest); }
        shell("cp -R '" + from + "' '" + dest + "'");
    }
}

// fontconfig's caches, made against a scratch system root whose fonts and
// configuration are links into the root, so the caches land in the cache
// folder the root's configuration names first. Only a folder under /usr is
// kept, usr/lib/fontconfig/cache: Recipe does not own files under /var. Fedora
// names it in fonts.conf; Alpine's fonts.conf names /var/cache/fontconfig, so
// the desktop-libs group adds conf.d/00-system-cache.conf naming the /usr one
// first (included before fonts.conf's own cachedir lines). Also the X server's
// core font indexes (fonts.scale and fonts.dir, as mkfontdir writes them)
// for each folder of bitmap fonts.
fn fonts(job) {
    let fonts = join_path(job.root, "usr/share/fonts");
    if !is_dir(fonts) { return; }
    let tool = program(job, ["fc-cache-64", "fc-cache"]);
    if tool != "" {
        let sysroot = join_path(job.scratch, "fontconfig-root");
        if exists(sysroot) { rm(sysroot); }
        for dir in ["etc", "usr/share", "usr/lib/fontconfig/cache", "var/cache/fontconfig"] { mkdir(join_path(sysroot, dir)); }
        for link in ["etc/fonts", "usr/share/fonts", "usr/share/fontconfig"] {
            if exists(join_path(job.root, link)) { ln_force(join_path(job.root, link), join_path(sysroot, link)); }
        }
        run(job, tool, ["--sysroot=" + sysroot, "--system-only"]);
        let caches = join_path(sysroot, "usr/lib/fontconfig/cache");
        for entry in entries(caches) {
            keep(job, join_path(caches, entry), "usr/lib/fontconfig/cache/" + entry);
        }
    }
    let index = program(job, ["mkfontscale"]);
    if index == "" { return; }
    for dir in entries(fonts) {
        let path = join_path(fonts, dir);
        if dir == "encodings" || !entries(path).some(|name| name.ends_with(".pcf.gz")) { continue; }
        let out = join_path(job.scratch, "fonts/" + dir);
        if exists(out) { rm(out); }
        mkdir(out);
        run(job, index, ["-o", join_path(out, "fonts.scale"), path]);
        run(job, index, ["-b", "-s", "-l", "-o", join_path(out, "fonts.dir"), path]);
        keep(job, join_path(out, "fonts.scale"), "usr/share/fonts/" + dir + "/fonts.scale");
        keep(job, join_path(out, "fonts.dir"), "usr/share/fonts/" + dir + "/fonts.dir");
    }
}

// What theme folder `dir`'s icon cache is made from, as text. Without
// --include-image-data, gtk-update-icon-cache (gtk/updateiconcache.c,
// scan_directory and load_icon_data) reads no pixels: it records, for every
// folder below the theme, the names of the files in it that end in .png,
// .svg, .xpm or .icon (following links; a name that is neither a folder nor a
// regular file is skipped), and parses the .icon files. So the cache follows
// the tool, its arguments, those names and the .icon files' contents. It
// reads index.theme only to check that it exists; its contents are in too, to
// be safe. Not mtimes: Recipe installs files with fixed ones.
//
// Each name a program looks up is a call Ginux mediates, and following every
// link costs what the tool itself costs (breeze: 20000 names, 12000 of them
// links, 58 of those to folders that hold 34000 more names). So the tree is
// listed as it lies, with each name's own type and the type it leads to
// (one look-up per link), and only the links to folders are resolved: one
// that stays in the theme is its target, already listed, so the pair is
// recorded; one that leaves it is listed through, as the tool sees it.
//
// The lists go through files, one program at a time, not through pipes: while
// one task of a Ginux run makes a path or mapping call, Ginux stops all the
// others and gives up after 5 s, and two programs working at once (find
// feeding sort) abort the run that way ("argument peer interrupt timed
// out"). Any failing program fails the whole command (set -e), so a missing
// tool never passes for an unchanged theme. GNU find, xargs and realpath, as
// the runtime image has them.
fn icon_cache_inputs(job, tool, args, theme, dir) {
    let lists = join_path(job.scratch, "icon-inputs-" + theme);
    if exists(lists) { rm(lists); }
    mkdir(lists);
    let icon = "-name '*.png' -o -name '*.svg' -o -name '*.xpm' -o -name '*.icon'";
    let script = [
        "set -e",
        "cd '" + dir + "'",
        "L='" + lists + "'",
        // "ff", "dd", "lf", "ld" (a link to a folder), "lN" (a broken link).
        "find . \\( " + icon + " -o -type d -o -xtype d \\) -printf '%y%Y %p\\n' > \"$L/names\"",
        "LC_ALL=C sort -o \"$L/names\" \"$L/names\"",
        "sed -n 's/^ld //p' \"$L/names\" > \"$L/folder-links\"",
        "xargs -r -d '\\n' -a \"$L/folder-links\" realpath -e --relative-base=. > \"$L/folder-link-targets\"",
        "paste -d ' ' \"$L/folder-links\" \"$L/folder-link-targets\" > \"$L/folder-link-pairs\"",
        "sed -n '/^\\//p' \"$L/folder-link-targets\" > \"$L/outside\"",
        ": > \"$L/outside-names\"",
        "xargs -r -d '\\n' -a \"$L/outside\" -I{} find -L {} \\( -type d -printf 'd %p\\n' \\) -o \\( -type f \\( " + icon + " \\) -printf 'f %p\\n' \\) >> \"$L/outside-names\"",
        "LC_ALL=C sort -o \"$L/outside-names\" \"$L/outside-names\"",
        "sed -n 's/^[fl]f \\(.*\\.icon\\)$/\\1/p; s/^f \\(.*\\.icon\\)$/\\1/p' \"$L/names\" \"$L/outside-names\" > \"$L/icon-list\"",
        "xargs -r -d '\\n' -a \"$L/icon-list\" sha256sum > \"$L/icon-files\"",
        "sha256sum '" + join_path(job.root, tool) + "' index.theme \"$L/names\" \"$L/folder-link-pairs\" \"$L/outside-names\" \"$L/icon-files\"",
    ];
    let command = "";
    for line in script { command += (if command == "" { "" } else { "; " }) + line; }
    let sums = shell_output(command);
    let text = "gtk-update-icon-cache inputs 2\nargs";
    for arg in args { text += " " + arg; }
    text += "\n";
    let lines = 0;
    for line in sums.split("\n") {
        if trim(line) == "" { continue; }
        text += replace(replace(trim(line), lists + "/", ""), job.root + "/", "") + "\n";
        lines += 1;
    }
    if lines != 6 { throw "sha256sum gave " + lines + " sums, not 6"; }
    text
}

// Each theme's cache, made again only when what it is made from changed. The
// text icon_cache_inputs gives is kept beside the cache, as
// .icon-theme.cache.inputs (in the same package and folder, so it comes and
// goes with the cache; the tool skips it, as it skips every name without an
// icon suffix). A theme whose text matches the root's copy keeps the root's
// cache, copied as it is: the output replaces system-effects whole, so a
// cache not in it would be removed.
fn icon_caches(job) {
    let icons = join_path(job.root, "usr/share/icons");
    let tool = program(job, ["gtk-update-icon-cache"]);
    if tool == "" { return; }
    let args = ["-q", "-t", "-f"];
    for theme in entries(icons) {
        let dir = join_path(icons, theme);
        if !is_file(join_path(dir, "index.theme")) { continue; }
        let cache_path = "usr/share/icons/" + theme + "/icon-theme.cache";
        let inputs_path = "usr/share/icons/" + theme + "/.icon-theme.cache.inputs";
        // Inputs that cannot be read make the cache again, as every run did
        // before, and keep no inputs file.
        let inputs = "";
        try {
            inputs = icon_cache_inputs(job, tool, args, theme, dir);
        } catch (error) {
            log(`  icon cache of ${theme}: its inputs could not be read (${error})`);
        }
        let old_cache = join_path(job.root, cache_path);
        let old_inputs = join_path(job.root, inputs_path);
        if inputs != "" && is_file(old_cache) && is_file(old_inputs) && read_file(old_inputs) == inputs {
            log("  icon cache of " + theme + ": unchanged, kept");
            keep(job, old_cache, cache_path);
            keep_text(job, inputs_path, inputs);
            continue;
        }
        log("  icon cache of " + theme + ": making");
        let farm = farm(job, "icons-" + theme, dir);
        run(job, tool, args + [farm]);
        // A theme with no icon files (a cursor-only theme such as `default`,
        // which only names the theme it inherits) gets no cache from the tool.
        let cache = join_path(farm, "icon-theme.cache");
        if !is_file(cache) { continue; }
        keep(job, cache, cache_path);
        if inputs != "" { keep_text(job, inputs_path, inputs); }
    }
}

fn desktop_database(job) {
    let applications = "usr/share/applications";
    let tool = program(job, ["update-desktop-database"]);
    if tool == "" || !is_dir(join_path(job.root, applications)) { return; }
    let farm = farm(job, "applications", join_path(job.root, applications));
    run(job, tool, ["-q", farm]);
    keep(job, join_path(farm, "mimeinfo.cache"), applications + "/mimeinfo.cache");
}

fn build(src, out) {
    let job = #{ root: INPUTS.root, system: out, scratch: join_path(WORK, "scratch") };
    let lib = libc_of(job.root)[2];
    if exists(job.scratch) { rm(job.scratch); }
    mkdir(job.scratch);
    log("Rebuilding the root's caches...");
    gsettings_schemas(job);
    gio_modules(job, lib);
    module_cache(job, ["gdk-pixbuf-query-loaders-64", "gdk-pixbuf-query-loaders"],
        lib + "/gdk-pixbuf-2.0/2.10.0/loaders", lib + "/gdk-pixbuf-2.0/2.10.0/loaders.cache");
    module_cache(job, ["gtk-query-immodules-3.0-64", "gtk-query-immodules-3.0"],
        lib + "/gtk-3.0/3.0.0/immodules", lib + "/gtk-3.0/3.0.0/immodules.cache");
    mime_database(job);
    fonts(job);
    icon_caches(job);
    desktop_database(job);
}
