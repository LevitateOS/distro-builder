// Shared pinned package facts; no acquisition side effects.
fn facts() {
    let version = "43";
    let release = "1.6";
    let iso_name = "Fedora-Server-dvd-x86_64-" + version + "-" + release + ".iso";
    #{
        iso_name: iso_name,
        sha256: "aca06983bef83da9b43144c1a2ff4c8483e4745167c17f53725c16a16742e643",
        checksum_url: "https://dl.fedoraproject.org/pub/fedora/linux/releases/" + version +
            "/Server/x86_64/iso/Fedora-Server-" + version + "-" + release + "-x86_64-CHECKSUM",
        iso_url: "https://dl.fedoraproject.org/pub/fedora/linux/releases/" + version +
            "/Server/x86_64/iso/" + iso_name,
    }
}
