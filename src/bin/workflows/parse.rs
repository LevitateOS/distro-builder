use anyhow::{bail, Result};
use std::path::Path;

pub(crate) fn parse_release_build_command(
    args: Vec<&String>,
    repo_root: &Path,
) -> Result<(String, crate::BuildProduct)> {
    let known_distros = crate::workflows::discover_distro_ids(repo_root)?;

    match args.as_slice() {
        [] => Ok((
            crate::DEFAULT_DISTRO_ID.to_string(),
            parse_release_product(None)?,
        )),
        [arg] => parse_release_one_arg(arg, &known_distros),
        [arg1, arg2] => parse_release_two_args(arg1, arg2, &known_distros),
        _ => bail!(
            "unsupported positional arguments for `release build iso`; expected `[product_or_distro] [product_or_distro]`, max 2 args"
        ),
    }
}

pub(crate) fn parse_release_one_arg(
    arg: &str,
    known_distros: &[String],
) -> Result<(String, crate::BuildProduct)> {
    if let Ok(distro_id) = parse_distro_id(arg, known_distros) {
        return Ok((distro_id, parse_release_product(None)?));
    }

    let product = parse_release_product(Some(arg))?;
    Ok((crate::DEFAULT_DISTRO_ID.to_string(), product))
}

pub(crate) fn parse_release_two_args(
    arg1: &str,
    arg2: &str,
    known_distros: &[String],
) -> Result<(String, crate::BuildProduct)> {
    if let Ok(distro_id) = parse_distro_id(arg1, known_distros) {
        if let Ok(product) = parse_release_product(Some(arg2)) {
            return Ok((distro_id, product));
        }
    }

    if let Ok(product) = parse_release_product(Some(arg1)) {
        let distro_id = parse_distro_id(arg2, known_distros)?;
        return Ok((distro_id, product));
    }

    let known_distros = known_distros.join(", ");
    bail!(
        "invalid `release build iso` arguments: '{}' '{}'. Expected `<distro> <product>` or `<product> <distro>`.\n\
         `product` supports: {}, {}, {}.\n\
         available distros: {}",
        arg1,
        arg2,
        crate::PRODUCT_BASE_ROOTFS,
        crate::PRODUCT_LIVE_BOOT,
        crate::PRODUCT_LIVE_TOOLS,
        known_distros
    )
}

pub(crate) fn parse_distro_id(value: &str, known_distros: &[String]) -> Result<String> {
    if let Some(distro_id) = known_distros.iter().find(|d| d.as_str() == value) {
        return Ok(distro_id.to_string());
    }

    let supported = known_distros.join(", ");
    bail!(
        "unsupported distro '{}'; expected one of: {}",
        value,
        supported
    )
}

pub(crate) fn parse_product(value: Option<&str>) -> Result<crate::BuildProduct> {
    match value.unwrap_or(crate::PRODUCT_BASE_ROOTFS) {
        crate::PRODUCT_BASE_ROOTFS => Ok(product_base_rootfs()),
        crate::PRODUCT_LIVE_BOOT => Ok(product_live_boot()),
        crate::PRODUCT_LIVE_TOOLS => Ok(product_live_tools()),
        crate::PRODUCT_INSTALLED_BOOT => Ok(product_installed_boot()),
        other => bail!(
            "unsupported product '{}'; expected one of: '{}', '{}', '{}', '{}'",
            other,
            crate::PRODUCT_BASE_ROOTFS,
            crate::PRODUCT_LIVE_BOOT,
            crate::PRODUCT_LIVE_TOOLS,
            crate::PRODUCT_INSTALLED_BOOT
        ),
    }
}

pub(crate) fn parse_release_product(value: Option<&str>) -> Result<crate::BuildProduct> {
    let product = parse_product(value)?;
    if product.canonical == crate::PRODUCT_INSTALLED_BOOT {
        bail!(
            "unsupported release build product '{}'; release build supports '{}', '{}', '{}'.\n\
             '{}' is a canonical product preparation target, not a release ISO target.",
            product.canonical,
            crate::PRODUCT_BASE_ROOTFS,
            crate::PRODUCT_LIVE_BOOT,
            crate::PRODUCT_LIVE_TOOLS,
            crate::PRODUCT_INSTALLED_BOOT
        );
    }
    Ok(product)
}

pub(crate) fn product_for_logical_name(logical_name: &str) -> Result<crate::BuildProduct> {
    match logical_name {
        "product.rootfs.base" => Ok(product_base_rootfs()),
        "product.payload.boot.live" => Ok(product_live_boot()),
        "product.payload.live_tools" => Ok(product_live_tools()),
        "product.payload.boot.installed" => Ok(product_installed_boot()),
        other => bail!(
            "unsupported canonical product logical name '{}'; expected one of: product.rootfs.base, product.payload.boot.live, product.payload.live_tools, product.payload.boot.installed",
            other
        ),
    }
}

fn product_base_rootfs() -> crate::BuildProduct {
    crate::BuildProduct {
        canonical: crate::PRODUCT_BASE_ROOTFS,
        release_dir_name: crate::PRODUCT_BASE_ROOTFS,
        release_hook_script_name: Some("build-release.sh"),
        iso_suffix: "base-rootfs",
        live_overlay_dir_name: "live-overlay",
        rootfs_source_pointer_filename: ".live-rootfs-source.path",
        issue_banner_label: "Base Rootfs",
    }
}

fn product_live_boot() -> crate::BuildProduct {
    crate::BuildProduct {
        canonical: crate::PRODUCT_LIVE_BOOT,
        release_dir_name: crate::PRODUCT_LIVE_BOOT,
        release_hook_script_name: Some("boot-release.sh"),
        iso_suffix: "live-boot",
        live_overlay_dir_name: "live-overlay",
        rootfs_source_pointer_filename: ".live-rootfs-source.path",
        issue_banner_label: "Live Boot",
    }
}

fn product_live_tools() -> crate::BuildProduct {
    crate::BuildProduct {
        canonical: crate::PRODUCT_LIVE_TOOLS,
        release_dir_name: crate::PRODUCT_LIVE_TOOLS,
        release_hook_script_name: Some("live-tools-release.sh"),
        iso_suffix: "live-tools",
        live_overlay_dir_name: "live-overlay",
        rootfs_source_pointer_filename: ".live-rootfs-source.path",
        issue_banner_label: "Live Tools",
    }
}

fn product_installed_boot() -> crate::BuildProduct {
    crate::BuildProduct {
        canonical: crate::PRODUCT_INSTALLED_BOOT,
        release_dir_name: crate::PRODUCT_INSTALLED_BOOT,
        release_hook_script_name: None,
        iso_suffix: "installed-boot",
        live_overlay_dir_name: "boot-overlay",
        rootfs_source_pointer_filename: ".rootfs-source.path",
        issue_banner_label: "Installed Boot",
    }
}

pub(crate) fn discover_distro_ids(repo_root: &Path) -> Result<Vec<String>> {
    let distro_ids = distro_contract::distro_ids(repo_root);
    if distro_ids.is_empty() {
        bail!(
            "no distro variants under '{}'",
            repo_root.join("distro/variants").display()
        );
    }
    Ok(distro_ids)
}
