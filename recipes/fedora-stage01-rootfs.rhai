// Fedora Server 43 Stage 01 rootfs source producer for Levitate/Ralph.
//
// Rule boundary (strict):
// - This recipe never downloads/re-downloads the Fedora Server DVD in stage builds.
// - It requires a preseeded ISO in the canonical sibling downloads cache.
// - It requires a trust marker; no repeated checksum in normal iterations.
//
// Current implementation note:
// - This recipe intentionally reuses `rocky-deps` because that owner currently
//   provides only the generic host-side `7zz` bootstrap needed to extract the ISO.
//   The dependency naming should be neutralized later in the migration.

let deps = ["rocky-deps"];

fn _work_root(build_dir) {
    dirname(dirname(build_dir))
}

fn _downloads_dir(build_dir) {
    let parent = dirname(build_dir);
    if basename(parent) == "stage01-rootfs-provider" {
        return join_path(dirname(parent), "downloads");
    }
    join_path(parent, "downloads")
}

let ctx = #{
    checksum_url: "https://download.fedoraproject.org/pub/fedora/linux/releases/43/Server/x86_64/iso/Fedora-Server-43-1.6-x86_64-CHECKSUM",
    description: "Fedora Server 43 Stage 01 rootfs producer",
    iso_contents_path: join_path(BUILD_DIR, "iso-contents"),
    iso_name: "Fedora-Server-dvd-x86_64-43-1.6.iso",
    iso_path: join_path(BUILD_DIR, "Fedora-Server-dvd-x86_64-43-1.6.iso"),
    name: "fedora-stage01-rootfs",
    preseed_iso_path: join_path(_downloads_dir(BUILD_DIR), "Fedora-Server-dvd-x86_64-43-1.6.iso"),
    rootfs_path: join_path(BUILD_DIR, "rootfs-minimal"),
    rootfs_recipe_version: "fedora-stage01-v1",
    sha256: "aca06983bef83da9b43144c1a2ff4c8483e4745167c17f53725c16a16742e643",
    torrent_url: "https://torrent.fedoraproject.org/torrents/Fedora-Server-dvd-x86_64-43.torrent",
    trust_dir: _downloads_dir(BUILD_DIR),
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

fn _ensure_build_iso_present(iso_path, preseed_iso) {
    if is_file(iso_path) {
        return;
    }
    if preseed_iso == "" || !is_file(preseed_iso) {
        return;
    }
    let dst_dir = dirname(iso_path);
    mkdir(dst_dir);
    copy_file_reflink(preseed_iso, iso_path);
    log("Staged Fedora ISO from trusted preseed cache: " + preseed_iso);
}

fn _require_preseeded_iso(iso_path, preseed_iso) {
    _ensure_build_iso_present(iso_path, preseed_iso);
    if is_file(iso_path) {
        return;
    }
    throw "Fedora ISO is missing at '" + iso_path + "'.\n" +
          "Stage builds forbid download/redownload.\n" +
          "Pre-seed canonical cache at '" + preseed_iso + "', then run once with FORCE_TRUST_CREATE=1.";
}

fn _ensure_trusted_iso(build_dir, iso_path, sha256, trust_dir_define, trust_iso_path) {
    let trust_dir = if trust_dir_define == "" { build_dir } else { trust_dir_define };
    let trust_target = if trust_iso_path == "" { iso_path } else { trust_iso_path };
    mkdir(trust_dir);
    if _marker_is_valid(trust_dir, trust_target, sha256) {
        log("Fedora ISO trust marker hit; skipping checksum.");
        return;
    }

    let create_trust = trim(shell_output("printf '%s' \"${FORCE_TRUST_CREATE:-0}\"")) == "1";
    if !create_trust {
        throw "Fedora ISO trust marker missing/stale for '" + trust_target + "'.\n" +
              "Checksum-in-build is disabled by policy.\n" +
              "Run once with FORCE_TRUST_CREATE=1 after explicit trusted acquisition.";
    }

    log("Creating Fedora ISO trust marker (one-time full checksum)...");
    verify_sha256(trust_target, sha256);
    _write_verify_marker(trust_dir, trust_target, sha256);
}

fn _find_rpm_path(iso_contents, pkg_name) {
    let cmd = "find '" + iso_contents + "' " +
              "-type f -path '*/Packages/" + pkg_name + "-[0-9]*.rpm' 2>/dev/null | " +
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
    _require_preseeded_iso(iso, ctx.preseed_iso_path);
    _ensure_trusted_iso(BUILD_DIR, iso, ctx.sha256, ctx.trust_dir, ctx.preseed_iso_path);

    ctx.iso_path = iso;
    ctx.iso_contents_path = join_path(BUILD_DIR, "iso-contents");
    ctx.rootfs_path = join_path(BUILD_DIR, "rootfs-minimal");
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);
    let iso = join_path(BUILD_DIR, ctx.iso_name);
    _require_preseeded_iso(iso, ctx.preseed_iso_path);
    _ensure_trusted_iso(BUILD_DIR, iso, ctx.sha256, ctx.trust_dir, ctx.preseed_iso_path);

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
        "usr/sbin/auditd",
        "usr/sbin/ip",
        "usr/lib64/libbpf.so.1",
        "usr/lib64/libelf.so.1",
        "usr/lib64/libmnl.so.0",
        "usr/lib64/libzstd.so.1",
        "usr/lib/systemd/system/sshd.service",
        "usr/lib/systemd/system/auditd.service",
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
        "usr/bin/sfdisk",
        "usr/bin/ping",
        "usr/bin/curl",
        "usr/bin/lspci",
        "usr/bin/lsusb",
        "usr/bin/less",
        "usr/lib64/libbrotlidec.so.1",
        "usr/lib64/libbrotlicommon.so.1",
        "usr/lib64/libevent-2.1.so.7",
        "usr/lib64/libsasl2.so.3",
        "usr/lib/systemd/system/dbus.socket",
        "usr/lib/systemd/system/dbus-broker.service",
        "etc/systemd/system/dbus.service",
        "etc/systemd/system/sockets.target.wants/dbus.socket",
        "var/log/audit",
        "usr/lib64/libexpat.so.1",
        "etc/pam.d/system-auth",
        "etc/pam.d/password-auth",
        "etc/pam.d/postlogin",
        "usr/local/bin/gnu-smoke",
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

    _require_preseeded_iso(iso, ctx.preseed_iso_path);
    _ensure_trusted_iso(BUILD_DIR, iso, ctx.sha256, ctx.trust_dir, ctx.preseed_iso_path);

    if !is_dir(join_path(iso_contents, "Packages")) && !is_dir(join_path(iso_contents, "os/Packages")) {
        log("Extracting Fedora ISO contents...");
        mkdir(iso_contents);
        shell("7z x -o" + iso_contents + " " + iso + " -y");
    }

    let packages_dir_ok = is_dir(join_path(iso_contents, "Packages")) || is_dir(join_path(iso_contents, "os/Packages"));
    let repodata_dir_ok = is_dir(join_path(iso_contents, "repodata")) || is_dir(join_path(iso_contents, "os/repodata"));
    if !packages_dir_ok || !repodata_dir_ok {
        throw "Fedora ISO package repositories missing after extraction (expected Packages/ and repodata/, or os/Packages and os/repodata under " + iso_contents + ")";
    }

    // Build Stage 01 source rootfs from ISO RPMs via recipe-native extraction.
    // No dnf/microdnf/yum in stage builds.
    shell("chmod -R u+rwX " + rootfs + " >/dev/null 2>&1 || true");
    shell("rm -rf " + rootfs);
    mkdir(rootfs);
    let packages = [
        // Root filesystem skeleton and core runtime.
        "filesystem",
        "setup",
        "bash",
        "bash-completion",
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
        "iputils",
        "curl",
        "libcurl",
        "libidn2",
        "libunistring",
        "libnghttp2",
        "libssh",
        "libpsl",
        "openldap",
        "libbrotli",
        "libevent",
        "cyrus-sasl-lib",
        "pciutils",
        "pciutils-libs",
        "usbutils",
        "libusb1",
        "less",
        "libfdisk",
        "readline",
        "kmod-libs",
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
        "dbus-common",
        "dbus-daemon",
        "dbus-broker",
        "expat",
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
        "audit",
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
            throw "required Stage 01 package missing from Fedora ISO repos: " + pkg;
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
        ln_force("bash", join_path(rootfs, "usr/bin/sh"));
    }
    if !exists(join_path(rootfs, "usr/sbin/agetty")) && exists(join_path(rootfs, "usr/bin/agetty")) {
        ln_force("../bin/agetty", join_path(rootfs, "usr/sbin/agetty"));
    }
    if !exists(join_path(rootfs, "usr/sbin/modprobe")) && exists(join_path(rootfs, "usr/bin/kmod")) {
        ln_force("../bin/kmod", join_path(rootfs, "usr/sbin/modprobe"));
    }
    if !exists(join_path(rootfs, "usr/bin/login")) && exists(join_path(rootfs, "usr/sbin/login")) {
        ln_force("../sbin/login", join_path(rootfs, "usr/bin/login"));
    }
    if !exists(join_path(rootfs, "usr/bin/sfdisk")) && exists(join_path(rootfs, "usr/sbin/sfdisk")) {
        ln_force("../sbin/sfdisk", join_path(rootfs, "usr/bin/sfdisk"));
    }
    if !exists(join_path(rootfs, "usr/bin/lspci")) && exists(join_path(rootfs, "usr/sbin/lspci")) {
        ln_force("../sbin/lspci", join_path(rootfs, "usr/bin/lspci"));
    }
    if !exists(join_path(rootfs, "usr/bin/lsusb")) && exists(join_path(rootfs, "usr/sbin/lsusb")) {
        ln_force("../sbin/lsusb", join_path(rootfs, "usr/bin/lsusb"));
    }
    // RPM scriptlets are not executed in this producer path. Ensure dbus broker
    // is reachable via dbus.service and dbus.socket is enabled for socket activation.
    mkdir(join_path(rootfs, "etc/systemd/system/sockets.target.wants"));
    ln_force("/usr/lib/systemd/system/dbus-broker.service", join_path(rootfs, "etc/systemd/system/dbus.service"));
    ln_force("/usr/lib/systemd/system/dbus.socket", join_path(rootfs, "etc/systemd/system/sockets.target.wants/dbus.socket"));

    // RPM scriptlets are not executed in this producer path. Ensure OpenSSH
    // privilege-separation account entries exist so sshd can start in Stage 01.
    let group_file = join_path(rootfs, "etc/group");
    let gshadow_file = join_path(rootfs, "etc/gshadow");
    let passwd_file = join_path(rootfs, "etc/passwd");
    let shadow_file = join_path(rootfs, "etc/shadow");
    shell("touch '" + group_file + "' '" + gshadow_file + "' '" + passwd_file + "' '" + shadow_file + "'");
    append_line_if_missing(group_file, "sshd:x:74:");
    append_line_if_missing(gshadow_file, "sshd:!::");
    append_line_if_missing(passwd_file, "sshd:x:74:74:Privilege-separated SSH:/run/sshd:/sbin/nologin");
    append_line_if_missing(shadow_file, "sshd:!:20000:0:99999:7:::");
    append_line_if_missing(group_file, "dbus:x:81:");
    append_line_if_missing(gshadow_file, "dbus:!::");
    append_line_if_missing(passwd_file, "dbus:x:81:81:System Message Bus:/:/sbin/nologin");
    append_line_if_missing(shadow_file, "dbus:!:20000:0:99999:7:::");

    // RPM scriptlets are not executed in this producer path, so PAM include
    // stack files normally materialized by auth tooling must be created here.
    let pam_dir = join_path(rootfs, "etc/pam.d");
    let factory_system_auth = join_path(rootfs, "usr/share/factory/etc/pam.d/system-auth");
    let system_auth = join_path(pam_dir, "system-auth");
    let password_auth = join_path(pam_dir, "password-auth");
    let postlogin = join_path(pam_dir, "postlogin");
    shell("mkdir -p '" + pam_dir + "'");
    if !exists(system_auth) && exists(factory_system_auth) {
        copy_into_dir(factory_system_auth, pam_dir);
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
    ln_force("usr/bin", join_path(rootfs, "bin"));
    ln_force("usr/sbin", join_path(rootfs, "sbin"));
    ln_force("usr/lib", join_path(rootfs, "lib"));
    ln_force("usr/lib64", join_path(rootfs, "lib64"));

    // Install a callable GNU userland smoke script for Levitate live/rootfs validation.
    shell("mkdir -p '" + join_path(rootfs, "usr/local/bin") + "'");
    let gnu_smoke = "#!/bin/sh\n";
    gnu_smoke += "set -u\n";
    gnu_smoke += "fails=0\n";
    gnu_smoke += "\n";
    gnu_smoke += "check_cmd() {\n";
    gnu_smoke += "    label=\"$1\"\n";
    gnu_smoke += "    pattern=\"$2\"\n";
    gnu_smoke += "    shift 2\n";
    gnu_smoke += "    cmd=\"$1\"\n";
    gnu_smoke += "    if ! command -v \"$cmd\" >/dev/null 2>&1; then\n";
    gnu_smoke += "        echo \"[FAIL] $label: missing command '$cmd'\"\n";
    gnu_smoke += "        fails=$((fails + 1))\n";
    gnu_smoke += "        return\n";
    gnu_smoke += "    fi\n";
    gnu_smoke += "    out=\"$($@ 2>&1 | head -n 2)\"\n";
    gnu_smoke += "    if printf '%s\\n' \"$out\" | grep -Eqi \"$pattern\"; then\n";
    gnu_smoke += "        echo \"[PASS] $label\"\n";
    gnu_smoke += "    else\n";
    gnu_smoke += "        echo \"[FAIL] $label: expected pattern '$pattern'\"\n";
    gnu_smoke += "        printf '%s\\n' \"$out\"\n";
    gnu_smoke += "        fails=$((fails + 1))\n";
    gnu_smoke += "    fi\n";
    gnu_smoke += "}\n";
    gnu_smoke += "\n";
    gnu_smoke += "check_cmd \"GNU Bash\" \"GNU bash\" bash --version\n";
    gnu_smoke += "check_cmd \"GNU Coreutils\" \"GNU coreutils\" ls --version\n";
    gnu_smoke += "check_cmd \"GNU Grep\" \"GNU grep\" grep --version\n";
    gnu_smoke += "check_cmd \"GNU Sed\" \"GNU sed\" sed --version\n";
    gnu_smoke += "check_cmd \"GNU Findutils\" \"GNU findutils\" find --version\n";
    gnu_smoke += "check_cmd \"GNU Awk\" \"GNU Awk\" awk --version\n";
    gnu_smoke += "check_cmd \"glibc runtime\" \"GNU C Library|glibc\" ldd --version\n";
    gnu_smoke += "\n";
    gnu_smoke += "if [ \"$fails\" -eq 0 ]; then\n";
    gnu_smoke += "    echo \"GNU smoke PASS\"\n";
    gnu_smoke += "    exit 0\n";
    gnu_smoke += "fi\n";
    gnu_smoke += "echo \"GNU smoke FAIL ($fails checks)\"\n";
    gnu_smoke += "exit 1\n";
    write_file(join_path(rootfs, "usr/local/bin/gnu-smoke"), gnu_smoke);
    shell("chmod 0755 '" + join_path(rootfs, "usr/local/bin/gnu-smoke") + "'");

    // Keep rootfs recipe outputs deterministic and lean.
    shell("rm -rf '" + join_path(rootfs, "var/cache") + "' '" + join_path(rootfs, "var/log") + "'");
    shell("mkdir -p '" + join_path(rootfs, "var/cache") + "' '" + join_path(rootfs, "var/log") + "' '" + join_path(rootfs, "var/log/audit") + "'");
    shell("chmod 0700 '" + join_path(rootfs, "var/log/audit") + "' >/dev/null 2>&1 || true");
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
