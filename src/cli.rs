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
    ReleaseTransport {
        distro: String,
        bundle: PathBuf,
        root: PathBuf,
        output: PathBuf,
        commit: String,
        part_bytes: u64,
    },
    ReleasePublish {
        distro: String,
        publication: Option<PathBuf>,
        additional_publications: Vec<PathBuf>,
        feed: PathBuf,
        audience: String,
        index_key: PathBuf,
    },
    ReleaseVerifyOnline {
        distro: String,
        directory: PathBuf,
        audience: String,
        representative: bool,
    },
    ReleaseInspectPublication {
        distro: String,
        directory: PathBuf,
    },
    ReleaseVerifyRemote {
        distro: String,
        directory: PathBuf,
        representative: bool,
    },
    ReleaseAuthorize {
        distro: String,
        document: PathBuf,
        output: PathBuf,
        root_key: PathBuf,
    },
    ReleaseVerifyFeed {
        distro: String,
        directory: PathBuf,
    },
    ReleaseIndex {
        distro: String,
        bundle: PathBuf,
        output: PathBuf,
        sequence: u64,
        signing_key: PathBuf,
    },
    /// Describe and verify existing release artifacts; never build them.
    ReleaseManifest {
        distro: String,
        flavour: Flavour,
        root: PathBuf,
        directory: PathBuf,
        signing_key: Option<PathBuf>,
    },
    /// Build `product`'s release ISO for `distro` from its system and overlay
    /// parts, building or reusing each (`parts`); no other product is built.
    /// `exact`: Recipe and the part keys name the exact Recipe and Ginux bytes,
    /// so nothing built by other tool binaries is reused
    /// (`recipe::exact_identity`; a signed build is always exact).
    ReleaseBuild {
        product: ProductId,
        distro: String,
        exact: bool,
        /// Compile our own tools and the TUI binary instead of reusing or
        /// downloading prebuilts (`prebuilt`); exact builds do so anyway.
        no_prebuilt: bool,
    },
    /// Print the key of a prebuilt (`prebuilt::Key`): `family` is `tools` or
    /// `tui`, `name` a tool package or a TUI binary (`install-tui`,
    /// `system-tui`); a TUI's libc comes from `distro`.
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
         release transport <distro> <bundle> <root> <output> <source-commit> <part-bytes>  verify/split an existing signed x86_64 release\n  \
         release publish <distro> <publication|refresh> <feed> <candidate|stable> --index-key PKCS8 [--publication DIR]  sign discovery metadata\n  \
         release inspect-publication <distro> <publication>  authenticate prepared public metadata\n  \
         release verify-online <distro> <feed> <candidate|stable> [--representative]  check deployed Pages bytes through Recipe\n  \
         release verify-remote <distro> <publication> [--representative]  verify public assets through Recipe\n  \
         release authorize <distro> <keys.json> <proof.json> --root-key PKCS8  sign public role authorization\n  \
         release verify-feed <distro> <directory>           authenticate staged production metadata (no builds/network)\n  \
         release index <distro> <bundle> <publish_dir> <sequence> --signing-key PKCS8  publish a signed local update index\n  \
         release manifest <distro> <base|desktop> <root> <dir> [--signing-key PKCS8]  verify/package existing release artifacts\n  \
         release build <product> <distro> [--exact] [--no-prebuilt]  build a release ISO from its cached parts ({releases}); --exact (implied when signed): reuse only what byte-identical Recipe and Ginux built, and use no prebuilt; --no-prebuilt: compile our tools and the TUI binary instead of reusing or downloading them\n  \
         product prepare <product> <distro> <dir>           prepare a product's rootfs and overlay ({products})\n  \
         transform build product-erofs <prepared_dir>       write a prepared product's EROFS images\n  \
         artifact prepare-tool-source <package> <dir>       write a tool's offline source tree\n  \
         artifact preseed-rootfs-source <distro> [--desktop] [--refresh]  acquire a distro's base (or desktop) lock\n  \
         prebuilt key <tools|tui> <name> [--distro <id>]    print the key of a prebuilt program (a tool package, or install-tui/system-tui, which needs --distro)\n  \
         prebuilt list                                      print every prebuilt part (<family> <name> [--distro <id>]), one per line\n  \
         prebuilt build <tools|tui> <name> [--distro <id>]  compile that part into the local cache (no download, no ISO; needs root) and print its key and cache directory\n  \
         prebuilt publish <tools|tui> <name> [--distro <id>]  upload the local cache entry of that key to the prebuilt repository's release with gh",
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
            Self::ReleaseTransport {
                distro,
                bundle,
                root,
                output,
                commit,
                part_bytes,
            } => push(&[
                &"release",
                &"transport",
                distro,
                bundle,
                root,
                output,
                commit,
                &part_bytes.to_string(),
            ]),
            Self::ReleasePublish {
                distro,
                publication,
                additional_publications,
                feed,
                audience,
                index_key,
            } => {
                push(&[
                    &"release",
                    &"publish",
                    distro,
                    &publication
                        .as_ref()
                        .map(|p| p.as_os_str())
                        .unwrap_or_else(|| std::ffi::OsStr::new("refresh")),
                    feed,
                    audience,
                    &"--index-key",
                    index_key,
                ]);
                for directory in additional_publications {
                    push(&[&"--publication", directory]);
                }
            }
            Self::ReleaseVerifyOnline {
                distro,
                directory,
                audience,
                representative,
            } => {
                push(&[&"release", &"verify-online", distro, directory, audience]);
                if *representative {
                    push(&[&"--representative"]);
                }
            }
            Self::ReleaseInspectPublication { distro, directory } => {
                push(&[&"release", &"inspect-publication", distro, directory])
            }
            Self::ReleaseVerifyRemote {
                distro,
                directory,
                representative,
            } => {
                push(&[&"release", &"verify-remote", distro, directory]);
                if *representative {
                    push(&[&"--representative"]);
                }
            }
            Self::ReleaseAuthorize {
                distro,
                document,
                output,
                root_key,
            } => push(&[
                &"release",
                &"authorize",
                distro,
                document,
                output,
                &"--root-key",
                root_key,
            ]),
            Self::ReleaseVerifyFeed { distro, directory } => {
                push(&[&"release", &"verify-feed", distro, directory])
            }
            Self::ReleaseIndex {
                distro,
                bundle,
                output,
                sequence,
                signing_key,
            } => push(&[
                &"release",
                &"index",
                distro,
                bundle,
                output,
                &sequence.to_string(),
                &"--signing-key",
                signing_key,
            ]),
            Self::ReleaseManifest {
                distro,
                flavour,
                root,
                directory,
                signing_key,
            } => {
                push(&[
                    &"release",
                    &"manifest",
                    distro,
                    &flavour.as_str(),
                    root,
                    directory,
                ]);
                if let Some(key) = signing_key {
                    push(&[&"--signing-key", key]);
                }
            }
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
        ["release", "transport", id, bundle, root, output, commit, part_bytes] => {
            Command::ReleaseTransport {
                distro: distro(id)?,
                bundle: bundle.into(),
                root: root.into(),
                output: output.into(),
                commit: commit.to_string(),
                part_bytes: part_bytes
                    .parse()
                    .map_err(|_| Usage("part-bytes must be an integer".into()))?,
            }
        }
        ["release", "publish", id, publication, feed, audience @ ("candidate" | "stable"), "--index-key", key, extra @ ..] => {
            if extra.len() % 2 != 0
                || extra.as_chunks::<2>().0.iter().any(|pair| pair[0] != "--publication")
                || (*publication == "refresh" && !extra.is_empty())
            {
                return usage("expected repeated --publication DIR after the index key; refresh cannot select releases".into());
            }
            Command::ReleasePublish {
                distro: distro(id)?,
                publication: (*publication != "refresh").then(|| PathBuf::from(publication)),
                additional_publications: extra.as_chunks::<2>().0.iter().map(|pair| PathBuf::from(pair[1])).collect(),
                feed: feed.into(),
                audience: audience.to_string(),
                index_key: key.into(),
            }
        }
        ["release", "verify-online", id, directory, audience @ ("candidate" | "stable"), flags @ ..]
            if flags.is_empty() || *flags == ["--representative"] =>
        {
            Command::ReleaseVerifyOnline {
                distro: distro(id)?,
                directory: directory.into(),
                audience: audience.to_string(),
                representative: !flags.is_empty(),
            }
        }
        ["release", "inspect-publication", id, directory] => Command::ReleaseInspectPublication {
            distro: distro(id)?,
            directory: directory.into(),
        },
        ["release", "verify-remote", id, directory, flags @ ..]
            if flags.is_empty() || *flags == ["--representative"] =>
        {
            Command::ReleaseVerifyRemote {
                distro: distro(id)?,
                directory: directory.into(),
                representative: !flags.is_empty(),
            }
        }
        ["release", "authorize", id, document, output, "--root-key", root_key] => {
            Command::ReleaseAuthorize {
                distro: distro(id)?,
                document: document.into(),
                output: output.into(),
                root_key: root_key.into(),
            }
        }
        ["release", "verify-feed", id, directory] => Command::ReleaseVerifyFeed {
            distro: distro(id)?,
            directory: directory.into(),
        },
        ["release", "index", id, bundle, output, sequence, "--signing-key", key] => {
            Command::ReleaseIndex {
                distro: distro(id)?,
                bundle: bundle.into(),
                output: output.into(),
                sequence: sequence
                    .parse()
                    .map_err(|_| Usage("index sequence must be an integer".into()))?,
                signing_key: key.into(),
            }
        }
        ["release", "manifest", id, flavour, root, dir, flags @ ..] => {
            let signing_key = match flags {
                [] => None,
                ["--signing-key", key] => Some(PathBuf::from(key)),
                _ => return usage("expected --signing-key <PKCS8>".into()),
            };
            Command::ReleaseManifest {
                distro: distro(id)?,
                flavour: flavour.parse().map_err(Usage)?,
                root: root.into(),
                directory: dir.into(),
                signing_key,
            }
        }
        ["release", "build", name, id, flags @ ..]
            if flags.iter().all(|flag| ["--exact", "--no-prebuilt"].contains(flag)) =>
        {
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
                exact: flags.contains(&"--exact"),
                no_prebuilt: flags.contains(&"--no-prebuilt"),
            }
        }
        ["prebuilt", "list"] => Command::PrebuiltList,
        ["prebuilt", verb @ ("key" | "build" | "publish"), family, name, flags @ ..] => {
            if crate::prebuilt::Family::parse(family).is_none() {
                return usage(format!(
                    "unsupported prebuilt family '{family}'; expected one of: {}",
                    crate::prebuilt::Family::NAMES
                ));
            }
            let selected = match flags {
                [] => None,
                ["--distro", id] => Some(distro(id)?),
                _ => return usage("expected --distro <id>".into()),
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
