use anyhow::{bail, Result};
use distro_builder::product::{ProductId, ReleaseProduct};
use std::path::Path;

/// `release build iso [<product>] <distro>`, the two in either order; the
/// product defaults to base-rootfs.
pub(crate) fn parse_release_build_command(
    args: Vec<&String>,
    repo_root: &Path,
) -> Result<(String, ReleaseProduct)> {
    let known_distros = crate::workflows::discover_distro_ids(repo_root)?;
    let is_distro = |arg: &str| known_distros.iter().any(|d| d == arg);

    match args.as_slice() {
        [] => bail!(
            "`release build iso` needs a distro (one of: {})",
            known_distros.join(", ")
        ),
        [distro] => Ok((
            parse_distro_id(distro, &known_distros)?,
            parse_release_product(ProductId::BaseRootfs.name())?,
        )),
        [a, b] if is_distro(a) => Ok((a.to_string(), parse_release_product(b)?)),
        [a, b] => Ok((parse_distro_id(b, &known_distros)?, parse_release_product(a)?)),
        _ => bail!(
            "unsupported positional arguments for `release build iso`; expected `[<product>] <distro>`"
        ),
    }
}

pub(crate) fn parse_distro_id(value: &str, known_distros: &[String]) -> Result<String> {
    if let Some(distro_id) = known_distros.iter().find(|d| d.as_str() == value) {
        return Ok(distro_id.to_string());
    }
    bail!(
        "unsupported distro '{}'; expected one of: {}",
        value,
        known_distros.join(", ")
    )
}

pub(crate) fn parse_product(value: &str) -> Result<ProductId> {
    ProductId::from_name(value).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported product '{value}'; expected one of: {}",
            ProductId::list(ProductId::ALL)
        )
    })
}

pub(crate) fn parse_release_product(value: &str) -> Result<ReleaseProduct> {
    let product = parse_product(value)?;
    ReleaseProduct::new(product).ok_or_else(|| {
        anyhow::anyhow!(
            "'{product}' is prepared, not built into a release ISO; release products: {}",
            ProductId::list(ProductId::releases())
        )
    })
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
