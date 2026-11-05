// Shared package facts and preseed marker format; no acquisition side effects.
fn facts() {
    let version = "43";
    let release = "1.6";
    let iso_name = "Fedora-Server-dvd-x86_64-" + version + "-" + release + ".iso";
    #{
        iso_name: iso_name,
        sha256: "aca06983bef83da9b43144c1a2ff4c8483e4745167c17f53725c16a16742e643",
        checksum_url: "https://download.fedoraproject.org/pub/fedora/linux/releases/" + version +
            "/Server/x86_64/iso/Fedora-Server-" + version + "-" + release + "-x86_64-CHECKSUM",
        torrent_url: "https://torrent.fedoraproject.org/torrents/Fedora-Server-dvd-x86_64-" + version + ".torrent",
    }
}

fn marker_path(trust_dir) {
    join_path(trust_dir, ".fedora-iso-trust.marker")
}

fn marker_content(iso_path, sha256) {
    let size = trim(exec_output("stat", ["-c", "%s", "--", iso_path]));
    "sha256=" + sha256 + "\nfingerprint=" + size + "\n"
}

fn marker_is_valid(trust_dir, iso_path, sha256) {
    let marker = marker_path(trust_dir);
    is_file(marker) && read_file(marker) == marker_content(iso_path, sha256)
}

fn write_marker(trust_dir, iso_path, sha256) {
    write_file(marker_path(trust_dir), marker_content(iso_path, sha256));
}
