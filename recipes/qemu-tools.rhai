// Pinned standalone QEMU/OVMF producer; host scenarios consume its named
// published tree after explicit `just tools-install` preparation.
// SHA256 facts: Rocky10.1 vault BaseOS/AppStream repodata primary XML.
const RECIPE_API = 2;
let ctx = #{
    name: "qemu-tools",
    version: "10.0.0",
    description: "QEMU and OVMF for boot scenarios",
    tools: RUNTIME.paths.output,
    rpms: [
        // ELF runtime dependencies absent from the compiler/base image.
        ["https://dl.rockylinux.org/vault/rocky/10.1/BaseOS/x86_64/os/Packages/l/libaio-0.3.111-22.el10.x86_64.rpm", "libaio.rpm", "9d6afa18f923b6020b1dde5333184002f15a1c63fe3ad4e13792aca69d6bf477"],
        ["https://dl.rockylinux.org/vault/rocky/10.1/BaseOS/x86_64/os/Packages/l/libpng-1.6.40-8.el10_1.4.x86_64.rpm", "libpng.rpm", "dbbedb2df5ca13157926de2268ad36376939f15819e9df4585db1304d26903c0"],
        ["https://dl.rockylinux.org/vault/rocky/10.1/BaseOS/x86_64/os/Packages/l/libseccomp-2.5.6-1.el10.x86_64.rpm", "libseccomp.rpm", "ee91ee0f4ad9b2b0deac8b215b2e9b335edfc43f6fc5cd90817f7dc4f3f1ccce"],
        ["https://dl.rockylinux.org/vault/rocky/10.1/BaseOS/x86_64/os/Packages/l/liburing-2.5-5.el10.x86_64.rpm", "liburing.rpm", "5b1aa3dfd643836c950be5984444c8d741403259a352e757a725f8cb5dd54cfb"],
        ["https://dl.rockylinux.org/vault/rocky/10.1/BaseOS/x86_64/os/Packages/l/lzo-2.10-14.el10.x86_64.rpm", "lzo.rpm", "c216409af7f24d009b41073e2fbd54d803ba4c63898e98b0eb782f4f355ebcf5"],
        ["https://dl.rockylinux.org/vault/rocky/10.1/BaseOS/x86_64/os/Packages/n/numactl-libs-2.0.19-2.el10.x86_64.rpm", "numactl-libs.rpm", "4ae515aeb4b75bc0bdcf260414555d76e830bbec4334a84ab47f633d88d3cd7e"],
        ["https://dl.rockylinux.org/vault/rocky/10.1/BaseOS/x86_64/os/Packages/s/snappy-1.1.10-7.el10.x86_64.rpm", "snappy.rpm", "8730234ef6ef6dd14bb5a05e4c8178293a1ae6c533420a81072fda4dd692e391"],
        // QEMU, firmware, and libraries provided by AppStream.
        ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/q/qemu-img-10.0.0-14.el10_1.x86_64.rpm", "qemu-img.rpm", "131e20dbdc855fec860fee6943c02a20c74abe406522b9064b1e8c319b7bd068"],
    ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/q/qemu-kvm-core-10.0.0-14.el10_1.x86_64.rpm", "qemu-kvm-core.rpm", "36cd677807ab9f7aa6122b76220244ad939db7cf3c324c78e916121231986af4"],
    ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/q/qemu-kvm-common-10.0.0-14.el10_1.x86_64.rpm", "qemu-kvm-common.rpm", "f9d599b8b87ace482bb4724d8ab2ae75e5bfaee7d4eba5440bdcf688e4865595"],
    ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/e/edk2-ovmf-20250523-2.el10.noarch.rpm", "edk2-ovmf.rpm", "7888d70358b76f7740a3c64c63d5ef61293d443e2d8ddc0c49ca1988c26d8726"],
    ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/c/capstone-5.0.1-6.el10.x86_64.rpm", "capstone.rpm", "34f7154e9075eab0d07f1ae6bf43d8bbec8152fd2bb4e892476c06c79a1607c3"],
    ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/p/pixman-0.43.4-2.el10.x86_64.rpm", "pixman.rpm", "6007a7578cab13b204a142d38fd429e4961d7c841594b1f2f3439d0bd62d118d"],
    ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/l/libslirp-4.7.0-10.el10.x86_64.rpm", "libslirp.rpm", "67030e1b8afd89159dbdb7e8a4adcdab886c23c96e1479416890223d47dae2c8"],
    ["https://dl.rockylinux.org/vault/rocky/10.1/AppStream/x86_64/os/Packages/i/ipxe-roms-qemu-20240119-5.gitde8a0821.el10.noarch.rpm", "ipxe-roms-qemu.rpm", "c292eb13a545e3ee8d3764a7cc80491c62d561ce3305d4546ddc6e6ec9ae27f3"],
    ],
};

fn is_acquired(ctx) {
    for rpm in ctx.rpms {
        let archive = join_path(ctx.runtime.paths.build, rpm[1]);
        if !is_file(archive) { return needs_work(ctx, rpm[1] + " is missing"); }
        verify_sha256(archive, rpm[2]);
    }
    ready(ctx)
}

fn acquire(ctx) {
    let work = ctx.runtime.paths.build;
    mkdir(work);
    for rpm in ctx.rpms {
        let archive = join_path(work, rpm[1]);
        if !is_file(archive) || ctx.runtime.options.force_acquire {
            download(rpm[0], archive, rpm[2]);
        } else { verify_sha256(archive, rpm[2]); }
    }
    ctx
}

fn is_installed(ctx) {
    for path in ["usr/bin/qemu-img", "usr/libexec/qemu-kvm",
                 "usr/share/edk2/ovmf/OVMF_CODE.fd", "usr/share/edk2/ovmf/OVMF_VARS.fd"] {
        if !is_file(join_path(ctx.tools, path)) { return needs_work(ctx, path + " is missing"); }
    }
    let libraries = join_path(ctx.tools, "usr/lib64") + ":/usr/lib:/usr/lib64";
    let options = #{env: #{LD_LIBRARY_PATH: libraries}};
    let expected = "version " + ctx.version;
    for program in ["usr/bin/qemu-img", "usr/libexec/qemu-kvm"] {
        let executable = join_path(ctx.tools, program);
        let version = exec_output(executable, ["--version"], options);
        if !version.contains(expected) {
            throw "Unexpected QEMU executable version: " + version;
        }
    }
    ctx.outputs.tools = ctx.tools;
    ready(ctx)
}

fn install(ctx) {
    for rpm in ctx.rpms {
        let archive = join_path(ctx.runtime.paths.build, rpm[1]);
        verify_sha256(archive, rpm[2]);
        let cpio = archive + ".cpio";
        // The producer and extractor must each succeed; no swallowed pipe error.
        exec_checked("sh", ["-c", "rpm2cpio \"$1\" > \"$2\"", "rpm2cpio", archive, cpio]);
        // Boot scenarios use user networking, not the privileged host-bridge
        // helper. Select that package subset explicitly; extraction still fails
        // on every error in the retained payload.
        exec_checked("sh", ["-c", "cpio -idmu --quiet --nonmatching ./usr/libexec/qemu-bridge-helper < \"$1\"", "cpio", cpio], #{cwd: ctx.tools});
        rm(cpio);
    }
    let bin = join_path(ctx.tools, "usr/bin");
    mkdir(bin);
    ln_force("../libexec/qemu-kvm", join_path(bin, "qemu-system-x86_64"));
    let data = join_path(ctx.tools, "usr/share/qemu-kvm");
    let ipxe = join_path(ctx.tools, "usr/share/ipxe/qemu");
    if is_dir(ipxe) {
        mkdir(data);
        for rom in glob_list(join_path(ipxe, "*.rom")) {
            ln_force("../ipxe/qemu/" + basename(rom), join_path(data, basename(rom)));
        }
    }
    ctx.outputs.tools = ctx.tools;
    ctx
}

fn remove(ctx) {
    let usr = join_path(ctx.tools, "usr");
    if is_dir(usr) { rm(usr); }
    ctx.outputs = #{};
    ctx
}

fn cleanup(ctx, reason) {
    if reason == "manual" {
        for rpm in ctx.rpms {
            let archive = join_path(ctx.runtime.paths.build, rpm[1]);
            if is_file(archive) { rm(archive); }
        }
    }
    ctx
}
