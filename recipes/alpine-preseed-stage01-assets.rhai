// Canonical Alpine Stage 01 preseed recipe.
//
// This recipe performs explicit trusted acquisition into the canonical cache:
//   BUILD_DIR/
//     alpine-extended-<version>-x86_64.iso
//     apk-tools-static-<version>.apk
//
// Current canonical assets:
// - release branch: v3.23
// - alpine-extended-3.23.2-x86_64.iso
// - ISO SHA256: 8d50854936dba58e7616ac3bfa07ad67dac0347305fec1cba6d288cf5df1577d
// - apk-tools-static-3.0.5-r0.apk
// - apk-tools SHA256: 6e832468b4efebd6ab7245e4989ca38b6e4589a9772d52a03b58c690a7aeacbe
//
// Optional env:
// - ALPINE_FORCE_REFRESH=1 (remove existing assets and download again)

let ctx = #{
    apk_tools_path: join_path(BUILD_DIR, "apk-tools-static-3.0.5-r0.apk"),
    apk_tools_sha256: "6e832468b4efebd6ab7245e4989ca38b6e4589a9772d52a03b58c690a7aeacbe",
    apk_tools_version: "3.0.5-r0",
    description: "Canonical Alpine Stage 01 preseed",
    iso_path: join_path(BUILD_DIR, "alpine-extended-3.23.2-x86_64.iso"),
    iso_sha256: "8d50854936dba58e7616ac3bfa07ad67dac0347305fec1cba6d288cf5df1577d",
    iso_version: "3.23.2",
    name: "alpine-preseed-stage01-assets",
    release_branch: "v3.23",
    trust_dir: BUILD_DIR,
};

fn _iso_name(ctx) {
    "alpine-extended-" + ctx.iso_version + "-x86_64.iso"
}

fn _iso_url(ctx) {
    "https://dl-cdn.alpinelinux.org/alpine/" +
    ctx.release_branch +
    "/releases/x86_64/" +
    _iso_name(ctx)
}

fn _apk_tools_name(ctx) {
    "apk-tools-static-" + ctx.apk_tools_version + ".apk"
}

fn _apk_tools_url(ctx) {
    "https://dl-cdn.alpinelinux.org/alpine/" +
    ctx.release_branch +
    "/main/x86_64/" +
    _apk_tools_name(ctx)
}

fn _verify_marker_path(trust_dir) {
    join_path(trust_dir, ".alpine-stage01-preseed.marker")
}

fn _fingerprint(path) {
    trim(shell_output("stat -c '%s' '" + path + "'"))
}

fn _verify_marker_content(ctx) {
    "iso_sha256=" + ctx.iso_sha256 + "\n" +
    "iso_fingerprint=" + _fingerprint(ctx.iso_path) + "\n" +
    "apk_tools_sha256=" + ctx.apk_tools_sha256 + "\n" +
    "apk_tools_fingerprint=" + _fingerprint(ctx.apk_tools_path) + "\n"
}

fn _marker_is_valid(ctx) {
    let marker = _verify_marker_path(ctx.trust_dir);
    if !is_file(marker) {
        return false;
    }
    read_file_or_empty(marker) == _verify_marker_content(ctx)
}

fn _write_verify_marker(ctx) {
    write_file(_verify_marker_path(ctx.trust_dir), _verify_marker_content(ctx));
}

fn _refresh_requested() {
    trim(shell_output("printf '%s' \"${ALPINE_FORCE_REFRESH:-0}\"")) == "1"
}

fn _ensure_parent_dir(path) {
    let parent = dirname(path);
    if parent != "" {
        mkdir(parent);
    }
}

fn _remove_marker(ctx) {
    let marker = _verify_marker_path(ctx.trust_dir);
    if is_file(marker) {
        rm(marker);
    }
}

fn is_acquired(ctx) {
    if _refresh_requested() {
        throw "forced refresh";
    }
    if !is_file(ctx.iso_path) {
        throw "Alpine Stage 01 ISO missing at " + ctx.iso_path;
    }
    if !is_file(ctx.apk_tools_path) {
        throw "Alpine Stage 01 apk-tools package missing at " + ctx.apk_tools_path;
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    if !_marker_is_valid(ctx) {
        throw "Alpine Stage 01 trust marker missing/stale under " + ctx.trust_dir;
    }
    ctx
}

fn acquire(ctx) {
    mkdir(ctx.trust_dir);
    _ensure_parent_dir(ctx.iso_path);
    _ensure_parent_dir(ctx.apk_tools_path);

    if _refresh_requested() {
        log("ALPINE_FORCE_REFRESH=1: removing existing Alpine Stage 01 preseed assets");
        if is_file(ctx.iso_path) { rm(ctx.iso_path); }
        if is_file(ctx.apk_tools_path) { rm(ctx.apk_tools_path); }
        _remove_marker(ctx);
    }

    if !is_file(ctx.iso_path) {
        log("Downloading " + basename(ctx.iso_path) + "...");
        let downloaded = download(_iso_url(ctx), ctx.iso_path);
        if downloaded == "" {
            throw "failed to download Alpine ISO from " + _iso_url(ctx);
        }
    } else {
        log("Alpine ISO already present at preseed path; re-validating checksum.");
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);

    if !is_file(ctx.apk_tools_path) {
        log("Downloading " + basename(ctx.apk_tools_path) + "...");
        let downloaded = download(_apk_tools_url(ctx), ctx.apk_tools_path);
        if downloaded == "" {
            throw "failed to download apk-tools-static from " + _apk_tools_url(ctx);
        }
    } else {
        log("apk-tools package already present at preseed path; re-validating checksum.");
    }
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);

    _write_verify_marker(ctx);
    ctx
}

fn is_built(ctx) {
    if _refresh_requested() {
        throw "forced refresh";
    }
    if !is_file(ctx.iso_path) {
        throw "Alpine Stage 01 ISO missing at " + ctx.iso_path;
    }
    if !is_file(ctx.apk_tools_path) {
        throw "Alpine Stage 01 apk-tools package missing at " + ctx.apk_tools_path;
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    if !_marker_is_valid(ctx) {
        throw "Alpine Stage 01 trust marker missing/stale under " + ctx.trust_dir;
    }
    ctx
}

fn build(ctx) {
    ctx
}

fn is_installed(ctx) {
    if !is_file(ctx.iso_path) {
        throw "Alpine Stage 01 ISO missing at " + ctx.iso_path;
    }
    if !is_file(ctx.apk_tools_path) {
        throw "Alpine Stage 01 apk-tools package missing at " + ctx.apk_tools_path;
    }
    if !_marker_is_valid(ctx) {
        throw "Alpine Stage 01 trust marker missing/stale under " + ctx.trust_dir;
    }
    ctx
}

fn install(ctx) {
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "auto.acquire.failure" {
        // Ensure retries do not reuse partial files.
        if is_file(ctx.iso_path) {
            rm(ctx.iso_path);
        }
        if is_file(ctx.apk_tools_path) {
            rm(ctx.apk_tools_path);
        }
    }
    ctx
}
