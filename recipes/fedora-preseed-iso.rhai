// Canonical Fedora Server 43 x86_64 DVD preseed recipe for rootfs source flows.
//
// This recipe performs explicit trusted acquisition into the canonical cache:
//   BUILD_DIR/Fedora-Server-dvd-x86_64-43-1.6.iso
//
// Pinned source facts and marker format: sources/fedora.rhai.
//
// Trusted acquire behavior:
// - verify the pinned SHA256 against the published CHECKSUM manifest
// - acquire the ISO via torrent only after that manifest check passes
// - verify the local ISO content and write a local trust marker
//
// Optional env:
// - ROOTFS_SOURCE_FORCE_REFRESH=1 (generic rootfs source refresh request)
// - FEDORA_FORCE_REFRESH=1 (remove existing ISO and download again)

import "sources/fedora.rhai" as source;
let assets = source::facts();

let ctx = #{
    outputs: #{},
    checksum_url: assets.checksum_url,
    description: "Canonical Fedora Server 43 x86_64 DVD preseed",
    iso_name: assets.iso_name,
    iso_path: join_path(BUILD_DIR, assets.iso_name),
    name: "fedora-preseed-iso",
    sha256: assets.sha256,
    torrent_url: assets.torrent_url,
    trust_dir: BUILD_DIR,
};

fn _refresh_requested() {
    let generic = env("ROOTFS_SOURCE_FORCE_REFRESH");
    if generic != "" {
        return trim(generic) == "1";
    }
    trim(env("FEDORA_FORCE_REFRESH")) == "1"
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

fn _expected_checksum_line(ctx) {
    "SHA256 (" + ctx.iso_name + ") = " + ctx.sha256
}

fn _verify_published_checksum(ctx) {
    log("Validating pinned SHA256 against published CHECKSUM manifest...");
    let manifest = http_get(ctx.checksum_url);
    let needle = _expected_checksum_line(ctx);
    if !manifest.contains(needle) {
        throw "Published CHECKSUM manifest at '" + ctx.checksum_url + "' does not contain expected line:\n" +
              needle;
    }
}

fn is_acquired(ctx) {
    if _refresh_requested() {
        throw "forced refresh";
    }
    if !is_file(ctx.iso_path) {
        throw "Fedora ISO missing at " + ctx.iso_path;
    }
    verify_sha256(ctx.iso_path, ctx.sha256);
    if !source::marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.sha256) {
        throw "Fedora ISO trust marker missing/stale for '" + ctx.iso_path + "'";
    }
    ctx.outputs = #{iso: ctx.iso_path};
    ctx
}

fn acquire(ctx) {
    _ensure_parent_dir(ctx.iso_path);
    mkdir(ctx.trust_dir);

    if _refresh_requested() && is_file(ctx.iso_path) {
        log("Rootfs source refresh requested: removing existing Fedora ISO before trusted acquisition");
        rm(ctx.iso_path);
    }

    _verify_published_checksum(ctx);

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
    source::write_marker(ctx.trust_dir, ctx.iso_path, ctx.sha256);
    ctx.outputs = #{iso: ctx.iso_path};
    ctx
}

fn is_built(ctx) {
    is_acquired(ctx)
}

fn build(ctx) {
    ctx
}

fn is_installed(ctx) {
    // Preseeding has one result: the declared ISO is verified and reusable.
    is_acquired(ctx)
}

fn install(ctx) {
    ctx.outputs = #{iso: ctx.iso_path};
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
