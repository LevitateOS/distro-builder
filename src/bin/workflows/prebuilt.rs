use anyhow::{Context, Result};
use distro_builder::prebuilt::{self, Family};
use std::path::Path;

fn family(name: &str) -> Result<Family> {
    Family::parse(name).with_context(|| format!("unsupported prebuilt family '{name}'"))
}

/// `prebuilt key`: prints the key, for CI to compute the same one.
pub(crate) fn key_cmd(repo: &Path, family_name: &str, name: &str, distro: Option<&str>) -> Result<()> {
    let key = prebuilt::key_for(repo, family(family_name)?, name, distro)?;
    println!("{}", key.hex);
    Ok(())
}

/// `prebuilt publish`: uploads the local cache entry of the current key.
pub(crate) fn publish_cmd(repo: &Path, family_name: &str, name: &str, distro: Option<&str>) -> Result<()> {
    let family = family(family_name)?;
    let key = prebuilt::key_for(repo, family, name, distro)?;
    prebuilt::publish(repo, family, name, &key)
}
