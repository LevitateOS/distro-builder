// Canonical Fedora Server 43 x86_64 DVD preseed recipe for Stage 01 source producer flows.
//
// This recipe performs explicit trusted acquisition into the canonical cache:
//   .artifacts/work/<distro>/downloads/Fedora-Server-dvd-x86_64-43-1.6.iso
//
// Required defines:
// - FEDORA_ISO_NAME
// - FEDORA_SHA256
// - FEDORA_CHECKSUM_URL
// - FEDORA_TORRENT_URL
// - FEDORA_PRESEED_ISO
// - FEDORA_TRUST_DIR
//
// Current canonical artifact:
// - Fedora-Server-dvd-x86_64-43-1.6.iso
// - SHA256: aca06983bef83da9b43144c1a2ff4c8483e4745167c17f53725c16a16742e643
// - CHECKSUM: https://download.fedoraproject.org/pub/fedora/linux/releases/43/Server/x86_64/iso/Fedora-Server-43-1.6-x86_64-CHECKSUM
// - Torrent: https://torrent.fedoraproject.org/torrents/Fedora-Server-dvd-x86_64-43.torrent
//
// Optional env:
// - FEDORA_FORCE_REFRESH=1 (remove existing ISO and download again)

let ctx = #{
    checksum_url: FEDORA_CHECKSUM_URL,
    description: "Canonical Fedora Server 43 x86_64 DVD preseed",
    iso_name: FEDORA_ISO_NAME,
    iso_path: FEDORA_PRESEED_ISO,
    name: "fedora-preseed-iso",
    sha256: FEDORA_SHA256,
    torrent_url: FEDORA_TORRENT_URL,
    trust_dir: FEDORA_TRUST_DIR,
};

fn _verify_marker_path(trust_dir) {
    join_path(trust_dir, ".fedora-iso-trust.marker")
}

fn _iso_fingerprint(iso_path) {
    trim(shell_output("stat -c '%s' '" + iso_path + "'"))
}

fn _verify_marker_content(iso_path, sha256) {
    "sha256=" + sha256 + "\n" +
    "fingerprint=" + _iso_fingerprint(iso_path) + "\n"
}

fn _marker_is_valid(trust_dir, iso_path, sha256) {
    let marker = _verify_marker_path(trust_dir);
    if !is_file(marker) {
        return false;
    }
    read_file_or_empty(marker) == _verify_marker_content(iso_path, sha256)
}

fn _write_verify_marker(trust_dir, iso_path, sha256) {
    write_file(_verify_marker_path(trust_dir), _verify_marker_content(iso_path, sha256));
}

fn _refresh_requested() {
    trim(shell_output("printf '%s' \"${FEDORA_FORCE_REFRESH:-0}\"")) == "1"
}

fn _ensure_parent_dir(path) {
    let parent = dirname(path);
    if parent != "" {
        mkdir(parent);
    }
}

fn _normalize_torrent_output(path, iso_name) {
    let resolved = path;
    if is_dir(path) {
        let expected = join_path(path, iso_name);
        if !is_file(expected) {
            throw "torrent produced directory without expected ISO '" + iso_name + "': " + path;
        }
        resolved = expected;
    }
    if basename(resolved) != iso_name {
        throw "torrent produced unexpected filename: " + basename(resolved) + " (expected " + iso_name + ")";
    }
    resolved
}

fn is_acquired(ctx) {
    if _refresh_requested() {
        throw "forced refresh";
    }
    if !is_file(ctx.iso_path) {
        throw "Fedora ISO missing at " + ctx.iso_path;
    }
    verify_sha256(ctx.iso_path, ctx.sha256);
    if !_marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.sha256) {
        throw "Fedora ISO trust marker missing/stale for '" + ctx.iso_path + "'";
    }
    ctx
}

fn acquire(ctx) {
    _ensure_parent_dir(ctx.iso_path);
    mkdir(ctx.trust_dir);

    if _refresh_requested() && is_file(ctx.iso_path) {
        log("FEDORA_FORCE_REFRESH=1: removing existing Fedora ISO before trusted acquisition");
        rm(ctx.iso_path);
    }

    if !is_file(ctx.iso_path) {
        let target_dir = dirname(ctx.iso_path);
        log("Downloading " + ctx.iso_name + " via torrent...");
        let fetched = torrent(ctx.torrent_url, target_dir);
        let resolved = _normalize_torrent_output(fetched, ctx.iso_name);
        if resolved != ctx.iso_path {
            mv(resolved, ctx.iso_path);
        }
    } else {
        log("Fedora ISO already present at preseed path; re-validating checksum.");
    }

    verify_sha256(ctx.iso_path, ctx.sha256);
    _write_verify_marker(ctx.trust_dir, ctx.iso_path, ctx.sha256);
    ctx
}

fn is_built(ctx) {
    if _refresh_requested() {
        throw "forced refresh";
    }
    if !is_file(ctx.iso_path) {
        throw "Fedora ISO missing at " + ctx.iso_path;
    }
    verify_sha256(ctx.iso_path, ctx.sha256);
    if !_marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.sha256) {
        throw "Fedora ISO trust marker missing/stale for '" + ctx.iso_path + "'";
    }
    ctx
}

fn build(ctx) {
    ctx
}

fn is_installed(ctx) {
    if !is_file(ctx.iso_path) {
        throw "Fedora ISO missing at " + ctx.iso_path;
    }
    if !_marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.sha256) {
        throw "Fedora ISO trust marker missing/stale for '" + ctx.iso_path + "'";
    }
    ctx
}

fn install(ctx) {
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "auto.acquire.failure" {
        // Ensure retries do not reuse partially downloaded content.
        if is_file(ctx.iso_path) {
            rm(ctx.iso_path);
        }
    }
    ctx
}
