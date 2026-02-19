// Rocky Stage 01 rootfs source producer for Levitate.
//
// Rule boundary (strict):
// - This recipe never downloads/re-downloads the Rocky DVD in stage builds.
// - It requires a preseeded ISO at BUILD_DIR/ROCKY_ISO_NAME.
// - It requires a trust marker; no repeated checksum in normal iterations.
//
// Required defines:
// - ROCKY_ISO_NAME
// - ROCKY_SHA256
// - ROCKY_SHA256_URL
// - ROCKY_TORRENT_URL
// - ROCKY_PRESEED_ISO
// - ROCKY_TRUST_DIR

let deps = ["rocky-deps"];

let ctx = #{
    description: "Rocky rootfs producer for Levitate Stage 01",
    iso_contents_path: "/data/vince/LevitateOS/.artifacts/out/levitate/s01-boot/stage01-rootfs-provider/rocky/iso-contents",
    iso_name: "Rocky-10.1-x86_64-dvd1.iso",
    iso_path: "/data/vince/LevitateOS/.artifacts/out/levitate/s01-boot/stage01-rootfs-provider/rocky/Rocky-10.1-x86_64-dvd1.iso",
    name: "rocky-stage01-rootfs",
    rootfs_path: "/data/vince/LevitateOS/.artifacts/out/levitate/s01-boot/stage01-rootfs-provider/rocky/rootfs",
    sha256: "55f96d45a052c0ed4f06309480155cb66281a008691eb7f3f359957205b1849a",
    sha256_url: "https://download.rockylinux.org/pub/rocky/10/isos/x86_64/CHECKSUM",
    torrent_url: "https://download.rockylinux.org/pub/rocky/10/isos/x86_64/Rocky-10.1-x86_64-dvd1.torrent",
};

fn _verify_marker_path(trust_dir) {
    join_path(trust_dir, ".rocky-iso-trust.marker")
}

fn _legacy_verify_marker_path(trust_dir) {
    join_path(trust_dir, ".rocky-iso-verified.marker")
}

fn _iso_fingerprint(iso_path) {
    trim(shell_output("stat -c '%s:%Y:%i' '" + iso_path + "'"))
}

fn _verify_marker_content(iso_path, sha256) {
    "sha256=" + sha256 + "\n" +
    "fingerprint=" + _iso_fingerprint(iso_path) + "\n"
}

fn _marker_is_valid(trust_dir, iso_path, sha256) {
    let expected = _verify_marker_content(iso_path, sha256);
    let marker = _verify_marker_path(trust_dir);
    if is_file(marker) {
        return read_file_or_empty(marker) == expected;
    }

    let legacy = _legacy_verify_marker_path(trust_dir);
    if is_file(legacy) && read_file_or_empty(legacy) == expected {
        // Migrate old marker name to current trust marker.
        write_file(marker, expected);
        return true;
    }

    false
}

fn _write_verify_marker(trust_dir, iso_path, sha256) {
    write_file(_verify_marker_path(trust_dir), _verify_marker_content(iso_path, sha256));
}

fn _ensure_build_iso_present(iso_path, preseed_iso) {
    if is_file(iso_path) {
        return;
    }
    if preseed_iso == "" || !is_file(preseed_iso) {
        return;
    }
    let dst_dir = dirname(iso_path);
    mkdir(dst_dir);
    shell("cp --reflink=auto '" + preseed_iso + "' '" + iso_path + "'");
    log("Staged Rocky ISO from trusted preseed cache: " + preseed_iso);
}

fn _require_preseeded_iso(iso_path, preseed_iso) {
    _ensure_build_iso_present(iso_path, preseed_iso);
    if is_file(iso_path) {
        return;
    }
    throw "Rocky ISO is missing at '" + iso_path + "'.\n" +
          "Stage builds forbid download/redownload.\n" +
          "Pre-seed canonical cache at '" + preseed_iso + "', then run once with FORCE_TRUST_CREATE=1.";
}

fn _ensure_trusted_iso(build_dir, iso_path, sha256, trust_dir_define) {
    let trust_dir = if trust_dir_define == "" { build_dir } else { trust_dir_define };
    mkdir(trust_dir);
    if _marker_is_valid(trust_dir, iso_path, sha256) {
        log("Rocky ISO trust marker hit; skipping checksum.");
        return;
    }

    let create_trust = trim(shell_output("printf '%s' \"${FORCE_TRUST_CREATE:-0}\"")) == "1";
    if !create_trust {
        throw "Rocky ISO trust marker missing/stale for '" + iso_path + "'.\n" +
              "Checksum-in-build is disabled by policy.\n" +
              "Run once with FORCE_TRUST_CREATE=1 after explicit trusted acquisition.";
    }

    log("Creating Rocky ISO trust marker (one-time full checksum)...");
    verify_sha256(iso_path, sha256);
    _write_verify_marker(trust_dir, iso_path, sha256);
}

fn is_acquired(ctx) {
    let iso = join_path(BUILD_DIR, ctx.iso_name);
    _require_preseeded_iso(iso, ROCKY_PRESEED_ISO);
    _ensure_trusted_iso(BUILD_DIR, iso, ROCKY_SHA256, ROCKY_TRUST_DIR);

    ctx.iso_path = iso;
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs");
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);
    let iso = join_path(BUILD_DIR, ctx.iso_name);
    _require_preseeded_iso(iso, ROCKY_PRESEED_ISO);
    _ensure_trusted_iso(BUILD_DIR, iso, ROCKY_SHA256, ROCKY_TRUST_DIR);

    ctx.iso_path = iso;
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs");
    ctx
}

fn is_built(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs");
    if !is_dir(join_path(rootfs, "usr")) {
        throw "rootfs not built";
    }

    ctx.iso_path = join_path(BUILD_DIR, ctx.iso_name);
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = rootfs;
    ctx
}

fn build(ctx) {
    let iso = join_path(BUILD_DIR, ctx.iso_name);
    let iso_contents = join_path(BUILD_DIR, "iso-contents");
    let install_img = join_path(iso_contents, "images/install.img");
    let rootfs = join_path(BUILD_DIR, "rootfs");

    _require_preseeded_iso(iso, ROCKY_PRESEED_ISO);
    _ensure_trusted_iso(BUILD_DIR, iso, ROCKY_SHA256, ROCKY_TRUST_DIR);

    if !is_file(install_img) {
        log("Extracting Rocky ISO contents...");
        mkdir(iso_contents);
        shell("7z x -o" + iso_contents + " " + iso + " -y");
    }

    if !is_file(install_img) {
        throw "install image missing after ISO extraction: " + install_img;
    }

    if !is_dir(join_path(rootfs, "usr")) {
        log("Extracting install.img to rootfs...");
        shell("rm -rf " + rootfs);
        shell("unsquashfs -no-xattrs -d " + rootfs + " " + install_img);
        shell("chmod -R u+rw " + rootfs);
    }

    ctx.iso_path = iso;
    ctx.iso_contents_path = iso_contents;
    ctx.rootfs_path = rootfs;
    ctx
}

fn is_installed(ctx) {
    // Force deterministic refresh of the stage source every invocation.
    // Returning a throwing path here ensures the recipe executes acquire/build/install
    // instead of returning early on cached state.
    throw "forced reinstall";
}

fn install(ctx) {
    ctx.iso_path = join_path(BUILD_DIR, ctx.iso_name);
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs");
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" {
        let iso_contents = join_path(BUILD_DIR, "iso-contents");
        if is_dir(iso_contents) {
            rm(iso_contents);
        }
    }
    ctx
}
