// Materialize one rootfs from explicit immutable iso/apk_tools inputs.
const RECIPE_API = 2;
import "sources/alpine.rhai" as source;
import "installed-system.rhai" as installed;
let assets = source::facts();
let ctx = #{
    name: "alpine-live-source-rootfs",
    description: "Alpine live rootfs source producer",
    alpine_version: assets.iso_version,
    apk_static_path: "",
    apk_tools_path: join_path(RUNTIME.paths.inputs, "apk_tools"),
    apk_tools_sha256: assets.apk_tools_sha256,
    apk_tools_version: assets.apk_tools_version,
    iso_path: join_path(RUNTIME.paths.inputs, "iso"),
    extra_apks_path: join_path(RUNTIME.paths.inputs, "extra_apks"),
    extra_apks: assets.extra_apks,
    iso_sha256: assets.iso_sha256,
    iso_name: assets.iso_name,
    apk_tools_name: assets.apk_tools_name,
    profile: "acorn",
    release_branch: assets.release_branch,
    rootfs_path: join_path(RUNTIME.paths.output, "rootfs"),
    // A new Alpine pin rebuilds the rootfs.
    rootfs_recipe_version: "alpine-source-v8-installed-" + assets.iso_version,
};

fn _verify_inputs(ctx) {
    if !is_file(ctx.iso_path) || !is_file(ctx.apk_tools_path) {
        throw "Import the named iso and apk_tools outputs from explicit preseed acquisition";
    }
    verify_sha256(ctx.iso_path, ctx.iso_sha256);
    verify_sha256(ctx.apk_tools_path, ctx.apk_tools_sha256);
    for apk in ctx.extra_apks {
        let file = join_path(ctx.extra_apks_path, apk[0]);
        if !is_file(file) { throw "the extra_apks input lacks " + apk[0] + "; run the rootfs source preseed"; }
        verify_sha256(file, apk[1]);
    }
}

// The pinned packages the ISO lacks, as quoted file arguments to `apk add`.
fn _extra_apk_files(ctx) {
    let files = "";
    for apk in ctx.extra_apks {
        files += " '" + join_path(ctx.extra_apks_path, apk[0]) + "'";
    }
    files
}

fn _profile_packages(profile) {
    let normalized = trim(profile).to_lower();
    if normalized == "acorn" {
        // From the ISO. The rest (findutils, grep, less, musl-locales,
        // openrc-init, systemd-boot, hdparm, smartmontools, vim) are the
        // pinned extra_apks files, which `apk add` takes alongside these names.
        return "alpine-base busybox openrc doas " +
               "openssh openssh-server util-linux e2fsprogs dosfstools " +
               "iproute2 iputils dhcpcd curl pciutils htop";
    }

    throw "Unsupported Alpine live source profile '" + profile + "'. Use profile 'acorn'.";
}

fn _network_interfaces() {
    "# Installed loopback; dhcpcd discovers wired interfaces.\n" +
    "auto lo\niface lo inet loopback\n"
}

fn _rootfs_marker(rootfs) {
    join_path(rootfs, ".source-rootfs.recipe-version")
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
    if !is_file(join_path(rootfs, "usr/bin/doas")) {
        return false;
    }
    let doas_conf = join_path(rootfs, "etc/doas.conf");
    if !is_file(doas_conf) || trim(read_file(doas_conf)) != "permit persist :wheel" {
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

    // The source rootfs is copied via rsync; source files must be owner-readable.
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

fn _ensure_openrc_live_source_completeness(rootfs) {
    // Acorn uses password-authenticated doas for wheel administrators.
    let doas_conf = join_path(rootfs, "etc/doas.conf");
    write_file(doas_conf, "permit persist :wheel\n");
    chmod(doas_conf, 0o600);
    mkdir(join_path(rootfs, "usr/share/empty.sshd"));
    shell("chmod 755 '" + join_path(rootfs, "usr/share/empty.sshd") + "'");
    write_file(join_path(rootfs, "etc/locale.conf"), "LANG=C.UTF-8\n");
    mkdir(join_path(rootfs, "etc/network"));
    write_file(join_path(rootfs, "etc/network/interfaces"), _network_interfaces());
    mkdir(join_path(rootfs, "etc/conf.d"));
    write_file(join_path(rootfs, "etc/conf.d/dhcpcd"), "dhcpcd_args=\"--quiet\"\n");
}

fn is_acquired(ctx) {
    _verify_inputs(ctx);
    let tools = join_path(ctx.runtime.paths.build, "apk-tools");
    let apk_static = _resolve_apk_static(tools);
    if apk_static == "" { return needs_work(ctx, "apk.static has not been extracted"); }
    ctx.apk_static_path = apk_static;
    ready(ctx)
}

fn acquire(ctx) {
    _verify_inputs(ctx);
    let tools = join_path(ctx.runtime.paths.build, "apk-tools");
    mkdir(tools);
    ctx.apk_static_path = _extract_apk_tools_static(ctx.apk_tools_path, tools);
    ctx
}

fn is_built(ctx) {
    let rootfs = ctx.rootfs_path;
    if !_rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        return needs_work(ctx, "rootfs not built");
    }
    ctx.rootfs_path = rootfs;
    ready(ctx)
}

fn build(ctx) {
    let iso_contents = join_path(ctx.runtime.paths.build, "iso-contents");
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

    let rootfs = ctx.rootfs_path;
    if is_dir(rootfs) {
        // Normalize readability on cached rootfs before validity checks.
        _normalize_rootfs_permissions(rootfs, "cached rootfs preflight");
    }
    if _rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        log("Alpine live source rootfs already current; skipping rebuild");
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
    log("Installing Alpine live source package baseline (" + ctx.profile + ")...");
    let apk_cmd = "'" + ctx.apk_static_path + "' --root '" + rootfs + "' " +
        "--usermode --initdb --no-progress --allow-untrusted add " + packages +
        _extra_apk_files(ctx);
    shell(apk_cmd);
    _normalize_rootfs_permissions(rootfs, "post-apk install");
    _ensure_openrc_live_source_completeness(rootfs);

    if !is_file(join_path(rootfs, "bin/busybox")) {
        throw "invalid rootfs: missing /bin/busybox after apk install";
    }
    let sh_path = join_path(rootfs, "bin/sh");
    if shell_status("[ -L '" + sh_path + "' ] || [ -f '" + sh_path + "' ]") != 0 {
        throw "invalid rootfs: missing /bin/sh path after apk install";
    }

    installed::openrc(rootfs);
    write_file(_rootfs_marker(rootfs), ctx.rootfs_recipe_version + "\n");
    ctx.rootfs_path = rootfs;
    ctx
}

fn is_installed(ctx) {
    let rootfs = ctx.rootfs_path;
    if !_rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        return needs_work(ctx, "rootfs not installed");
    }
    ctx.rootfs_path = rootfs;
    ctx.outputs = #{rootfs: ctx.rootfs_path};
    ready(ctx)
}

fn install(ctx) {
    let rootfs = ctx.rootfs_path;
    if !_rootfs_valid(rootfs, ctx.rootfs_recipe_version) {
        throw "cannot install: rootfs missing or stale at " + rootfs;
    }
    ctx.rootfs_path = rootfs;
    ctx.outputs = #{rootfs: ctx.rootfs_path};
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" {
        let rootfs = ctx.rootfs_path;
        let marker = _rootfs_marker(rootfs);
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
