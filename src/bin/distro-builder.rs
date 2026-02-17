use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use distro_builder::stages::s00_build::{
    ensure_kernel_installed_via_recipe, run_00build_evidence_script, S00BuildEvidenceSpec,
    S00BuildKernelEnsureOutcome, S00BuildKernelSpec,
};
use distro_builder::stages::s01_boot_inputs::{
    load_s00_build_input_spec, load_s01_boot_input_spec,
    prepare_s00_build_inputs as prepare_s00_build_inputs_for_distro,
    prepare_s01_boot_inputs as prepare_s01_boot_inputs_for_distro,
};
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_contract::{
    load_stage_00_contract_bundle_for_distro_from, require_valid_contract, LoadedVariantContract,
};

const STAGE00_NATIVE_BUILD_SCRIPT: &str = "00Build-build.sh";
const STAGE01_NATIVE_BUILD_SCRIPT: &str = "01Boot-build.sh";
const STAGE00_CANONICAL: &str = "00Build";
const STAGE00_SLUG: &str = "s00_build";
const STAGE00_DIRNAME: &str = "s00-build";
const STAGE01_CANONICAL: &str = "01Boot";
const STAGE01_SLUG: &str = "s01_boot";
const STAGE01_DIRNAME: &str = "s01-boot";

#[derive(Clone, Copy)]
struct BuildStage {
    canonical: &'static str,
    slug: &'static str,
    dir_name: &'static str,
}

fn usage() -> &'static str {
    "Usage:\n  distro-builder iso build <distro_id> [00Build|s00_build|s00-build|01Boot|s01_boot|s01-boot]\n  distro-builder iso build-all [00Build|s00_build|s00-build|01Boot|s01_boot|s01-boot]\n  distro-builder artifact build-rootfs-erofs <source_dir> <output>\n  distro-builder artifact build-overlayfs-erofs <source_dir> <output>\n  distro-builder artifact prepare-s00-build-inputs <distro_id> <output_dir>\n  distro-builder artifact prepare-s01-boot-inputs <distro_id> <output_dir>"
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
        [artifact, prepare_s01, distro, output_dir]
            if artifact == "artifact" && prepare_s01 == "prepare-s01-boot-inputs" =>
        {
            prepare_live_inputs_cmd(distro, Path::new(output_dir))
        }
        [artifact, prepare_s00, distro, output_dir]
            if artifact == "artifact" && prepare_s00 == "prepare-s00-build-inputs" =>
        {
            prepare_s00_build_inputs_cmd(distro, Path::new(output_dir))
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

fn prepare_live_inputs_cmd(distro_id: &str, output_dir: &Path) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;

    let s01_spec = load_s01_boot_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
        .with_context(|| format!("loading 01Boot config for '{}'", distro_id))?;
    let prepared = prepare_s01_boot_inputs_for_distro(&s01_spec, output_dir)
        .with_context(|| format!("preparing 01Boot inputs for '{}'", distro_id))?;

    let rootfs_source = format!("{}\n", prepared.rootfs_source_dir.display());
    let source_path_file = output_dir.join(".live-rootfs-source.path");
    std::fs::write(&source_path_file, &rootfs_source).with_context(|| {
        format!(
            "writing live rootfs source path file '{}'",
            source_path_file.display()
        )
    })?;

    println!("Live boot inputs prepared:");
    println!("  rootfs source: {}", prepared.rootfs_source_dir.display());
    println!("  live overlay:  {}", prepared.live_overlay_dir.display());
    println!("  source path:   {}", source_path_file.display());
    Ok(())
}

fn prepare_s00_build_inputs_cmd(distro_id: &str, output_dir: &Path) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;

    let output_root = bundle.repo_root.join(".artifacts/out");
    let s00_spec = load_s00_build_input_spec(
        distro_id,
        &bundle.contract.identity.os_name,
        &bundle.contract.identity.os_id,
        &output_root,
    )
    .with_context(|| format!("loading 00Build stage baseline for '{}'", distro_id))?;
    let prepared = prepare_s00_build_inputs_for_distro(&s00_spec, output_dir)
        .with_context(|| format!("preparing 00Build inputs for '{}'", distro_id))?;

    let rootfs_source = format!("{}\n", prepared.rootfs_source_dir.display());
    let source_path_file = output_dir.join(".live-rootfs-source.path");
    std::fs::write(&source_path_file, &rootfs_source).with_context(|| {
        format!(
            "writing Stage 00 rootfs source path file '{}'",
            source_path_file.display()
        )
    })?;

    println!("00Build inputs prepared:");
    println!("  rootfs source: {}", prepared.rootfs_source_dir.display());
    println!("  live overlay:  {}", prepared.live_overlay_dir.display());
    println!("  source path:   {}", source_path_file.display());
    Ok(())
}

fn parse_stage(value: Option<&str>) -> Result<BuildStage> {
    match value.unwrap_or(STAGE00_CANONICAL) {
        STAGE00_CANONICAL | STAGE00_SLUG | STAGE00_DIRNAME | "00build" | "00_BUILD"
        | "S00_BUILD" | "S00-BUILD" => Ok(BuildStage {
            canonical: STAGE00_CANONICAL,
            slug: STAGE00_SLUG,
            dir_name: STAGE00_DIRNAME,
        }),
        STAGE01_CANONICAL | STAGE01_SLUG | STAGE01_DIRNAME | "01boot" | "01_BOOT" | "S01_BOOT"
        | "S01-BOOT" => Ok(BuildStage {
            canonical: STAGE01_CANONICAL,
            slug: STAGE01_SLUG,
            dir_name: STAGE01_DIRNAME,
        }),
        other => bail!(
            "unsupported stage '{}'; expected one of: '{}', '{}', '{}', '{}', '{}', '{}'",
            other,
            STAGE00_CANONICAL,
            STAGE00_SLUG,
            STAGE00_DIRNAME,
            STAGE01_CANONICAL,
            STAGE01_SLUG,
            STAGE01_DIRNAME
        ),
    }
}

fn build_all(stage: BuildStage) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let distro_ids = discover_distro_ids(&cwd)?;
    for distro_id in &distro_ids {
        println!("[iso:{}] building {}...", stage.slug, distro_id);
        build_one(distro_id, stage)?;
    }
    Ok(())
}

fn build_one(distro_id: &str, stage: BuildStage) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{distro_id}'"))?;

    require_valid_contract(&bundle.contract)
        .with_context(|| format!("validating 00Build contract for '{distro_id}'"))?;

    let kernel_output_dir = output_dir_for(&bundle.repo_root, distro_id);
    std::fs::create_dir_all(&kernel_output_dir).with_context(|| {
        format!(
            "creating kernel output directory '{}'",
            kernel_output_dir.display()
        )
    })?;
    let stage_output_dir = stage_output_dir_for(&bundle.repo_root, distro_id, stage);
    std::fs::create_dir_all(&stage_output_dir).with_context(|| {
        format!(
            "creating stage output directory '{}'",
            stage_output_dir.display()
        )
    })?;

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

    match ensure_kernel_installed_via_recipe(
        &bundle.repo_root,
        distro_id,
        &kernel_output_dir,
        &kernel_spec,
    )
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

    ensure_iso_exists(
        &bundle,
        distro_id,
        &kernel_output_dir,
        &stage_output_dir,
        stage,
    )?;

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
        iso_filename: iso_filename_for_stage(&bundle.contract.artifacts.iso_filename, stage),
    };

    run_00build_evidence_script(
        &bundle.repo_root,
        &bundle.variant_dir,
        &kernel_output_dir,
        &stage_output_dir,
        &evidence_spec,
    )
    .with_context(|| format!("running 00Build evidence for '{distro_id}'"))?;

    println!(
        "[iso:{}:{distro_id}] stage {} passed; ISO at {}",
        stage.slug,
        stage.canonical,
        stage_output_dir
            .join(iso_filename_for_stage(
                &bundle.contract.artifacts.iso_filename,
                stage
            ))
            .display()
    );

    Ok(())
}

fn ensure_iso_exists(
    bundle: &LoadedVariantContract,
    distro_id: &str,
    kernel_output_dir: &Path,
    stage_output_dir: &Path,
    stage: BuildStage,
) -> Result<()> {
    let iso_filename = iso_filename_for_stage(&bundle.contract.artifacts.iso_filename, stage);
    let iso_path = stage_output_dir.join(&iso_filename);
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
        kernel_output_dir.join(&bundle.contract.stages.stage_00_build.kernel_release_path);
    let kernel_image_path =
        kernel_output_dir.join(&bundle.contract.stages.stage_00_build.kernel_image_path);

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
        .env("ISO_FILENAME", &iso_filename)
        .env("BUILD_STAGE_NAME", stage.canonical)
        .env("BUILD_STAGE_SLUG", stage.slug)
        .env("BUILD_STAGE_DIRNAME", stage.dir_name)
        .env("KERNEL_OUTPUT_DIR", kernel_output_dir)
        .env("STAGE_OUTPUT_DIR", stage_output_dir)
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
        STAGE01_SLUG => STAGE01_NATIVE_BUILD_SCRIPT,
        _ => unreachable!("validated in parse_stage"),
    }
}

fn iso_filename_for_stage(stage00_iso_filename: &str, stage: BuildStage) -> String {
    match stage.slug {
        STAGE00_SLUG => stage00_iso_filename.to_string(),
        STAGE01_SLUG => derive_s01_iso_filename(stage00_iso_filename),
        _ => unreachable!("validated in parse_stage"),
    }
}

fn derive_s01_iso_filename(stage00_iso_filename: &str) -> String {
    if stage00_iso_filename.contains(STAGE00_SLUG) {
        return stage00_iso_filename.replacen(STAGE00_SLUG, STAGE01_SLUG, 1);
    }
    if let Some(base) = stage00_iso_filename.strip_suffix(".iso") {
        return format!("{base}-{STAGE01_SLUG}.iso");
    }
    format!("{stage00_iso_filename}-{STAGE01_SLUG}.iso")
}

fn output_dir_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root.join(".artifacts/out").join(distro_id)
}

fn stage_output_dir_for(repo_root: &Path, distro_id: &str, stage: BuildStage) -> PathBuf {
    output_dir_for(repo_root, distro_id).join(stage.dir_name)
}

fn discover_distro_ids(repo_root: &Path) -> Result<Vec<String>> {
    let variants_dir = repo_root.join("distro-variants");
    let entries = std::fs::read_dir(&variants_dir)
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
