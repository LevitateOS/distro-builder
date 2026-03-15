use anyhow::{bail, Context, Result};
use std::fs;
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
    let variants_dir = repo_root.join("distro-variants");
    let entries = fs::read_dir(&variants_dir)
        .with_context(|| format!("reading variants directory '{}'", variants_dir.display()))?;

    let mut distro_ids = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "reading entry under variants directory '{}'",
                variants_dir.display()
            )
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if !path.join("identity.toml").is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        distro_ids.push(name.to_string());
    }

    if distro_ids.is_empty() {
        bail!(
            "no distro variants discovered under '{}'; expected directories with identity.toml",
            variants_dir.display()
        );
    }

    distro_ids.sort();
    Ok(distro_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_parser_accepts_canonical_names() {
        assert_eq!(
            parse_product(Some(crate::PRODUCT_BASE_ROOTFS))
                .expect("parse base-rootfs")
                .canonical,
            crate::PRODUCT_BASE_ROOTFS
        );
        assert_eq!(
            parse_product(Some(crate::PRODUCT_LIVE_BOOT))
                .expect("parse live-boot")
                .canonical,
            crate::PRODUCT_LIVE_BOOT
        );
        assert_eq!(
            parse_product(Some(crate::PRODUCT_LIVE_TOOLS))
                .expect("parse live-tools")
                .canonical,
            crate::PRODUCT_LIVE_TOOLS
        );
        assert_eq!(
            parse_product(Some(crate::PRODUCT_INSTALLED_BOOT))
                .expect("parse installed-boot")
                .canonical,
            crate::PRODUCT_INSTALLED_BOOT
        );
    }

    #[test]
    fn product_parser_rejects_stage_aliases() {
        let err = parse_product(Some("01Boot")).expect_err("stage alias must be rejected");
        assert!(
            err.to_string().contains("unsupported product"),
            "unexpected error: {err:#}"
        );
        let err = parse_product(Some("02")).expect_err("numeric stage alias must be rejected");
        assert!(
            err.to_string().contains("unsupported product"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn product_for_logical_name_maps_runtime_products() {
        assert_eq!(
            product_for_logical_name("product.rootfs.base")
                .expect("map rootfs base")
                .canonical,
            crate::PRODUCT_BASE_ROOTFS
        );
        assert_eq!(
            product_for_logical_name("product.payload.boot.live")
                .expect("map live boot")
                .canonical,
            crate::PRODUCT_LIVE_BOOT
        );
        assert_eq!(
            product_for_logical_name("product.payload.live_tools")
                .expect("map live tools")
                .canonical,
            crate::PRODUCT_LIVE_TOOLS
        );
        assert_eq!(
            product_for_logical_name("product.payload.boot.installed")
                .expect("map installed boot")
                .canonical,
            crate::PRODUCT_INSTALLED_BOOT
        );
    }

    #[test]
    fn release_product_parser_rejects_installed_boot() {
        let err = parse_release_product(Some(crate::PRODUCT_INSTALLED_BOOT))
            .expect_err("installed-boot must not be a release ISO product");
        assert!(
            err.to_string()
                .contains("canonical product preparation target"),
            "unexpected error: {err:#}"
        );
    }
}
