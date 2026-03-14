use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use distro_contract::LoadedVariantContract;

use crate::{BuildOutputLayout, BuildProduct, CompatibilityBuildStage};

pub(crate) fn ensure_release_iso_via_compatibility_hook(
    bundle: &LoadedVariantContract,
    distro_id: &str,
    kernel_output_dir: &Path,
    build_layout: &BuildOutputLayout,
    product: BuildProduct,
) -> Result<()> {
    let compat_stage = crate::workflows::compatibility_stage_for_product(product);
    let output_dir = &build_layout.output_dir;
    let iso_filename = crate::workflows::build::iso_filename_for_product(
        &bundle.contract.artifacts.iso_filename,
        product,
    );
    let iso_path = output_dir.join(&iso_filename);
    let native_build = bundle.variant_dir.join(compat_stage.native_build_script);
    if !native_build.is_file() {
        bail!(
            "missing compatibility build hook for release product '{}' on '{}': {}\n\
             expected '{}' under '{}'.",
            product.canonical,
            distro_id,
            native_build.display(),
            compat_stage.native_build_script,
            bundle.variant_dir.display()
        );
    }

    let kernel_release_path =
        kernel_output_dir.join(&bundle.contract.stages.stage_00_build.kernel_release_path);
    let kernel_image_path =
        kernel_output_dir.join(&bundle.contract.stages.stage_00_build.kernel_image_path);

    println!(
        "[release:iso:{}:{distro_id}] building release run {} via compatibility hook {} (output: {})",
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
        .env(
            "S00_LIVE_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .live_uki_filename,
        )
        .env(
            "S00_EMERGENCY_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .emergency_uki_filename,
        )
        .env(
            "S00_DEBUG_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .debug_uki_filename,
        )
        .env(
            "S00_LIVE_CMDLINE",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .live_cmdline,
        )
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .env("ISO_FILENAME", &iso_filename)
        .env("PRODUCT_NAME", product.canonical)
        .env("BUILD_TARGET_LABEL", product.issue_banner_label)
        .env("ROOTFS_FILENAME", product.rootfs_erofs_filename)
        .env("INITRAMFS_LIVE_FILENAME", product.initramfs_live_filename)
        .env("LIVE_OVERLAY_DIRNAME", product.live_overlay_dir_name)
        .env(
            "LIVE_OVERLAY_IMAGE_FILENAME",
            product.overlay_erofs_filename,
        )
        .env(
            "ROOTFS_SOURCE_POINTER_FILENAME",
            product.rootfs_source_pointer_filename,
        )
        .env("RUN_ROOT_DIR", &build_layout.root_dir)
        .env("RUN_OUTPUT_DIR", output_dir)
        .env("RUN_OUTPUT_ROOT", output_dir)
        .env("BUILD_RUN_ID", build_layout.run_id.as_deref().unwrap_or(""))
        .env("DISTRO_BUILDER_BIN", &distro_builder_bin)
        .env("KERNEL_OUTPUT_DIR", kernel_output_dir)
        .env("COMPAT_BUILD_STAGE_NAME", compat_stage.canonical)
        .env("COMPAT_BUILD_STAGE_SLUG", compat_stage.slug)
        .env("COMPAT_BUILD_STAGE_DIRNAME", compat_stage.dir_name)
        .env("COMPAT_STAGE_ARTIFACT_TAG", compat_stage.artifact_tag)
        .env(
            "COMPAT_STAGE_REQUIRED_KERNEL_CMDLINE",
            stage_required_kernel_cmdline(bundle, compat_stage),
        )
        .status()
        .with_context(|| {
            format!(
                "running compatibility build hook '{}' for release product '{}' on '{}'",
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

fn stage_required_kernel_cmdline(
    bundle: &LoadedVariantContract,
    compat_stage: CompatibilityBuildStage,
) -> String {
    match compat_stage.slug {
        "s01_boot" | "s02_live_tools" => bundle
            .contract
            .stages
            .stage_01_live_boot
            .required_kernel_cmdline
            .join(" "),
        _ => String::new(),
    }
}
