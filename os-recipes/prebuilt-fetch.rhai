const RECIPE_API = 1;
// Fetch one published prebuilt: the release `<family>-<key16>` of the prebuilt
// repository holds `prebuilt.json` and the archive of the programs.
//   --define BASE_URL=https://github.com/<owner>/<repo>/releases/download/<tag>
//   --define KEY=<64 hex>      the key the builder computed; prebuilt.json must carry it
//   --define ARCHIVE=<asset>   the archive's file name (`<family>-<name>.tar.gz`)
// Output `files` holds the extracted programs and `manifest` is prebuilt.json. The
// builder checks every file against the manifest's SHA-256 again before using it;
// a 404 or any mismatch fails this recipe, which the builder treats as "no prebuilt".
// The URL and digest come from the defines and the downloaded manifest, so
// everything is downloaded in build, not declared as SOURCES.
const NEEDS = #{
    defines: #{ BASE_URL: (), KEY: (), ARCHIVE: () },
    outputs: #{ files: "directory", manifest: "file" },
    network: ["github.com:443", "release-assets.githubusercontent.com:443"],
    network_max_bytes: 536870912,
    memory_mib: 2048,
    timeout_secs: 900,
    runtime: "minimal",
};
const PACKAGE = #{ name: "prebuilt-fetch", description: "One published prebuilt: its manifest and extracted programs" };

fn build(src, out) {
    let base_url = DEFINES.BASE_URL;
    let key = DEFINES.KEY;
    let archive_name = DEFINES.ARCHIVE;
    if !base_url.starts_with("https://github.com/") || key.len() != 64 ||
        archive_name == "" || archive_name.contains("/") {
        throw "Define BASE_URL (https://github.com/...), KEY (64 hex) and ARCHIVE (a file name)";
    }
    fetch_bounded(base_url + "/prebuilt.json", out.manifest, 1048576);
    let manifest = parse_json(read_file(out.manifest));
    if manifest.key != key { throw "prebuilt.json is for another key"; }
    let archive = manifest.archive;
    if archive.name != archive_name { throw "prebuilt.json names another archive"; }
    let file = join_path(WORK, archive_name);
    download_exact(base_url + "/" + archive_name, file, archive.sha256, archive.bytes);
    extract(file, out.files);
}

fn verify(out) {
    if !is_dir(out.files) { throw "the prebuilt programs are missing"; }
    if !is_file(out.manifest) { throw "prebuilt.json is missing"; }
}
