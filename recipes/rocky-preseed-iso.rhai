// Canonical Rocky ISO preseed recipe for Stage 01 source producer flows.
//
// This recipe performs explicit trusted acquisition into the canonical cache:
//   BUILD_DIR/Rocky-10.1-x86_64-dvd1.iso
//
// Canonical artifact:
// - Rocky-10.1-x86_64-dvd1.iso
// - SHA256: 55f96d45a052c0ed4f06309480155cb66281a008691eb7f3f359957205b1849a
// - CHECKSUM: https://download.rockylinux.org/pub/rocky/10/isos/x86_64/CHECKSUM
// - Torrent: https://download.rockylinux.org/pub/rocky/10/isos/x86_64/Rocky-10.1-x86_64-dvd1.torrent
//
// Optional env:
// - ROCKY_FORCE_REFRESH=1 (remove existing ISO and download again)

let ctx = #{
    description: "Canonical Rocky ISO preseed",
    iso_name: "Rocky-10.1-x86_64-dvd1.iso",
    iso_path: join_path(BUILD_DIR, "Rocky-10.1-x86_64-dvd1.iso"),
    name: "rocky-preseed-iso",
    sha256: "55f96d45a052c0ed4f06309480155cb66281a008691eb7f3f359957205b1849a",
    sha256_url: "https://download.rockylinux.org/pub/rocky/10/isos/x86_64/CHECKSUM",
    torrent_url: "https://download.rockylinux.org/pub/rocky/10/isos/x86_64/Rocky-10.1-x86_64-dvd1.torrent",
    trust_dir: BUILD_DIR,
};

fn _verify_marker_path(trust_dir) {
    join_path(trust_dir, ".rocky-iso-trust.marker")
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
    trim(shell_output("printf '%s' \"${ROCKY_FORCE_REFRESH:-0}\"")) == "1"
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
        throw "Rocky ISO missing at " + ctx.iso_path;
    }
    verify_sha256(ctx.iso_path, ctx.sha256);
    if !_marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.sha256) {
        throw "Rocky ISO trust marker missing/stale for '" + ctx.iso_path + "'";
    }
    ctx
}

fn acquire(ctx) {
    _ensure_parent_dir(ctx.iso_path);
    mkdir(ctx.trust_dir);

    if _refresh_requested() && is_file(ctx.iso_path) {
        log("ROCKY_FORCE_REFRESH=1: removing existing Rocky ISO before trusted acquisition");
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
        log("Rocky ISO already present at preseed path; re-validating checksum.");
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
        throw "Rocky ISO missing at " + ctx.iso_path;
    }
    verify_sha256(ctx.iso_path, ctx.sha256);
    if !_marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.sha256) {
        throw "Rocky ISO trust marker missing/stale for '" + ctx.iso_path + "'";
    }
    ctx
}

fn build(ctx) {
    ctx
}

fn is_installed(ctx) {
    if !is_file(ctx.iso_path) {
        throw "Rocky ISO missing at " + ctx.iso_path;
    }
    if !_marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.sha256) {
        throw "Rocky ISO trust marker missing/stale for '" + ctx.iso_path + "'";
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
