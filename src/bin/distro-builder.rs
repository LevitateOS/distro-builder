//! distro-builder: the command line over the library. Its one grammar is
//! `distro_builder::cli`; each command calls one workflow. A usage error exits 2,
//! a failed command exits 1.

use anyhow::{Context, Result};
use distro_builder::cli::{self, Command};
use distro_builder::process::{self, HostNeed};
use std::path::Path;
use std::process::ExitCode;

mod workflows;

use workflows::{artifacts, build, prebuilt};

fn main() -> ExitCode {
    let args: Result<Vec<String>, _> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect();
    let args = match args {
        Ok(args) => args,
        Err(arg) => return usage_error(&format!("argument {arg:?} is not UTF-8")),
    };
    if matches!(args.as_slice(), [flag] if flag == "--help" || flag == "-h") {
        println!("{}", cli::usage());
        return ExitCode::SUCCESS;
    }
    let (repo, command) = match cli::parse(&args) {
        Ok(parsed) => parsed,
        Err(cli::Usage(message)) => return usage_error(&message),
    };
    match run(&repo, command).with_context(|| format!("distro-builder {}", args[2..].join(" "))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("distro-builder: {message}\n\n{}", cli::usage());
    ExitCode::from(2)
}

fn run(repo: &Path, command: Command) -> Result<()> {
    // If the launcher dies (cancel, abort), this build dies with it.
    process::die_with_parent().context("arming the parent-death signal for distro-builder")?;
    match command {
        Command::ReleaseTransport {
            distro,
            bundle,
            root,
            output,
            commit,
            part_bytes,
        } => {
            let variant = distro_contract::load(repo, &distro)?;
            let value = distro_builder::release_index::production_publish::prepare(
                &variant, &bundle, &root, &output, &commit, part_bytes,
            )?;
            println!("{}", serde_json::to_string(&value)?);
            Ok(())
        }
        Command::ReleasePublish {
            distro,
            publication,
            additional_publications,
            feed,
            audience,
            index_key,
        } => {
            let variant = distro_contract::load(repo, &distro)?;
            let publications: Vec<_> = publication
                .into_iter()
                .chain(additional_publications)
                .collect();
            let audience = if audience == "candidate" {
                distro_contract::release::production::Audience::Candidate
            } else {
                distro_contract::release::production::Audience::Stable
            };
            println!(
                "{}",
                distro_builder::release_index::production_publish::publish(
                    &variant,
                    &publications,
                    &feed,
                    audience,
                    &index_key
                )?
            );
            Ok(())
        }
        Command::ReleaseVerifyOnline {
            distro,
            directory,
            audience,
            representative,
        } => {
            let variant = distro_contract::load(repo, &distro)?;
            let audience = if audience == "candidate" {
                distro_contract::release::production::Audience::Candidate
            } else {
                distro_contract::release::production::Audience::Stable
            };
            println!(
                "{}",
                distro_builder::release_index::production_publish::verify_online(
                    &variant,
                    &directory,
                    audience,
                    representative
                )?
            );
            Ok(())
        }
        Command::ReleaseInspectPublication { distro, directory } => {
            let variant = distro_contract::load(repo, &distro)?;
            println!(
                "{}",
                serde_json::to_string(
                    &distro_builder::release_index::production_publish::read_publication(
                        &variant,
                        &directory,
                        distro_contract::release::production::Audience::Candidate
                    )?
                )?
            );
            Ok(())
        }
        Command::ReleaseVerifyRemote {
            distro,
            directory,
            representative,
        } => {
            let variant = distro_contract::load(repo, &distro)?;
            println!(
                "{}",
                distro_builder::release_index::production_publish::verify_remote(
                    &variant,
                    &directory,
                    representative
                )?
            );
            Ok(())
        }
        Command::ReleaseAuthorize {
            distro,
            document,
            output,
            root_key,
        } => {
            let variant = distro_contract::load(repo, &distro)?;
            distro_builder::release_index::authorize(&variant, &document, &output, &root_key)?;
            println!("{}", output.display());
            Ok(())
        }
        Command::ReleaseVerifyFeed { distro, directory } => {
            let variant = distro_contract::load(repo, &distro)?;
            println!(
                "{}",
                distro_builder::release_index::verify_feed(&variant, &directory)?
            );
            Ok(())
        }
        Command::ReleaseIndex {
            distro,
            bundle,
            output,
            sequence,
            signing_key,
        } => {
            let variant = distro_contract::load(repo, &distro)?;
            distro_builder::release_index::publish(
                &variant,
                &bundle,
                &output,
                sequence,
                &signing_key,
            )?;
            println!("{}", output.join(distro_contract::release::INDEX).display());
            Ok(())
        }
        Command::ReleaseManifest {
            distro,
            flavour,
            root,
            directory,
            signing_key,
        } => {
            let variant = distro_contract::load(repo, &distro)?;
            let manifest = distro_builder::release_manifest::write(
                &variant,
                flavour,
                &root,
                &directory,
                &directory.join(recinit::live::KERNEL_ISO_PATH),
                &directory.join(recinit::live::INITRAMFS_INSTALLED_ISO_PATH),
                signing_key.as_deref(),
            )?;
            println!("{}", manifest.path.display());
            Ok(())
        }
        Command::ReleaseBuild {
            product,
            distro,
            exact,
            no_prebuilt,
        } => {
            process::check_host_tools(HostNeed::Build)?;
            if no_prebuilt {
                distro_builder::prebuilt::select_no_prebuilt();
            }
            if exact {
                distro_builder::recipe::select_exact_identity();
            }
            if distro_builder::recipe::exact_identity() {
                distro_builder::tprintln!(
                    "exact identity: reusing only what byte-identical Recipe and Ginux built{}",
                    if exact { "" } else { " (the release is signed)" }
                );
            }
            build::build_one(repo, &distro, product)
        }
        Command::PrebuiltKey { family, name, distro } => {
            prebuilt::key_cmd(repo, &family, &name, distro.as_deref())
        }
        Command::PrebuiltList => prebuilt::list_cmd(repo),
        Command::PrebuiltBuild { family, name, distro } => {
            prebuilt::build_cmd(repo, &family, &name, distro.as_deref())
        }
        Command::PrebuiltPublish { family, name, distro } => {
            prebuilt::publish_cmd(repo, &family, &name, distro.as_deref())
        }
        Command::ProductPrepare {
            product,
            distro,
            output,
        } => artifacts::prepare_product_cmd(repo, product, &distro, &output),
        Command::ProductErofs { prepared } => {
            artifacts::build_prepared_product_erofs_cmd(&prepared)
        }
        Command::PrepareToolSource { package, output } => {
            distro_builder::recipe::tools::prepare_tool_source_into(repo, &package, &output)?;
            println!("{}", output.display());
            Ok(())
        }
        Command::PreseedRootfsSource {
            distro,
            flavour,
            refresh,
        } => artifacts::preseed_rootfs_source_cmd(repo, &distro, flavour, refresh),
    }
}
