// Alpine Stage 01 rootfs source producer for OpenRC variants.
//
// Produces:
// - BUILD_DIR/rootfs
//
// Required defines:
// - ALPINE_RELEASE_BRANCH (example: v3.23)
// - ALPINE_EXTENDED_VERSION (example: 3.23.2)
// - ALPINE_EXTENDED_SHA256
// - ALPINE_APK_TOOLS_VERSION (example: 3.0.4-r0)
// - ALPINE_APK_TOOLS_SHA256
// - ALPINE_STAGE01_PROFILE (example: iuppiter|acorn)
// - ALPINE_PRESEED_ISO
// - ALPINE_PRESEED_APK_TOOLS
// - ALPINE_TRUST_DIR
//
// This recipe intentionally keeps all outputs under BUILD_DIR so Stage 01 can
// consume it through non-legacy rootfs_source wiring.
// Stage builds never download/redownload Alpine assets.

let ctx = #{
    alpine_version: ALPINE_EXTENDED_VERSION,
    apk_static_path: "",
    apk_tools_preseed_path: ALPINE_PRESEED_APK_TOOLS,
    apk_tools_sha256: ALPINE_APK_TOOLS_SHA256,
    apk_tools_version: ALPINE_APK_TOOLS_VERSION,
    description: "Alpine Stage 01 rootfs source producer",
    iso_path: "",
    iso_preseed_path: ALPINE_PRESEED_ISO,
    iso_sha256: ALPINE_EXTENDED_SHA256,
    name: "alpine-stage01-rootfs",
    profile: ALPINE_STAGE01_PROFILE,
    release_branch: ALPINE_RELEASE_BRANCH,
    rootfs_path: "",
    rootfs_recipe_version: "alpine-stage01-v5",
    trust_dir: ALPINE_TRUST_DIR,
};

fn _iso_name(ctx) {
    "alpine-extended-" + ctx.alpine_version + "-x86_64.iso"
}

fn _apk_tools_name(ctx) {
    "apk-tools-static-" + ctx.apk_tools_version + ".apk"
}

fn _verify_marker_path(trust_dir) {
    join_path(trust_dir, ".alpine-stage01-preseed.marker")
}

fn _fingerprint(path) {
    trim(shell_output("stat -c '%s' '" + path + "'"))
}

fn _verify_marker_content(iso_path, iso_sha256, apk_tools_path, apk_tools_sha256) {
    "iso_sha256=" + iso_sha256 + "\n" +
    "iso_fingerprint=" + _fingerprint(iso_path) + "\n" +
    "apk_tools_sha256=" + apk_tools_sha256 + "\n" +
    "apk_tools_fingerprint=" + _fingerprint(apk_tools_path) + "\n"
}

fn _marker_is_valid(trust_dir, iso_path, iso_sha256, apk_tools_path, apk_tools_sha256) {
    let marker = _verify_marker_path(trust_dir);
    if !is_file(marker) {
        return false;
    }
    let expected = _verify_marker_content(iso_path, iso_sha256, apk_tools_path, apk_tools_sha256);
    read_file_or_empty(marker) == expected
}

fn _ensure_parent_dir(path) {
    let parent = dirname(path);
    if parent != "" {
        mkdir(parent);
    }
}

fn _ensure_build_asset_present(build_path, preseed_path, label) {
    if is_file(build_path) {
        return;
    }
    if preseed_path == "" || !is_file(preseed_path) {
        return;
    }
    _ensure_parent_dir(build_path);
    shell("cp --reflink=auto '" + preseed_path + "' '" + build_path + "'");
    log("Staged " + label + " from trusted preseed cache: " + preseed_path);
}

fn _require_preseeded_asset(build_path, preseed_path, label, remediation) {
    _ensure_build_asset_present(build_path, preseed_path, label);
    if is_file(build_path) {
        return;
    }
    throw label + " is missing at '" + build_path + "'.\n" +
          "Stage builds forbid download/redownload.\n" +
          "Pre-seed canonical cache at '" + preseed_path + "'.\n" +
          remediation;
}

fn _require_trusted_preseed_assets(ctx, remediation) {
    if !is_file(ctx.iso_preseed_path) {
        throw "Alpine preseed ISO missing at '" + ctx.iso_preseed_path + "'.\n" + remediation;
    }
    if !is_file(ctx.apk_tools_preseed_path) {
        throw "Alpine preseed apk-tools package missing at '" + ctx.apk_tools_preseed_path + "'.\n" + remediation;
    }
    if !_marker_is_valid(
        ctx.trust_dir,
        ctx.iso_preseed_path,
        ctx.iso_sha256,
        ctx.apk_tools_preseed_path,
        ctx.apk_tools_sha256
    ) {
        throw "Alpine Stage 01 trust marker missing/stale under '" + ctx.trust_dir + "'.\n" +
              remediation;
    }
}

fn _profile_packages(profile) {
    let normalized = trim(profile).to_lower();
    if normalized == "acorn" {
        return "alpine-base busybox openrc openrc-init " +
               "openssh openssh-server util-linux e2fsprogs dosfstools " +
               "systemd-boot " +
               "iproute2 iputils dhcpcd less grep findutils curl " +
               "musl-locales musl-locales-lang";
    }

    // Default profile is iuppiter.
    "alpine-base busybox openrc openrc-init " +
    "openssh openssh-server util-linux e2fsprogs dosfstools " +
    "systemd-boot " +
    "iproute2 iputils dhcpcd less grep findutils " +
    "musl-locales musl-locales-lang"
}

fn _network_interfaces() {
    "# /etc/network/interfaces - Stage 01 OpenRC live\n" +
    "auto lo\n" +
    "iface lo inet loopback\n\n" +
    "# QEMU slirp default interface\n" +
    "auto eth0\n" +
    "iface eth0 inet dhcp\n"
}

fn _rootfs_marker(rootfs) {
    join_path(rootfs, ".stage01-rootfs.recipe-version")
}

fn _resolve_apk_static(apk_tools_dir) {
    let candidates = [
        join_path(apk_tools_dir, "sbin/apk.static"),
        join_path(apk_tools_dir, "usr/sbin/apk.static"),
        join_path(apk_tools_dir, "apk.static"),
    ];
    for candidate in candidates {
        if is_file(candidate) {
            return candidate;
        }
    }
    ""
}

fn _extract_apk_tools_static(apk_tools_pkg, apk_tools_dir) {
    extract_with_format(apk_tools_pkg, apk_tools_dir, "apk");
    let resolved = _resolve_apk_static(apk_tools_dir);
    if resolved == "" {
        throw "apk.static not found after extraction in " + apk_tools_dir;
    }
    chmod(resolved, 0o755);
    resolved
}

fn _rootfs_valid(rootfs, expected_version) {
    let marker = _rootfs_marker(rootfs);
    if !is_dir(rootfs) || !is_dir(join_path(rootfs, "bin")) || !is_file(marker) {
        return false;
    }
    if !is_file(join_path(rootfs, "bin/busybox")) {
        return false;
    }
    let sh_path = join_path(rootfs, "bin/sh");
    if shell_status("[ -L '" + sh_path + "' ] || [ -f '" + sh_path + "' ]") != 0 {
        return false;
    }
    if !is_dir(join_path(rootfs, "usr/share/empty.sshd")) {
        return false;
    }
    let locale_conf = join_path(rootfs, "etc/locale.conf");
    if !is_file(locale_conf) {
        return false;
    }
    if shell_status("grep -qx 'LANG=C.UTF-8' '" + locale_conf + "'") != 0 {
        return false;
    }
    if !is_file(join_path(rootfs, "usr/share/i18n/locales/musl/en_US.UTF-8")) {
        return false;
    }
    if !is_file(join_path(rootfs, "etc/network/interfaces")) {
        return false;
    }
    if shell_status("grep -qx 'iface eth0 inet dhcp' '" + join_path(rootfs, "etc/network/interfaces") + "'") != 0 {
        return false;
    }
    if !is_file(join_path(rootfs, "etc/init.d/networking")) {
        return false;
    }
    if !is_file(join_path(rootfs, "etc/init.d/dhcpcd")) {
        return false;
    }
    let mkfs_fat_ok = shell_status("[ -x '" + join_path(rootfs, "sbin/mkfs.fat") + "' ] || [ -x '" + join_path(rootfs, "usr/sbin/mkfs.fat") + "' ]") == 0;
    if !mkfs_fat_ok {
        return false;
    }
    if !is_file(join_path(rootfs, "usr/lib/systemd/boot/efi/systemd-bootx64.efi")) {
        return false;
    }
    trim(read_file(marker)) == expected_version
}

fn _normalize_rootfs_permissions(rootfs, context) {
    if !is_dir(rootfs) {
        return;
    }

    // Stage 01 rootfs is copied via rsync; source files must be owner-readable.
    let chmod_cmd = "chmod -R u+rX '" + rootfs + "'";
    if shell_status(chmod_cmd) != 0 {
        throw "failed to normalize rootfs permissions during " + context +
              ": chmod -R u+rX returned non-zero for " + rootfs;
    }

    let unreadable_cmd = "find '" + rootfs + "' -xdev -type f ! -perm -u+r -print -quit";
    let unreadable = trim(shell_output(unreadable_cmd));
    if unreadable != "" {
        throw "rootfs remains owner-unreadable after permission normalization during " +
              context + ": " + unreadable;
    }
}

fn _ensure_openrc_stage01_completeness(rootfs) {
    mkdir(join_path(rootfs, "usr/share/empty.sshd"));
    shell("chmod 755 '" + join_path(rootfs, "usr/share/empty.sshd") + "'");
    write_file(join_path(rootfs, "etc/locale.conf"), "LANG=C.UTF-8\n");
    mkdir(join_path(rootfs, "etc/network"));
    write_file(join_path(rootfs, "etc/network/interfaces"), _network_interfaces());
    mkdir(join_path(rootfs, "etc/conf.d"));
    write_file(join_path(rootfs, "etc/conf.d/dhcpcd"), "dhcpcd_args=\"--quiet\"\n");
}

fn is_acquired(ctx) {
    let remediation = "Run 'cargo run -p distro-builder --bin distro-builder -- artifact preseed-alpine-stage01-assets " + ctx.profile + " --refresh'";
    let iso_path = join_path(BUILD_DIR, _iso_name(ctx));
    _require_preseeded_asset(iso_path, ctx.iso_preseed_path, "Alpine ISO", remediation);
    _require_trusted_preseed_assets(ctx, remediation);
    let apk_tools_dir = join_path(BUILD_DIR, "apk-tools");
    let apk_tools_pkg = join_path(apk_tools_dir, _apk_tools_name(ctx));
    _require_preseeded_asset(
        apk_tools_pkg,
        ctx.apk_tools_preseed_path,
        "apk-tools-static package",
        remediation
    );
    let apk_static = _resolve_apk_static(apk_tools_dir);

    if apk_static == "" {
        throw "apk.static not acquired under " + apk_tools_dir;
    }

    ctx.iso_path = iso_path;
    ctx.apk_static_path = apk_static;
    ctx
}

fn acquire(ctx) {
    let remediation = "Run 'cargo run -p distro-builder --bin distro-builder -- artifact preseed-alpine-stage01-assets " + ctx.profile + " --refresh'";
    mkdir(BUILD_DIR);
    let iso_dest = join_path(BUILD_DIR, _iso_name(ctx));
    _require_preseeded_asset(iso_dest, ctx.iso_preseed_path, "Alpine ISO", remediation);
    _require_trusted_preseed_assets(ctx, remediation);
    verify_sha256(iso_dest, ctx.iso_sha256);

    let apk_tools_dir = join_path(BUILD_DIR, "apk-tools");
    let apk_tools_pkg = join_path(apk_tools_dir, _apk_tools_name(ctx));
    let apk_static = _resolve_apk_static(apk_tools_dir);
    mkdir(apk_tools_dir);

    _require_preseeded_asset(
        apk_tools_pkg,
        ctx.apk_tools_preseed_path,
        "apk-tools-static package",
        remediation
    );
    verify_sha256(apk_tools_pkg, ctx.apk_tools_sha256);

    if apk_static == "" {
        log("Extracting apk-tools-static...");
        ctx.apk_static_path = _extract_apk_tools_static(apk_tools_pkg, apk_tools_dir);
    } else {
        ctx.apk_static_path = apk_static;
    }

    ctx.iso_path = iso_dest;
    ctx
}

fn is_built(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs");
    if !_rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        throw "rootfs not built";
    }
    ctx.rootfs_path = rootfs;
    ctx
}

fn build(ctx) {
    let iso_contents = join_path(BUILD_DIR, "iso-contents");
    if !is_dir(join_path(iso_contents, "apks")) {
        log("Extracting Alpine ISO...");
        shell("rm -rf '" + iso_contents + "'");
        mkdir(iso_contents);
        if shell_status("command -v xorriso >/dev/null 2>&1") == 0 {
            let extract_cmd = "xorriso -osirrox on -indev '" + ctx.iso_path;
            extract_cmd += "' -extract / '" + iso_contents;
            extract_cmd += "/'";
            shell(extract_cmd);
        } else if shell_status("command -v 7z >/dev/null 2>&1") == 0 {
            let extract_cmd = "7z x -y -o'" + iso_contents;
            extract_cmd += "' '" + ctx.iso_path;
            extract_cmd += "' >/dev/null";
            shell(extract_cmd);
        } else {
            throw "missing ISO extractor: install xorriso or 7z";
        }
    }
    if !is_dir(join_path(iso_contents, "apks")) {
        throw "Alpine ISO extraction missing apks dir under " + iso_contents;
    }

    let rootfs = join_path(BUILD_DIR, "rootfs");
    if is_dir(rootfs) {
        // Normalize readability on cached rootfs before validity checks.
        _normalize_rootfs_permissions(rootfs, "cached rootfs preflight");
    }
    if _rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        log("Stage 01 Alpine rootfs already current; skipping rebuild");
        ctx.rootfs_path = rootfs;
        return ctx;
    }

    shell("rm -rf '" + rootfs + "'");
    mkdir(rootfs);
    mkdir(join_path(rootfs, "etc/apk"));
    mkdir(join_path(rootfs, "etc/apk/keys"));
    mkdir(join_path(rootfs, "var/cache/apk"));

    let repositories = join_path(iso_contents, "apks") + "\n";
    repositories += "https://dl-cdn.alpinelinux.org/alpine/" + ctx.release_branch;
    repositories += "/main\n";
    repositories += "https://dl-cdn.alpinelinux.org/alpine/" + ctx.release_branch;
    repositories += "/community\n";
    write_file(join_path(rootfs, "etc/apk/repositories"), repositories);

    let packages = _profile_packages(ctx.profile);
    log("Installing Alpine Stage 01 package baseline (" + ctx.profile + ")...");
    let apk_cmd = "'" + ctx.apk_static_path + "' --root '" + rootfs + "' " +
        "--usermode --initdb --no-progress --allow-untrusted add " + packages;
    shell(apk_cmd);
    _normalize_rootfs_permissions(rootfs, "post-apk install");
    _ensure_openrc_stage01_completeness(rootfs);

    if !is_file(join_path(rootfs, "bin/busybox")) {
        throw "invalid rootfs: missing /bin/busybox after apk install";
    }
    let sh_path = join_path(rootfs, "bin/sh");
    if shell_status("[ -L '" + sh_path + "' ] || [ -f '" + sh_path + "' ]") != 0 {
        throw "invalid rootfs: missing /bin/sh path after apk install";
    }

    write_file(_rootfs_marker(rootfs), ctx.rootfs_recipe_version + "\n");
    ctx.rootfs_path = rootfs;
    ctx
}

fn is_installed(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs");
    if !_rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        throw "rootfs not installed";
    }
    ctx.rootfs_path = rootfs;
    ctx
}

fn install(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs");
    if !_rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        throw "cannot install: rootfs missing or stale at " + rootfs;
    }
    ctx.rootfs_path = rootfs;
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" {
        let rootfs = join_path(BUILD_DIR, "rootfs");
        let marker = _rootfs_marker(rootfs);
        if is_file(marker) {
            rm(marker);
        }
    }
    ctx
}
