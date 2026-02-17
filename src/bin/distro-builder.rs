use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use distro_builder::stages::s00_build::{
    ensure_kernel_installed_via_recipe, run_00build_evidence_script, S00BuildEvidenceSpec,
    S00BuildKernelEnsureOutcome, S00BuildKernelSpec,
};
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_contract::{
    load_stage_00_contract_bundle_for_distro_from, require_valid_contract, LoadedVariantContract,
};

const DISTROS: &[&str] = &["levitate", "acorn", "iuppiter", "ralph"];
const STAGE00_NATIVE_BUILD_SCRIPT: &str = "00Build-build.sh";
const STAGE00_CANONICAL: &str = "00Build";
const STAGE00_SLUG: &str = "s00_build";

#[derive(Clone, Copy)]
struct BuildStage {
    canonical: &'static str,
    slug: &'static str,
}

fn usage() -> &'static str {
    "Usage:\n  distro-builder iso build <levitate|acorn|iuppiter|ralph> [00Build|s00_build]\n  distro-builder iso build-all [00Build|s00_build]\n  distro-builder artifact build-rootfs-erofs <source_dir> <output>\n  distro-builder artifact build-overlayfs-erofs <source_dir> <output>"
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.as_slice() {
        [iso, build, distro] if iso == "iso" && build == "build" => {
            build_one(distro, parse_stage(None)?)
        }
        [iso, build, distro, stage] if iso == "iso" && build == "build" => {
            build_one(distro, parse_stage(Some(stage))?)
        }
        [iso, build_all_cmd] if iso == "iso" && build_all_cmd == "build-all" => {
            build_all(parse_stage(None)?)
        }
        [iso, build_all_cmd, stage] if iso == "iso" && build_all_cmd == "build-all" => {
            build_all(parse_stage(Some(stage))?)
        }
        [artifact, build_rootfs, source_dir, output]
            if artifact == "artifact" && build_rootfs == "build-rootfs-erofs" =>
        {
            build_rootfs_erofs(Path::new(source_dir), Path::new(output))
        }
        [artifact, build_overlay, source_dir, output]
            if artifact == "artifact" && build_overlay == "build-overlayfs-erofs" =>
        {
            build_overlayfs_erofs(Path::new(source_dir), Path::new(output))
        }
        _ => bail!(usage()),
    }
}

fn build_rootfs_erofs(source_dir: &Path, output: &Path) -> Result<()> {
    build_erofs_default(source_dir, output).with_context(|| {
        format!(
            "building rootfs EROFS from '{}' to '{}'",
            source_dir.display(),
            output.display()
        )
    })
}

fn build_overlayfs_erofs(source_dir: &Path, output: &Path) -> Result<()> {
    build_overlayfs_default(source_dir, output).with_context(|| {
        format!(
            "building overlayfs EROFS from '{}' to '{}'",
            source_dir.display(),
            output.display()
        )
    })
}

fn parse_stage(value: Option<&str>) -> Result<BuildStage> {
    match value.unwrap_or(STAGE00_CANONICAL) {
        STAGE00_CANONICAL | STAGE00_SLUG | "00build" | "00_BUILD" | "S00_BUILD" => Ok(BuildStage {
            canonical: STAGE00_CANONICAL,
            slug: STAGE00_SLUG,
        }),
        other => bail!(
            "unsupported stage '{}'; expected '{}' (alias '{}')",
            other,
            STAGE00_CANONICAL,
            STAGE00_SLUG
        ),
    }
}

fn build_all(stage: BuildStage) -> Result<()> {
    for distro_id in DISTROS {
        println!("[iso:{}] building {distro_id}...", stage.slug);
        build_one(distro_id, stage)?;
    }
    Ok(())
}

fn build_one(distro_id: &str, stage: BuildStage) -> Result<()> {
    ensure_supported_distro(distro_id)?;

    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{distro_id}'"))?;

    require_valid_contract(&bundle.contract)
        .with_context(|| format!("validating 00Build contract for '{distro_id}'"))?;

    let output_dir = output_dir_for(&bundle.repo_root, distro_id);
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("creating output directory '{}'", output_dir.display()))?;

    let kernel_spec = S00BuildKernelSpec {
        recipe_kernel_script: bundle
            .contract
            .stages
            .stage_00_build
            .recipe_kernel_script
            .clone(),
        kernel_version: bundle.contract.stages.stage_00_build.kernel_version.clone(),
        kernel_sha256: bundle.contract.stages.stage_00_build.kernel_sha256.clone(),
        kernel_localversion: bundle
            .contract
            .stages
            .stage_00_build
            .kernel_localversion
            .clone(),
        module_install_path: bundle
            .contract
            .stages
            .stage_00_build
            .module_install_path
            .clone(),
    };

    match ensure_kernel_installed_via_recipe(&bundle.repo_root, &output_dir, &kernel_spec)
        .with_context(|| format!("ensuring kernel artifacts for '{distro_id}'"))?
    {
        S00BuildKernelEnsureOutcome::AlreadyInstalled => {
            println!("[iso:{}:{distro_id}] kernel already installed", stage.slug);
        }
        S00BuildKernelEnsureOutcome::InstalledNow => {
            println!(
                "[iso:{}:{distro_id}] kernel installed via recipe",
                stage.slug
            );
        }
    }

    ensure_iso_exists(&bundle, distro_id, &output_dir, stage)?;

    let evidence_spec = S00BuildEvidenceSpec {
        script_path: bundle
            .contract
            .stages
            .stage_00_build
            .evidence
            .script_path
            .clone(),
        pass_marker: bundle
            .contract
            .stages
            .stage_00_build
            .evidence
            .pass_marker
            .clone(),
        kernel_release_path: bundle
            .contract
            .stages
            .stage_00_build
            .kernel_release_path
            .clone(),
        kernel_image_path: bundle
            .contract
            .stages
            .stage_00_build
            .kernel_image_path
            .clone(),
        iso_filename: bundle.contract.artifacts.iso_filename.clone(),
    };

    run_00build_evidence_script(
        &bundle.repo_root,
        &bundle.variant_dir,
        &output_dir,
        &evidence_spec,
    )
    .with_context(|| format!("running 00Build evidence for '{distro_id}'"))?;

    println!(
        "[iso:{}:{distro_id}] stage {} passed; ISO at {}",
        stage.slug,
        stage.canonical,
        output_dir
            .join(&bundle.contract.artifacts.iso_filename)
            .display()
    );

    Ok(())
}

fn ensure_iso_exists(
    bundle: &LoadedVariantContract,
    distro_id: &str,
    output_dir: &Path,
    stage: BuildStage,
) -> Result<()> {
    let iso_path = output_dir.join(&bundle.contract.artifacts.iso_filename);
    let native_build = bundle.variant_dir.join(native_build_script_filename(stage));
    if !native_build.is_file() {
        bail!(
            "missing variant-native {} build hook for '{}': {}\n\
             legacy crate entrypoints are blocked by policy.\n\
             add '{}' under {} and implement ISO assembly there.",
            stage.canonical,
            distro_id,
            native_build.display(),
            native_build_script_filename(stage),
            bundle.variant_dir.display()
        );
    }

    let kernel_release_path =
        output_dir.join(&bundle.contract.stages.stage_00_build.kernel_release_path);
    let kernel_image_path =
        output_dir.join(&bundle.contract.stages.stage_00_build.kernel_image_path);

    if iso_path.is_file() {
        println!(
            "[iso:{}:{distro_id}] ISO exists; rebuilding via {}",
            stage.slug,
            native_build.display()
        );
    } else {
        println!(
            "[iso:{}:{distro_id}] ISO missing; invoking builder via {}",
            stage.slug,
            native_build.display()
        );
    }

    let distro_builder_bin =
        std::env::current_exe().context("resolving distro-builder executable path")?;

    let status = Command::new("sh")
        .arg(&native_build)
        .current_dir(&bundle.repo_root)
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .env("ISO_FILENAME", &bundle.contract.artifacts.iso_filename)
        .env("BUILD_STAGE_NAME", stage.canonical)
        .env("BUILD_STAGE_SLUG", stage.slug)
        .env("DISTRO_BUILDER_BIN", &distro_builder_bin)
        .status()
        .with_context(|| {
            format!(
                "running {} native build hook for '{}' using {}",
                stage.canonical,
                distro_id,
                native_build.display()
            )
        })?;

    if !status.success() {
        bail!("builder command failed for '{distro_id}' with status {status}");
    }

    if !iso_path.is_file() {
        bail!(
            "builder finished but ISO still missing for '{}': {}",
            distro_id,
            iso_path.display()
        );
    }

    Ok(())
}

fn native_build_script_filename(stage: BuildStage) -> &'static str {
    match stage.slug {
        STAGE00_SLUG => STAGE00_NATIVE_BUILD_SCRIPT,
        _ => unreachable!("validated in parse_stage"),
    }
}

fn ensure_supported_distro(distro_id: &str) -> Result<()> {
    if DISTROS.contains(&distro_id) {
        return Ok(());
    }

    bail!(
        "unsupported distro '{}'; expected one of: {}",
        distro_id,
        DISTROS.join(", ")
    )
}

fn output_dir_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root.join(".artifacts/out").join(match distro_id {
        "levitate" => "levitate",
        "acorn" => "acorn",
        "iuppiter" => "iuppiter",
        "ralph" => "ralph",
        _ => unreachable!("validated in ensure_supported_distro"),
    })
}
