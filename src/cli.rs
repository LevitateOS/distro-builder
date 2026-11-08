//! distro-builder's commands: one typed list, its one argument grammar, and the
//! arguments for each. The binary parses its arguments with [`parse`]; xtask builds
//! a [`Command`] and runs the binary with [`Command::to_args`], so both sides use
//! the same type.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use distro_contract::ProductId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Build `product`'s release ISO for `distro`, after any parent release it lacks.
    ReleaseBuild { product: ProductId, distro: String },
    /// Prepare `product`'s rootfs source and overlay for `distro` in `output`.
    ProductPrepare {
        product: ProductId,
        distro: String,
        output: PathBuf,
    },
    /// Write both EROFS images of a prepared product.
    ProductErofs { prepared: PathBuf },
    /// Write a tool package's offline source tree to `output`.
    PrepareToolSource { package: String, output: PathBuf },
    /// Run `distro`'s rootfs-source acquisition; `refresh` acquires again.
    PreseedRootfsSource { distro: String, refresh: bool },
}

/// A usage error: the message to show above [`usage`]. The binary exits 2.
#[derive(Debug)]
pub struct Usage(pub String);

/// The grammar, for `--help` and usage errors.
pub fn usage() -> String {
    format!(
        "Usage: distro-builder --repo-root <dir> <command>\n\n\
         Commands:\n  \
         release build <product> <distro>                   build a release ISO ({releases})\n  \
         product prepare <product> <distro> <dir>           prepare a product's rootfs and overlay ({products})\n  \
         transform build product-erofs <prepared_dir>       write a prepared product's EROFS images\n  \
         artifact prepare-tool-source <package> <dir>       write a tool's offline source tree\n  \
         artifact preseed-rootfs-source <distro> [--refresh]  acquire a distro's rootfs source",
        releases = ProductId::list(ProductId::releases()),
        products = ProductId::list(ProductId::ALL),
    )
}

impl Command {
    /// The binary's arguments for this command on the checkout at `repo_root`.
    pub fn to_args(&self, repo_root: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["--repo-root".into(), repo_root.into()];
        let mut push = |words: &[&dyn AsRef<std::ffi::OsStr>]| {
            args.extend(words.iter().map(|word| word.as_ref().to_os_string()));
        };
        match self {
            Self::ReleaseBuild { product, distro } => {
                push(&[&"release", &"build", &product.name(), distro])
            }
            Self::ProductPrepare {
                product,
                distro,
                output,
            } => push(&[&"product", &"prepare", &product.name(), distro, output]),
            Self::ProductErofs { prepared } => {
                push(&[&"transform", &"build", &"product-erofs", prepared])
            }
            Self::PrepareToolSource { package, output } => {
                push(&[&"artifact", &"prepare-tool-source", package, output])
            }
            Self::PreseedRootfsSource { distro, refresh } => {
                push(&[&"artifact", &"preseed-rootfs-source", distro]);
                if *refresh {
                    push(&[&"--refresh"]);
                }
            }
        }
        args
    }
}

/// The repo root and command from the binary's arguments (without `argv[0]`).
pub fn parse(args: &[String]) -> Result<(PathBuf, Command), Usage> {
    let usage = |message: String| Err(Usage(message));
    let [flag, root, rest @ ..] = args else {
        return usage("the first argument must be --repo-root <dir>".into());
    };
    if flag != "--repo-root" {
        return usage(format!("expected --repo-root <dir>, got '{flag}'"));
    }
    let repo = PathBuf::from(root);
    if !repo.join("distro/variants").is_dir() {
        return usage(format!(
            "'{root}' is not a LevitateOS checkout (it has no distro/variants)"
        ));
    }
    let distro = |name: &str| {
        let known = distro_contract::distro_ids(&repo);
        if known.iter().any(|id| id == name) {
            Ok(name.to_string())
        } else {
            Err(Usage(format!(
                "unsupported distro '{name}'; expected one of: {}",
                known.join(", ")
            )))
        }
    };
    let product = |name: &str| {
        ProductId::from_name(name).ok_or_else(|| {
            Usage(format!(
                "unsupported product '{name}'; expected one of: {}",
                ProductId::list(ProductId::ALL)
            ))
        })
    };
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    let command = match words.as_slice() {
        ["release", "build", name, id] => {
            let product = product(name)?;
            if !product.is_release() {
                return usage(format!(
                    "'{product}' is prepared, not built into a release ISO; release products: {}",
                    ProductId::list(ProductId::releases())
                ));
            }
            Command::ReleaseBuild {
                product,
                distro: distro(id)?,
            }
        }
        ["product", "prepare", name, id, output] => Command::ProductPrepare {
            product: product(name)?,
            distro: distro(id)?,
            output: output.into(),
        },
        ["transform", "build", "product-erofs", prepared] => Command::ProductErofs {
            prepared: prepared.into(),
        },
        ["artifact", "prepare-tool-source", package, output] => Command::PrepareToolSource {
            package: package.to_string(),
            output: output.into(),
        },
        ["artifact", "preseed-rootfs-source", id] => Command::PreseedRootfsSource {
            distro: distro(id)?,
            refresh: false,
        },
        ["artifact", "preseed-rootfs-source", id, "--refresh"] => Command::PreseedRootfsSource {
            distro: distro(id)?,
            refresh: true,
        },
        _ => return usage(format!("unknown command '{}'", words.join(" "))),
    };
    Ok((repo, command))
}
