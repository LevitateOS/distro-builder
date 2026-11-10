//! distro-builder: the command line over the library. Its one grammar is
//! `distro_builder::cli`; each command calls one workflow. A usage error exits 2,
//! a failed command exits 1.

use anyhow::{Context, Result};
use distro_builder::cli::{self, Command};
use distro_builder::process::{self, HostNeed};
use std::path::Path;
use std::process::ExitCode;

mod workflows;

use workflows::{artifacts, build};

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
        Command::ReleaseBuild { product, distro } => {
            process::check_host_tools(HostNeed::Build)?;
            build::ensure_release_prerequisites(repo, &distro, product)?;
            build::build_one(repo, &distro, product)
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
