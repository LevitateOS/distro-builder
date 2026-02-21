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
    iso_contents_path: "/data/vince/LevitateOS/.artifacts/work/levitate/stage01-rootfs-provider/rocky/iso-contents",
    iso_name: "Rocky-10.1-x86_64-dvd1.iso",
    iso_path: "/data/vince/LevitateOS/.artifacts/work/levitate/stage01-rootfs-provider/rocky/Rocky-10.1-x86_64-dvd1.iso",
    name: "rocky-stage01-rootfs",
    rootfs_path: "/data/vince/LevitateOS/.artifacts/work/levitate/stage01-rootfs-provider/rocky/rootfs-minimal",
    rootfs_recipe_version: "s01-minimal-v11",
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
    trim(shell_output("stat -c '%s' '" + iso_path + "'"))
}

fn _legacy_fingerprint_prefix(iso_path) {
    _iso_fingerprint(iso_path) + ":"
}

fn _verify_marker_content(iso_path, sha256) {
    "sha256=" + sha256 + "\n" +
    "fingerprint=" + _iso_fingerprint(iso_path) + "\n"
}

fn _marker_is_valid(trust_dir, iso_path, sha256) {
    let expected = _verify_marker_content(iso_path, sha256);
    let marker = _verify_marker_path(trust_dir);
    if is_file(marker) {
        let content = read_file_or_empty(marker);
        if content == expected {
            return true;
        }
        // Backward compatibility: older markers used size:mtime:inode.
        // If size prefix still matches, migrate marker in-place to size-only.
        let legacy_prefix = "sha256=" + sha256 + "\n" +
            "fingerprint=" + _legacy_fingerprint_prefix(iso_path);
        if content.starts_with(legacy_prefix) {
            write_file(marker, expected);
            return true;
        }
        return false;
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

fn _ensure_trusted_iso(build_dir, iso_path, sha256, trust_dir_define, trust_iso_path) {
    let trust_dir = if trust_dir_define == "" { build_dir } else { trust_dir_define };
    let trust_target = if trust_iso_path == "" { iso_path } else { trust_iso_path };
    mkdir(trust_dir);
    if _marker_is_valid(trust_dir, trust_target, sha256) {
        log("Rocky ISO trust marker hit; skipping checksum.");
        return;
    }

    let create_trust = trim(shell_output("printf '%s' \"${FORCE_TRUST_CREATE:-0}\"")) == "1";
    if !create_trust {
        throw "Rocky ISO trust marker missing/stale for '" + trust_target + "'.\n" +
              "Checksum-in-build is disabled by policy.\n" +
              "Run once with FORCE_TRUST_CREATE=1 after explicit trusted acquisition.";
    }

    log("Creating Rocky ISO trust marker (one-time full checksum)...");
    verify_sha256(trust_target, sha256);
    _write_verify_marker(trust_dir, trust_target, sha256);
}

fn _find_rpm_path(iso_contents, pkg_name) {
    let baseos = join_path(iso_contents, "BaseOS/Packages");
    let appstream = join_path(iso_contents, "AppStream/Packages");
    let cmd = "find '" + baseos + "' '" + appstream + "' " +
              "-type f -name '" + pkg_name + "-[0-9]*.rpm' 2>/dev/null | " +
              "grep -E '(x86_64|noarch)\\.rpm$' | " +
              "head -1";
    trim(shell_output(cmd))
}

fn _extract_rpm_into_rootfs(rootfs, rpm_path) {
    let cmd = "bash -o pipefail -c \"rpm2cpio '" + rpm_path + "' | cpio -idmu --quiet 2>/dev/null\"";
    shell_status_in(rootfs, cmd);
}

fn is_acquired(ctx) {
    let iso = join_path(BUILD_DIR, ctx.iso_name);
    _require_preseeded_iso(iso, ROCKY_PRESEED_ISO);
    _ensure_trusted_iso(BUILD_DIR, iso, ROCKY_SHA256, ROCKY_TRUST_DIR, ROCKY_PRESEED_ISO);

    ctx.iso_path = iso;
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs-minimal");
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);
    let iso = join_path(BUILD_DIR, ctx.iso_name);
    _require_preseeded_iso(iso, ROCKY_PRESEED_ISO);
    _ensure_trusted_iso(BUILD_DIR, iso, ROCKY_SHA256, ROCKY_TRUST_DIR, ROCKY_PRESEED_ISO);

    ctx.iso_path = iso;
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs-minimal");
    ctx
}

fn is_built(ctx) {
    let rootfs = join_path(BUILD_DIR, "rootfs-minimal");
    if !is_dir(join_path(rootfs, "usr")) {
        throw "rootfs not built";
    }
    let marker_path = join_path(rootfs, ".stage01-rootfs.recipe-version");
    let marker = trim(read_file_or_empty(marker_path));
    if marker != ctx.rootfs_recipe_version {
        throw "rootfs recipe version mismatch (" + marker + " != " + ctx.rootfs_recipe_version + ")";
    }
    let required_paths = [
        "usr/lib/systemd/systemd",
        "usr/sbin/sshd",
        "usr/sbin/ip",
        "usr/lib64/libbpf.so.1",
        "usr/lib64/libelf.so.1",
        "usr/lib64/libmnl.so.0",
        "usr/lib64/libzstd.so.1",
        "usr/lib/systemd/system/sshd.service",
        "usr/bin/systemd-tmpfiles",
        "usr/bin/udevadm",
        "usr/sbin/modprobe",
        "etc/ssh/sshd_config",
        "usr/share/empty.sshd",
        "usr/lib/locale/C.utf8/LC_CTYPE",
        "usr/lib64/libgssapi_krb5.so.2",
        "usr/lib64/libkrb5.so.3",
        "usr/lib64/libk5crypto.so.3",
        "usr/lib64/libkrb5support.so.0",
        "usr/lib64/libcom_err.so.2",
        "usr/lib64/libkeyutils.so.1",
        "usr/lib64/libverto.so.1",
        "etc/pam.d/system-auth",
        "etc/pam.d/password-auth",
        "etc/pam.d/postlogin",
    ];
    for rel in required_paths {
        let full = join_path(rootfs, rel);
        if !exists(full) {
            throw "minimal Stage 01 rootfs incomplete: missing " + rel;
        }
    }

    ctx.iso_path = join_path(BUILD_DIR, ctx.iso_name);
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = rootfs;
    ctx
}

fn build(ctx) {
    let iso = join_path(BUILD_DIR, ctx.iso_name);
    let iso_contents = join_path(BUILD_DIR, "iso-contents");
    let rootfs = join_path(BUILD_DIR, "rootfs-minimal");

    _require_preseeded_iso(iso, ROCKY_PRESEED_ISO);
    _ensure_trusted_iso(BUILD_DIR, iso, ROCKY_SHA256, ROCKY_TRUST_DIR, ROCKY_PRESEED_ISO);

    if !is_dir(join_path(iso_contents, "BaseOS/Packages")) || !is_dir(join_path(iso_contents, "AppStream/Packages")) {
        log("Extracting Rocky ISO contents...");
        mkdir(iso_contents);
        shell("7z x -o" + iso_contents + " " + iso + " -y");
    }

    if !is_dir(join_path(iso_contents, "BaseOS/Packages")) || !is_dir(join_path(iso_contents, "AppStream/Packages")) {
        throw "Rocky ISO package repositories missing after extraction (expected BaseOS/Packages and AppStream/Packages under " + iso_contents + ")";
    }

    // Build Stage 01 source rootfs from ISO RPMs via Recipe-native extraction.
    // No dnf/microdnf/yum in stage builds.
    shell("chmod -R u+rwX " + rootfs + " >/dev/null 2>&1 || true");
    shell("rm -rf " + rootfs);
    mkdir(rootfs);
    let packages = [
        // Root filesystem skeleton and core runtime.
        "filesystem",
        "setup",
        "basesystem",
        "bash",
        "coreutils",
        "coreutils-common",
        "findutils",
        "grep",
        "gawk",
        "ncurses-libs",
        "sed",
        "diffutils",
        // System boot/service runtime for Stage 01.
        "systemd",
        "systemd-libs",
        "systemd-udev",
        "iproute",
        "kmod",
        "util-linux",
        "util-linux-core",
        "libmount",
        "libblkid",
        "libsmartcols",
        "libuuid",
        "shadow-utils",
        // SSH + service wiring expectations.
        "openssh-server",
        "openssh",
        "openssh-clients",
        // RPM dependency solving is not active in this extraction path; include
        // OpenSSH Kerberos runtime libs explicitly for sshd-session.
        "krb5-libs",
        "keyutils-libs",
        "libverto",
        "libcom_err",
        "dbus",
        "dbus-broker",
        // Locale + console essentials.
        "glibc",
        "glibc-common",
        "glibc-minimal-langpack",
        "kbd",
        "kbd-misc",
        // Common runtime libs used by copied binaries.
        "libgcc",
        "libstdc++",
        "pam",
        "pam-libs",
        "libacl",
        "libattr",
        "libcap-ng",
        "libeconf",
        "libseccomp",
        "pcre2",
        "libxcrypt",
        "libcap",
        "libbpf",
        "elfutils-libelf",
        "libmnl",
        "libzstd",
        "audit-libs",
        "libselinux",
        "openssl-libs",
        "zlib-ng-compat",
        "xz-libs",
        "procps-ng",
    ];
    log("Extracting Stage 01 package set into rootfs...");
    for pkg in packages {
        let rpm_path = _find_rpm_path(iso_contents, pkg);
        if rpm_path == "" {
            throw "required Stage 01 package missing from Rocky ISO repos: " + pkg;
        }
        if !is_file(rpm_path) {
            throw "resolved RPM path is not a file: " + rpm_path;
        }
        log("  Extracting " + basename(rpm_path));
        _extract_rpm_into_rootfs(rootfs, rpm_path);
        // RPM payloads can carry restrictive directory modes; keep tree writable
        // so subsequent package extraction can add files into existing dirs.
        shell("chmod -R u+rwX " + rootfs + " >/dev/null 2>&1 || true");
    }

    // Normalize expected producer source directories even when package layout is sparse.
    shell("mkdir -p '" + join_path(rootfs, "usr/lib/systemd") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/lib/tmpfiles.d") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/lib/udev") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/lib/kbd") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/lib64") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/bin") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/sbin") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/libexec") + "'");
    shell("mkdir -p '" + join_path(rootfs, "usr/share/dbus-1") + "'");
    shell("mkdir -p '" + join_path(rootfs, "etc") + "'");
    shell("mkdir -p '" + join_path(rootfs, "var") + "'");
    if !exists(join_path(rootfs, "usr/bin/sh")) {
        shell("ln -sfn bash '" + join_path(rootfs, "usr/bin/sh") + "'");
    }
    if !exists(join_path(rootfs, "usr/sbin/agetty")) && exists(join_path(rootfs, "usr/bin/agetty")) {
        shell("ln -sfn ../bin/agetty '" + join_path(rootfs, "usr/sbin/agetty") + "'");
    }
    if !exists(join_path(rootfs, "usr/sbin/modprobe")) && exists(join_path(rootfs, "usr/bin/kmod")) {
        shell("ln -sfn ../bin/kmod '" + join_path(rootfs, "usr/sbin/modprobe") + "'");
    }
    if !exists(join_path(rootfs, "usr/bin/login")) && exists(join_path(rootfs, "usr/sbin/login")) {
        shell("ln -sfn ../sbin/login '" + join_path(rootfs, "usr/bin/login") + "'");
    }

    // RPM scriptlets are not executed in this producer path. Ensure OpenSSH
    // privilege-separation account entries exist so sshd can start in Stage 01.
    let group_file = join_path(rootfs, "etc/group");
    let gshadow_file = join_path(rootfs, "etc/gshadow");
    let passwd_file = join_path(rootfs, "etc/passwd");
    let shadow_file = join_path(rootfs, "etc/shadow");
    shell("touch '" + group_file + "' '" + gshadow_file + "' '" + passwd_file + "' '" + shadow_file + "'");
    shell("grep -q '^sshd:' '" + group_file + "' || printf 'sshd:x:74:\\n' >> '" + group_file + "'");
    shell("grep -q '^sshd:' '" + gshadow_file + "' || printf 'sshd:!::\\n' >> '" + gshadow_file + "'");
    shell("grep -q '^sshd:' '" + passwd_file + "' || printf 'sshd:x:74:74:Privilege-separated SSH:/run/sshd:/sbin/nologin\\n' >> '" + passwd_file + "'");
    shell("grep -q '^sshd:' '" + shadow_file + "' || printf 'sshd:!:20000:0:99999:7:::\\n' >> '" + shadow_file + "'");

    // RPM scriptlets are not executed in this producer path, so PAM include
    // stack files normally materialized by auth tooling must be created here.
    let pam_dir = join_path(rootfs, "etc/pam.d");
    let factory_system_auth = join_path(rootfs, "usr/share/factory/etc/pam.d/system-auth");
    let system_auth = join_path(pam_dir, "system-auth");
    let password_auth = join_path(pam_dir, "password-auth");
    let postlogin = join_path(pam_dir, "postlogin");
    shell("mkdir -p '" + pam_dir + "'");
    if !exists(system_auth) && exists(factory_system_auth) {
        shell("cp -f '" + factory_system_auth + "' '" + system_auth + "'");
    }
    if !exists(password_auth) {
        write_file(
            password_auth,
            "#%PAM-1.0\n" +
            "auth        include      system-auth\n" +
            "account     include      system-auth\n" +
            "password    include      system-auth\n" +
            "session     include      system-auth\n"
        );
    }
    if !exists(postlogin) {
        write_file(
            postlogin,
            "#%PAM-1.0\n" +
            "session     optional     pam_keyinit.so force revoke\n"
        );
    }

    // Enforce merged-usr symlink contract expected by Stage 01 envelope.
    shell("mkdir -p '" + join_path(rootfs, "usr/bin") + "' '" + join_path(rootfs, "usr/sbin") + "' '" + join_path(rootfs, "usr/lib") + "' '" + join_path(rootfs, "usr/lib64") + "'");
    shell("ln -sfn usr/bin '" + join_path(rootfs, "bin") + "'");
    shell("ln -sfn usr/sbin '" + join_path(rootfs, "sbin") + "'");
    shell("ln -sfn usr/lib '" + join_path(rootfs, "lib") + "'");
    shell("ln -sfn usr/lib64 '" + join_path(rootfs, "lib64") + "'");

    // Keep rootfs recipe outputs deterministic and lean.
    shell("rm -rf '" + join_path(rootfs, "var/cache") + "' '" + join_path(rootfs, "var/log") + "'");
    shell("mkdir -p '" + join_path(rootfs, "var/cache") + "' '" + join_path(rootfs, "var/log") + "'");
    write_file(join_path(rootfs, ".stage01-rootfs.recipe-version"), ctx.rootfs_recipe_version + "\n");

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
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs-minimal");
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
