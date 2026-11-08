use std::path::Path;

use anyhow::{bail, Context, Result};
use distro_builder::kernel::{KERNEL_IMAGE_PATH, KERNEL_RELEASE_PATH};
use distro_builder::process::Cmd;
use distro_contract::Variant;

use distro_builder::product::ReleaseProduct;

use distro_contract::runs::Run;

/// The default live UKI's extra kernel command line (its emergency and debug
/// siblings boot in text mode).
const LIVE_UKI_CMDLINE: &str = "video=1920x1080";

pub(crate) fn ensure_release_iso_via_variant_hook(
    bundle: &Variant,
    distro_id: &str,
    kernel_output_dir: &Path,
    run: &Run,
    release: ReleaseProduct,
) -> Result<()> {
    let product = release.product;
    let build = distro_builder::product::build(product);
    let artifacts = &bundle.contract.artifacts;
    let output_dir = &run.dir;
    let iso_filename = artifacts.iso(product);
    let iso_path = output_dir.join(&iso_filename);
    let native_build = bundle.path("hooks").join(release.hook);
    if !native_build.is_file() {
        bail!(
            "missing variant release build hook for product '{product}' on '{distro_id}': {}",
            native_build.display(),
        );
    }
    let required_kernel_cmdline = if build.live_session {
        bundle.contract.live_kernel_cmdline().join(" ")
    } else {
        String::new()
    };

    let kernel_release_path = kernel_output_dir.join(KERNEL_RELEASE_PATH);
    let kernel_image_path = kernel_output_dir.join(KERNEL_IMAGE_PATH);

    println!(
        "[release:iso:{product}:{distro_id}] building release run {} via variant release hook {} (output: {})",
        run.manifest.run_id,
        native_build.display(),
        iso_path.display()
    );

    let distro_builder_bin =
        std::env::current_exe().context("resolving distro-builder executable path")?;

    Cmd::new("sh")
        .arg(&native_build)
        .dir(&bundle.repo_root)
        .env("REPO_ROOT", &bundle.repo_root)
        .env("DISTRO_ID", distro_id)
        .env("IDENTITY_OS_NAME", &bundle.contract.identity.os_name)
        .env("IDENTITY_OS_ID", &bundle.contract.identity.os_id)
        .env("IDENTITY_OS_VERSION", &bundle.contract.identity.os_version)
        .env("IDENTITY_ISO_LABEL", &bundle.contract.identity.iso_label)
        .env("LIVE_UKI_FILENAME", &artifacts.live_uki.live)
        .env("EMERGENCY_UKI_FILENAME", &artifacts.live_uki.emergency)
        .env("DEBUG_UKI_FILENAME", &artifacts.live_uki.debug)
        .env("LIVE_UKI_CMDLINE", LIVE_UKI_CMDLINE)
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .env("ISO_FILENAME", &iso_filename)
        .env("PRODUCT_NAME", product.name())
        .env("PRODUCT_DIRNAME", product.name())
        .env("PRODUCT_ARTIFACT_TAG", product.name())
        .env("PRODUCT_BOOT_LABEL", product.title())
        .env("ROOTFS_FILENAME", &artifacts.rootfs)
        .env("INITRAMFS_LIVE_FILENAME", &artifacts.initramfs_live)
        .env(
            "INITRAMFS_INSTALLED_FILENAME",
            artifacts.initramfs_installed.as_deref().unwrap_or(""),
        )
        .env("LIVE_OVERLAY_DIRNAME", build.overlay_dir)
        .env("LIVE_OVERLAY_IMAGE_FILENAME", &artifacts.overlay)
        .env("ROOTFS_SOURCE_POINTER_FILENAME", build.rootfs_source_pointer)
        .env("RELEASE_OUTPUT_DIR", output_dir)
        .env("DISTRO_BUILDER_BIN", &distro_builder_bin)
        .env("KERNEL_OUTPUT_DIR", kernel_output_dir)
        .env("PRODUCT_REQUIRED_KERNEL_CMDLINE", &required_kernel_cmdline)
        .error_msg(format!(
            "variant release build hook '{}' for product '{product}' on '{distro_id}'",
            native_build.display(),
        ))
        .run_interactive()?;

    if std::fs::metadata(&iso_path).map_or(true, |m| !m.is_file() || m.len() == 0) {
        bail!(
            "builder finished but the ISO for '{}' is missing or empty: {}",
            distro_id,
            iso_path.display()
        );
    }

    Ok(())
}
