// QEMU + OVMF testing dependencies
//
// Provides: qemu-system-x86_64, qemu-img, OVMF firmware
// Downloaded as pre-built RPMs from Rocky mirrors and extracted.
// These are testing/development tools, not build dependencies.
//
// Required shared libraries (glib, gnutls, pixman, etc.) are expected
// to already be on the host system from the Rocky base install.

let base_url = "http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages";

let ctx = #{
    description: "QEMU and OVMF for boot testing",
    name: "qemu-deps",
    rpms: "[[\"http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages/q/qemu-kvm-core-10.0.0-14.el10_1.x86_64.rpm\", \"qemu-kvm-core.rpm\"], [\"http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages/q/qemu-kvm-common-10.0.0-14.el10_1.x86_64.rpm\", \"qemu-kvm-common.rpm\"], [\"http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages/e/edk2-ovmf-20250523-2.el10.noarch.rpm\", \"edk2-ovmf.rpm\"], [\"http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages/c/capstone-5.0.1-6.el10.x86_64.rpm\", \"capstone.rpm\"], [\"http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages/p/pixman-0.43.4-2.el10.x86_64.rpm\", \"pixman.rpm\"], [\"http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages/l/libslirp-4.7.0-10.el10.x86_64.rpm\", \"libslirp.rpm\"], [\"http://rockylinux.mirror.intermax.nl/10.1/AppStream/x86_64/os/Packages/i/ipxe-roms-qemu-20240119-5.gitde8a0821.el10.noarch.rpm\", \"ipxe-roms-qemu.rpm\"]]",
};

fn is_installed(ctx) {
    let bin = join_path(TOOLS_PREFIX, "usr/bin");
    let libexec = join_path(TOOLS_PREFIX, "usr/libexec");
    if !is_file(join_path(bin, "qemu-img")) {
        if !is_file(join_path(libexec, "qemu-kvm")) {
            throw "qemu not installed";
        }
    }
    // Check OVMF
    let ovmf = join_path(TOOLS_PREFIX, "usr/share/edk2/ovmf/OVMF_CODE.fd");
    if !is_file(ovmf) { throw "OVMF not installed"; }
    ctx
}

// === CLEANUP ===
fn cleanup(ctx, reason) { ctx }

fn is_acquired(ctx) {
    for rpm in ctx.rpms {
        if !is_file(join_path(BUILD_DIR, rpm[1])) {
            throw rpm[1] + " not downloaded";
        }
    }
    ctx
}

fn acquire(ctx) {
    mkdir(BUILD_DIR);
    for rpm in ctx.rpms {
        let dest = join_path(BUILD_DIR, rpm[1]);
        if !is_file(dest) {
            log("Downloading " + rpm[1] + "...");
            download(rpm[0], dest);
        }
    }
    ctx
}

fn install(ctx) {
    mkdir(TOOLS_PREFIX);
    for rpm in ctx.rpms {
        let rpm_path = join_path(BUILD_DIR, rpm[1]);
        log("Extracting " + rpm[1] + "...");
        shell_in(TOOLS_PREFIX, "rpm2cpio '" + rpm_path + "' | cpio -idmu --quiet 2>/dev/null");
    }

    // Create convenience symlink: qemu-system-x86_64 -> qemu-kvm
    let bin_dir = join_path(TOOLS_PREFIX, "usr/bin");
    let libexec = join_path(TOOLS_PREFIX, "usr/libexec");
    mkdir(bin_dir);
    if is_file(join_path(libexec, "qemu-kvm")) {
        if !exists(join_path(bin_dir, "qemu-system-x86_64")) {
            ln(join_path(libexec, "qemu-kvm"), join_path(bin_dir, "qemu-system-x86_64"));
        }
    }

    // Symlink iPXE ROMs into QEMU data dir so QEMU can find them
    let qemu_data = join_path(TOOLS_PREFIX, "usr/share/qemu-kvm");
    let ipxe_dir = join_path(TOOLS_PREFIX, "usr/share/ipxe/qemu");
    if is_dir(ipxe_dir) {
        shell("ln -sf " + ipxe_dir + "/*.rom " + qemu_data + "/");
    }

    ctx
}
