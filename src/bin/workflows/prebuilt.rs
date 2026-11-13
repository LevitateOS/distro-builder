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

/// `prebuilt publish-runtime`: uploads the Recipe runtime image to its release.
pub(crate) fn publish_runtime_cmd(repo: &Path) -> Result<()> {
    prebuilt::publish_runtime(repo)
}

/// `prebuilt list`: every part CI publishes, one `<family> <name> [--distro <id>]` per line.
pub(crate) fn list_cmd(repo: &Path) -> Result<()> {
    for part in prebuilt::list(repo)? {
        match part.distro {
            Some(distro) => println!("{} {} --distro {distro}", part.family.name(), part.name),
            None => println!("{} {}", part.family.name(), part.name),
        }
    }
    Ok(())
}

/// `prebuilt build`: compiles the part into the local cache and prints its key, then the
/// cache directory.
pub(crate) fn build_cmd(repo: &Path, family_name: &str, name: &str, distro: Option<&str>) -> Result<()> {
    let (key, dir) = prebuilt::build(repo, family(family_name)?, name, distro)?;
    println!("{}", key.hex);
    println!("{}", dir.display());
    Ok(())
}
