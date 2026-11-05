// Canonical Alpine rootfs source preseed recipe.
//
// This recipe performs explicit trusted acquisition into the canonical cache:
//   BUILD_DIR/
//     alpine-extended-<version>-x86_64.iso
//     apk-tools-static-<version>.apk
//
// Pinned versions, filenames, URLs, hashes, and marker format: sources/alpine.rhai.
//
// Optional env:
// - ALPINE_FORCE_REFRESH=1 (download verified replacements for existing assets)

import "sources/alpine.rhai" as source;
let assets = source::facts();

let ctx = #{
    apk_tools_path: join_path(BUILD_DIR, assets.apk_tools_name),
    apk_tools_sha256: assets.apk_tools_sha256,
    apk_tools_version: assets.apk_tools_version,
    description: "Canonical Alpine rootfs source preseed",
    iso_path: join_path(BUILD_DIR, assets.iso_name),
    iso_sha256: assets.iso_sha256,
    iso_version: assets.iso_version,
    iso_name: assets.iso_name,
    apk_tools_name: assets.apk_tools_name,
    iso_url: assets.iso_url,
    apk_tools_url: assets.apk_tools_url,
    name: "alpine-preseed-source-assets",
    release_branch: assets.release_branch,
    trust_dir: BUILD_DIR,
};

fn _refresh_requested() {
    trim(env("ALPINE_FORCE_REFRESH")) == "1"
}

fn _ensure_parent_dir(path) {
    let parent = dirname(path);
    if parent != "" {
        mkdir(parent);
    }
}

fn is_acquired(ctx) {
    if _refresh_requested() {
        throw "forced refresh";
    }
    if !is_file(ctx.iso_path) {
        throw "Alpine rootfs source ISO missing at " + ctx.iso_path;
    }
    if !is_file(ctx.apk_tools_path) {
        throw "Alpine rootfs source apk-tools package missing at " + ctx.apk_tools_path;
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    if !source::marker_is_valid(ctx.trust_dir, ctx.iso_path, ctx.iso_sha256, ctx.apk_tools_path, ctx.apk_tools_sha256) {
        throw "Alpine rootfs source trust marker missing/stale under " + ctx.trust_dir;
    }
    ctx
}

fn acquire(ctx) {
    mkdir(ctx.trust_dir);
    _ensure_parent_dir(ctx.iso_path);
    _ensure_parent_dir(ctx.apk_tools_path);

    let refresh = _refresh_requested();
    if refresh || !is_file(ctx.iso_path) {
        log("Downloading " + basename(ctx.iso_path) + "...");
        download(ctx.iso_url, ctx.iso_path, ctx.iso_sha256);
    } else {
        verify_sha256(ctx.iso_path, ctx.iso_sha256);
    }

    if refresh || !is_file(ctx.apk_tools_path) {
        log("Downloading " + basename(ctx.apk_tools_path) + "...");
        download(ctx.apk_tools_url, ctx.apk_tools_path, ctx.apk_tools_sha256);
    } else {
        verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    }

    source::write_marker(ctx.trust_dir, ctx.iso_path, ctx.iso_sha256, ctx.apk_tools_path, ctx.apk_tools_sha256);
    ctx
}

fn is_built(ctx) {
    is_acquired(ctx)
}

fn build(ctx) {
    ctx
}

fn is_installed(ctx) {
    // Preseeding has one result: all declared assets are verified and reusable.
    is_acquired(ctx)
}

fn install(ctx) {
    ctx
}
