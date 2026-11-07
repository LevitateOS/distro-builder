// Materialize the rootfs from the locked Fedora RPM set, imported as `rpms`.
// The RPMs are unpacked without a package manager; their install scripts do
// not run, so the steps they would take are done below.
const RECIPE_API = 2;
import "sources/levitate-rpm-lock.rhai" as lock;
import "installed-system.rhai" as installed;
let ctx = #{
    name: "fedora-rpm-source-rootfs",
    description: "Fedora " + lock::release() + " rootfs source from the locked RPM set",
    rpms_path: join_path(RUNTIME.paths.inputs, "rpms"),
    rootfs_path: join_path(RUNTIME.paths.output, "rootfs"),
    packages: lock::packages(),
};

// The rootfs records the exact builds it was made from; a new lock rebuilds it.
fn _rootfs_marker(ctx) {
    let marker = "fedora-rpm-set-v3-target-metadata";
    for rpm in ctx.packages { marker += "\n" + rpm[1]; }
    marker
}

fn _verify_rpms(ctx) {
    for rpm in ctx.packages {
        let file = join_path(ctx.rpms_path, basename(rpm[2]));
        if !is_file(file) { throw "the rpms input lacks " + basename(file) + "; run the rootfs source preseed"; }
        verify_sha256(file, rpm[3]);
    }
}

fn _extract_rpm_into_rootfs(rootfs, rpm_path) {
    target_extract_rpm(rpm_path, rootfs);
}

fn is_acquired(ctx) {
    _verify_rpms(ctx);
    ready(ctx)
}

fn acquire(ctx) {
    _verify_rpms(ctx);
    ctx
}

fn is_built(ctx) {
    let rootfs = ctx.rootfs_path;
    if !is_dir(join_path(rootfs, "usr")) {
        return needs_work(ctx, "rootfs not built");
    }
    let marker_path = join_path(rootfs, ".source-rootfs.recipe-version");
    let marker = trim(read_file_or_empty(marker_path));
    if marker != _rootfs_marker(ctx) {
        return needs_work(ctx, "rootfs was built from a different RPM lock");
    }
    let required_paths = [
        "usr/lib/systemd/systemd",
        "usr/sbin/sshd",
        "usr/sbin/auditd",
        "usr/bin/sudo",
        "etc/sudoers.d/10-wheel",
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
            return needs_work(ctx, "minimal source rootfs incomplete: missing " + rel);
        }
    }

    ready(ctx)
}

fn build(ctx) {
    let rootfs = ctx.rootfs_path;
    _verify_rpms(ctx);

    if is_dir(rootfs) { shell("chmod -R u+rwX " + rootfs); }
    shell("rm -rf " + rootfs);
    mkdir(rootfs);
    log("Extracting the locked Fedora " + lock::release() + " RPM set into the rootfs...");
    for rpm in ctx.packages {
        log("  Extracting " + rpm[1]);
        let path = join_path(ctx.rpms_path, basename(rpm[2]));
        // The helper already grants staging access independently of target
        // modes. Do not rescan/chmod the whole growing tree after each RPM.
        _extract_rpm_into_rootfs(rootfs, path);
    }

    // Fedora merged /usr/sbin into /usr/bin: `filesystem` lists /usr/sbin as a
    // ghost that its install script makes a link to bin.
    if !exists(join_path(rootfs, "usr/sbin")) {
        ln_force("bin", join_path(rootfs, "usr/sbin"));
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

    // Levitate uses password-authenticated sudo for wheel administrators.
    mkdir(join_path(rootfs, "etc/sudoers.d"));
    let wheel_sudoers = join_path(rootfs, "etc/sudoers.d/10-wheel");
    write_file(wheel_sudoers, "%wheel ALL=(ALL:ALL) ALL\n");
    target_mode(rootfs, "etc/sudoers.d/10-wheel", 0o440);

    // RPM scriptlets are not executed in this producer path. Ensure OpenSSH
    // Privilege-separation account entries must exist so sshd can start in live boot.
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

    // Enforce the merged-usr symlink contract expected by live boot.
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
    target_mode(rootfs, "var/log/audit", 0o700);
    installed::systemd(rootfs);
    target_finalize(rootfs);
    write_file(join_path(rootfs, ".source-rootfs.recipe-version"), _rootfs_marker(ctx) + "\n");
    ctx
}

fn is_installed(ctx) {
    let status = is_built(ctx);
    ctx = status.ctx;
    if status.status != "ready" { return status; }
    ctx.outputs = #{rootfs: ctx.rootfs_path};
    ready(ctx)
}

fn install(ctx) {
    let status = is_built(ctx);
    if status.status != "ready" { throw status.reason; }
    ctx = status.ctx;
    ctx.outputs = #{rootfs: ctx.rootfs_path};
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" {
        let marker = join_path(ctx.rootfs_path, ".source-rootfs.recipe-version");
        if is_file(marker) {
            rm(marker);
        }
    }
    ctx
}

fn remove(ctx) {
    if is_dir(ctx.rootfs_path) { rm(ctx.rootfs_path); }
    ctx.outputs = #{};
    ctx
}
