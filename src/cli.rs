//! distro-builder's commands: one typed list, its one argument grammar, and the
//! arguments for each. The binary parses its arguments with [`parse`]; xtask builds
//! a [`Command`] and runs the binary with [`Command::to_args`], so both sides use
//! the same type.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use distro_contract::profile::Flavour;
use distro_contract::ProductId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Build `product`'s release ISO for `distro` from its system and overlay
    /// parts, building or reusing each (`parts`); no other product is built.
    /// `exact`: Recipe and the part keys name the exact Recipe and Ginux bytes,
    /// so nothing built by other tool binaries is reused
    /// (`recipe::exact_identity`).
    ReleaseBuild {
        product: ProductId,
        distro: String,
        exact: bool,
        /// Compile our own tools and the TUI binary instead of reusing or
        /// downloading prebuilts (`prebuilt`); exact builds do so anyway.
        no_prebuilt: bool,
    },
    /// Print the key of a prebuilt (`prebuilt::Key`): `family` is `tools`,
    /// `tui`, `desktop-apps` or `display-server`, `name` a tool package, a TUI binary
    /// (`install-tui`) or, for the desktop apps and the display server, the
    /// distro; a TUI's libc comes from `distro`.
    PrebuiltKey {
        family: String,
        name: String,
        distro: Option<String>,
    },
    /// Upload the local cache entry of a prebuilt's current key to the prebuilt
    /// repository's release with `gh` (never replacing an asset that differs).
    PrebuiltPublish {
        family: String,
        name: String,
        distro: Option<String>,
    },
    /// Compress the Recipe runtime image the installed Recipe ships and upload it to the
    /// prebuilt repository's release `recipe-runtime-<digest16>` with `gh`.
    PrebuiltPublishRuntime,
    /// Compile a prebuilt (no download, no ISO) into the local cache and print its key and
    /// cache directory: what CI publishes. Needs root, like every compile.
    PrebuiltBuild {
        family: String,
        name: String,
        distro: Option<String>,
    },
    /// Print every prebuilt part the builder knows, one `<family> <name> [--distro <id>]`
    /// per line.
    PrebuiltList,
    /// Prepare `product`'s rootfs source and overlay for `distro` in `output`:
    /// copies of its parts' trees.
    ProductPrepare {
        product: ProductId,
        distro: String,
        output: PathBuf,
    },
    /// Write both EROFS images of a prepared product.
    ProductErofs {
        prepared: PathBuf,
    },
    /// Write a tool package's offline source tree to `output`.
    PrepareToolSource {
        package: String,
        output: PathBuf,
    },
    /// Run `distro`'s rootfs-source acquisition of `flavour`'s lock (the
    /// desktop system needs the base lock's too); `refresh` acquires again.
    PreseedRootfsSource {
        distro: String,
        flavour: Flavour,
        refresh: bool,
    },
}

/// A usage error: the message to show above [`usage`]. The binary exits 2.
#[derive(Debug)]
pub struct Usage(pub String);

/// The grammar, for `--help` and usage errors.
pub fn usage() -> String {
    format!(
        "Usage: distro-builder --repo-root <dir> <command>\n\n\
         Commands:\n  \
         release build <product> <distro> [--exact] [--no-prebuilt]  build a release ISO from its cached parts ({releases}); --exact: reuse only what byte-identical Recipe and Ginux built, and use no prebuilt; --no-prebuilt: compile our tools and the TUI binary instead of reusing or downloading them\n  \
         product prepare <product> <distro> <dir>           prepare a product's rootfs and overlay ({products})\n  \
         transform build product-erofs <prepared_dir>       write a prepared product's EROFS images\n  \
         artifact prepare-tool-source <package> <dir>       write a tool's offline source tree\n  \
         artifact preseed-rootfs-source <distro> [--desktop] [--refresh]  acquire a distro's base (or desktop) lock\n  \
         prebuilt key <tools|tui|desktop-apps|display-server> <name> [--distro <id>]    print the key of a prebuilt (a tool package; install-tui, which needs --distro; or desktop-apps/display-server <distro>)\n  \
         prebuilt list                                      print every prebuilt part (<family> <name> [--distro <id>]), one per line\n  \
         prebuilt build <tools|tui|desktop-apps|display-server> <name> [--distro <id>]  compile that part into the local cache (no download, no ISO; needs root) and print its key and cache directory\n  \
         prebuilt publish <tools|tui|desktop-apps|display-server> <name> [--distro <id>]  upload the local cache entry of that key to the prebuilt repository's release with gh\n  \
         prebuilt publish-runtime                           compress the Recipe runtime image and upload it to the release recipe-runtime-<digest16> with gh",
        releases = ProductId::list(ProductId::iso_products()),
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
            Self::ReleaseBuild {
                product,
                distro,
                exact,
                no_prebuilt,
            } => {
                push(&[&"release", &"build", &product.name(), distro]);
                if *exact {
                    push(&[&"--exact"]);
                }
                if *no_prebuilt {
                    push(&[&"--no-prebuilt"]);
                }
            }
            Self::PrebuiltList => push(&[&"prebuilt", &"list"]),
            Self::PrebuiltPublishRuntime => push(&[&"prebuilt", &"publish-runtime"]),
            Self::PrebuiltKey {
                family,
                name,
                distro,
            }
            | Self::PrebuiltBuild {
                family,
                name,
                distro,
            }
            | Self::PrebuiltPublish {
                family,
                name,
                distro,
            } => {
                let verb = match self {
                    Self::PrebuiltKey { .. } => "key",
                    Self::PrebuiltBuild { .. } => "build",
                    _ => "publish",
                };
                push(&[&"prebuilt", &verb, family, name]);
                if let Some(distro) = distro {
                    push(&[&"--distro", distro]);
                }
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
            Self::PreseedRootfsSource {
                distro,
                flavour,
                refresh,
            } => {
                push(&[&"artifact", &"preseed-rootfs-source", distro]);
                if *flavour == Flavour::Desktop {
                    push(&[&"--desktop"]);
                }
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
        ["release", "build", name, id, flags @ ..]
            if flags.iter().all(|flag| ["--exact", "--no-prebuilt"].contains(flag)) =>
        {
            let product = product(name)?;
            if !product.is_iso() {
                return usage(format!(
                    "'{product}' is prepared, not built into a release ISO; release products: {}",
                    ProductId::list(ProductId::iso_products())
                ));
            }
            Command::ReleaseBuild {
                product,
                distro: distro(id)?,
                exact: flags.contains(&"--exact"),
                no_prebuilt: flags.contains(&"--no-prebuilt"),
            }
        }
        ["prebuilt", "list"] => Command::PrebuiltList,
        ["prebuilt", "publish-runtime"] => Command::PrebuiltPublishRuntime,
        ["prebuilt", verb @ ("key" | "build" | "publish"), family, name, flags @ ..] => {
            if crate::prebuilt::Family::parse(family).is_none() {
                return usage(format!(
                    "unsupported prebuilt family '{family}'; expected one of: {}",
                    crate::prebuilt::Family::NAMES
                ));
            }
            let selected = if crate::prebuilt::Family::parse(family).is_some_and(|f| f.per_distro()) {
                // The desktop apps' and the display server's name is the distro itself.
                distro(name)?;
                if !flags.is_empty() {
                    return usage(format!("prebuilt {family} takes the distro as its name and no flags"));
                }
                None
            } else {
                match flags {
                    [] => None,
                    ["--distro", id] => Some(distro(id)?),
                    _ => return usage("expected --distro <id>".into()),
                }
            };
            let (family, name) = (family.to_string(), name.to_string());
            match *verb {
                "key" => Command::PrebuiltKey { family, name, distro: selected },
                "build" => Command::PrebuiltBuild { family, name, distro: selected },
                _ => Command::PrebuiltPublish { family, name, distro: selected },
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
        ["artifact", "preseed-rootfs-source", id, flags @ ..]
            if flags
                .iter()
                .all(|flag| ["--desktop", "--refresh"].contains(flag)) =>
        {
            Command::PreseedRootfsSource {
                distro: distro(id)?,
                flavour: if flags.contains(&"--desktop") {
                    Flavour::Desktop
                } else {
                    Flavour::Base
                },
                refresh: flags.contains(&"--refresh"),
            }
        }
        _ => return usage(format!("unknown command '{}'", words.join(" "))),
    };
    Ok((repo, command))
}
