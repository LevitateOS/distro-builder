use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use distro_contract::LoadedVariantContract;

use crate::{BuildOutputLayout, BuildProduct};

pub(crate) fn ensure_release_iso_via_variant_hook(
    bundle: &LoadedVariantContract,
    distro_id: &str,
    kernel_output_dir: &Path,
    build_layout: &BuildOutputLayout,
    product: BuildProduct,
) -> Result<()> {
    let output_dir = &build_layout.output_dir;
    let live_uki = &bundle.contract.transforms.live_uki;
    let [live_uki_filename, emergency_uki_filename, debug_uki_filename] =
        live_uki.output_names.as_slice()
    else {
        bail!(
            "invalid canonical Ring 1 live UKI transform for '{}': expected exactly three output names in `contract.transforms.live_uki.output_names`, found {:?}",
            distro_id,
            live_uki.output_names
        );
    };
    let live_cmdline = live_uki.extra_cmdline.clone().unwrap_or_default();
    let initramfs_live_filename = crate::workflows::canonical_initramfs_live_filename(
        &bundle.contract,
    )
    .with_context(|| {
        format!(
            "resolving canonical Ring 1 live initramfs output for '{}'",
            distro_id
        )
    })?;
    let rootfs_filename = crate::workflows::canonical_rootfs_erofs_filename(&bundle.contract)
        .with_context(|| {
            format!(
                "resolving canonical Ring 1 rootfs output for '{}'",
                distro_id
            )
        })?;
    let overlay_filename = crate::workflows::canonical_overlay_erofs_filename(&bundle.contract)
        .with_context(|| {
            format!(
                "resolving canonical Ring 1 overlay output for '{}'",
                distro_id
            )
        })?;
    let base_iso_filename = crate::workflows::canonical_iso_filename(&bundle.contract)
        .with_context(|| format!("resolving canonical Ring 0 ISO output for '{}'", distro_id))?;
    let iso_filename =
        crate::workflows::build::iso_filename_for_product(&base_iso_filename, product);
    let iso_path = output_dir.join(&iso_filename);
    let release_hook_script = product.release_hook_script_name.ok_or_else(|| {
        anyhow::anyhow!(
            "missing canonical release hook for product '{}': this product is not a release ISO target",
            product.canonical
        )
    })?;
    let native_build = bundle.paths.ring0_hook_path(release_hook_script);
    if !native_build.is_file() {
        bail!(
            "missing variant release build hook for product '{}' on '{}': {}\n\
             expected '{}' under '{}'.",
            product.canonical,
            distro_id,
            native_build.display(),
            release_hook_script,
            bundle.paths.ring0_hooks_dir.display()
        );
    }

    let kernel_release_path = kernel_output_dir.join(&bundle.contract.build.kernel.release_path);
    let kernel_image_path = kernel_output_dir.join(&bundle.contract.build.kernel.image_path);

    println!(
        "[release:iso:{}:{distro_id}] building release run {} via variant release hook {} (output: {})",
        product.canonical,
        build_layout.run_id.as_deref().unwrap_or("adhoc"),
        native_build.display(),
        iso_path.display()
    );

    let distro_builder_bin =
        std::env::current_exe().context("resolving distro-builder executable path")?;

    let status = Command::new("sh")
        .arg(&native_build)
        .current_dir(&bundle.repo_root)
        .env("DISTRO_ID", distro_id)
        .env("IDENTITY_OS_NAME", &bundle.contract.identity.os_name)
        .env("IDENTITY_OS_ID", &bundle.contract.identity.os_id)
        .env("IDENTITY_OS_VERSION", &bundle.contract.identity.os_version)
        .env("IDENTITY_ISO_LABEL", &bundle.contract.identity.iso_label)
        .env("LIVE_UKI_FILENAME", live_uki_filename)
        .env("EMERGENCY_UKI_FILENAME", emergency_uki_filename)
        .env("DEBUG_UKI_FILENAME", debug_uki_filename)
        .env("LIVE_UKI_CMDLINE", &live_cmdline)
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .env("ISO_FILENAME", &iso_filename)
        .env("PRODUCT_NAME", product.canonical)
        .env("PRODUCT_DIRNAME", product.release_dir_name)
        .env("PRODUCT_ARTIFACT_TAG", product.release_dir_name)
        .env("PRODUCT_BOOT_LABEL", product.issue_banner_label)
        .env("ROOTFS_FILENAME", &rootfs_filename)
        .env("INITRAMFS_LIVE_FILENAME", &initramfs_live_filename)
        .env("LIVE_OVERLAY_DIRNAME", product.live_overlay_dir_name)
        .env("LIVE_OVERLAY_IMAGE_FILENAME", &overlay_filename)
        .env(
            "ROOTFS_SOURCE_POINTER_FILENAME",
            product.rootfs_source_pointer_filename,
        )
        .env("RELEASE_ROOT_DIR", &build_layout.root_dir)
        .env("RELEASE_RUN_DIR", output_dir)
        .env("RELEASE_OUTPUT_DIR", output_dir)
        .env("BUILD_RUN_ID", build_layout.run_id.as_deref().unwrap_or(""))
        .env("DISTRO_BUILDER_BIN", &distro_builder_bin)
        .env("KERNEL_OUTPUT_DIR", kernel_output_dir)
        .env(
            "PRODUCT_REQUIRED_KERNEL_CMDLINE",
            product_required_kernel_cmdline(bundle, product),
        )
        .status()
        .with_context(|| {
            format!(
                "running variant release build hook '{}' for product '{}' on '{}'",
                native_build.display(),
                product.canonical,
                distro_id
            )
        })?;

    if !status.success() {
        bail!("builder command failed for '{distro_id}' with status {status}");
    }

    if !iso_path.is_file() {
        bail!(
            "builder finished but ISO still missing for '{}': {}",
            distro_id,
            iso_path.display()
        );
    }

    Ok(())
}

fn product_required_kernel_cmdline(
    bundle: &LoadedVariantContract,
    product: BuildProduct,
) -> String {
    match product.canonical {
        crate::PRODUCT_LIVE_BOOT | crate::PRODUCT_LIVE_TOOLS => bundle
            .contract
            .scenarios
            .live_boot
            .required_kernel_cmdline
            .join(" "),
        _ => String::new(),
    }
}
