use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

pub(crate) fn parse_release_build_command(
    args: Vec<&String>,
    repo_root: &Path,
) -> Result<(String, crate::BuildProduct)> {
    let known_distros = crate::workflows::discover_distro_ids(repo_root)?;

    match args.as_slice() {
        [] => Ok((crate::DEFAULT_DISTRO_ID.to_string(), parse_product(None)?)),
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
        return Ok((distro_id, parse_product(None)?));
    }

    let product = parse_product(Some(arg))?;
    Ok((crate::DEFAULT_DISTRO_ID.to_string(), product))
}

pub(crate) fn parse_release_two_args(
    arg1: &str,
    arg2: &str,
    known_distros: &[String],
) -> Result<(String, crate::BuildProduct)> {
    if let Ok(distro_id) = parse_distro_id(arg1, known_distros) {
        if let Ok(product) = parse_product(Some(arg2)) {
            return Ok((distro_id, product));
        }
    }

    if let Ok(product) = parse_product(Some(arg1)) {
        let distro_id = parse_distro_id(arg2, known_distros)?;
        return Ok((distro_id, product));
    }

    let known_distros = known_distros.join(", ");
    bail!(
        "invalid `release build iso` arguments: '{}' '{}'. Expected `<distro> <product>` or `<product> <distro>`.\n\
         `product` supports: {}, {}, {}; compatibility aliases: 00Build|01Boot|02LiveTools|0|00|1|01|2|02.\n\
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
        crate::PRODUCT_BASE_ROOTFS | crate::STAGE00_CANONICAL | "0" | "00" => {
            Ok(product_base_rootfs())
        }
        crate::PRODUCT_LIVE_BOOT | crate::STAGE01_CANONICAL | "1" | "01" => {
            Ok(product_live_boot())
        }
        crate::PRODUCT_LIVE_TOOLS | crate::STAGE02_CANONICAL | "2" | "02" => {
            Ok(product_live_tools())
        }
        other => bail!(
            "unsupported product '{}'; expected one of: '{}', '{}', '{}'; compatibility aliases: 00Build|01Boot|02LiveTools|0|00|1|01|2|02",
            other,
            crate::PRODUCT_BASE_ROOTFS,
            crate::PRODUCT_LIVE_BOOT,
            crate::PRODUCT_LIVE_TOOLS
        ),
    }
}

pub(crate) fn product_for_stage(stage: crate::BuildStage) -> crate::BuildProduct {
    match stage.slug {
        crate::STAGE00_SLUG => product_base_rootfs(),
        crate::STAGE01_SLUG => product_live_boot(),
        crate::STAGE02_SLUG => product_live_tools(),
        _ => unreachable!("validated in parse_stage"),
    }
}

pub(crate) fn parse_stage(value: Option<&str>) -> Result<crate::BuildStage> {
    Ok(parse_product(value)?.compatibility_stage)
}

fn product_base_rootfs() -> crate::BuildProduct {
    crate::BuildProduct {
        canonical: crate::PRODUCT_BASE_ROOTFS,
        compatibility_stage: crate::BuildStage {
            canonical: crate::STAGE00_CANONICAL,
            slug: crate::STAGE00_SLUG,
            dir_name: crate::STAGE00_DIRNAME,
        },
    }
}

fn product_live_boot() -> crate::BuildProduct {
    crate::BuildProduct {
        canonical: crate::PRODUCT_LIVE_BOOT,
        compatibility_stage: crate::BuildStage {
            canonical: crate::STAGE01_CANONICAL,
            slug: crate::STAGE01_SLUG,
            dir_name: crate::STAGE01_DIRNAME,
        },
    }
}

fn product_live_tools() -> crate::BuildProduct {
    crate::BuildProduct {
        canonical: crate::PRODUCT_LIVE_TOOLS,
        compatibility_stage: crate::BuildStage {
            canonical: crate::STAGE02_CANONICAL,
            slug: crate::STAGE02_SLUG,
            dir_name: crate::STAGE02_DIRNAME,
        },
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

        if !path.join("00Build.toml").is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        distro_ids.push(name.to_string());
    }

    if distro_ids.is_empty() {
        bail!(
            "no distro variants discovered under '{}'; expected directories with 00Build.toml",
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
    }

    #[test]
    fn product_parser_accepts_stage_aliases() {
        assert_eq!(
            parse_product(Some(crate::STAGE01_CANONICAL))
                .expect("parse stage alias")
                .canonical,
            crate::PRODUCT_LIVE_BOOT
        );
        assert_eq!(
            parse_product(Some("02"))
                .expect("parse numeric stage alias")
                .canonical,
            crate::PRODUCT_LIVE_TOOLS
        );
    }
}
