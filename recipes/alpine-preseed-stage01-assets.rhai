// Canonical Alpine Stage 01 preseed recipe.
//
// This recipe performs explicit trusted acquisition into the canonical cache:
//   .artifacts/work/<distro>/downloads/
//     alpine-extended-<version>-x86_64.iso
//     apk-tools-static-<version>.apk
//
// Required defines:
// - ALPINE_RELEASE_BRANCH
// - ALPINE_EXTENDED_VERSION
// - ALPINE_EXTENDED_SHA256
// - ALPINE_APK_TOOLS_VERSION
// - ALPINE_APK_TOOLS_SHA256
// - ALPINE_PRESEED_ISO
// - ALPINE_PRESEED_APK_TOOLS
// - ALPINE_TRUST_DIR
//
// Optional env:
// - ALPINE_FORCE_REFRESH=1 (remove existing assets and download again)

let ctx = #{
    apk_tools_path: ALPINE_PRESEED_APK_TOOLS,
    apk_tools_sha256: ALPINE_APK_TOOLS_SHA256,
    apk_tools_version: ALPINE_APK_TOOLS_VERSION,
    description: "Canonical Alpine Stage 01 preseed",
    iso_path: ALPINE_PRESEED_ISO,
    iso_sha256: ALPINE_EXTENDED_SHA256,
    iso_version: ALPINE_EXTENDED_VERSION,
    name: "alpine-preseed-stage01-assets",
    release_branch: ALPINE_RELEASE_BRANCH,
    trust_dir: ALPINE_TRUST_DIR,
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
