use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
use distro_builder::stages::s02_live_tools_inputs::{
    load_s02_live_tools_input_spec,
    prepare_s02_live_tools_inputs as prepare_s02_live_tools_inputs_for_distro,
};
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_contract::{
    load_stage_00_contract_bundle_for_distro_from, require_valid_contract, LoadedVariantContract,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const STAGE00_NATIVE_BUILD_SCRIPT: &str = "00Build-build.sh";
const STAGE01_NATIVE_BUILD_SCRIPT: &str = "01Boot-build.sh";
const STAGE02_NATIVE_BUILD_SCRIPT: &str = "02LiveTools-build.sh";
const STAGE00_CANONICAL: &str = "00Build";
const STAGE00_SLUG: &str = "s00_build";
const STAGE00_DIRNAME: &str = "s00-build";
const STAGE00_ARTIFACT_TAG: &str = "s00";
const STAGE01_CANONICAL: &str = "01Boot";
const STAGE01_SLUG: &str = "s01_boot";
const STAGE01_DIRNAME: &str = "s01-boot";
const STAGE01_ARTIFACT_TAG: &str = "s01";
const STAGE02_CANONICAL: &str = "02LiveTools";
const STAGE02_SLUG: &str = "s02_live_tools";
const STAGE02_DIRNAME: &str = "s02-live-tools";
const STAGE02_ARTIFACT_TAG: &str = "s02";
const DEFAULT_DISTRO_ID: &str = "levitate";
const S00_RUN_RETENTION_COUNT: usize = 5;
const RUN_MANIFEST_FILENAME: &str = "run-manifest.json";

#[derive(Clone, Copy)]
struct BuildStage {
    canonical: &'static str,
    slug: &'static str,
    dir_name: &'static str,
}

#[derive(Debug, Clone)]
struct StageOutputLayout {
    stage_root_dir: PathBuf,
    stage_output_dir: PathBuf,
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct StageRunMetadata {
    run_id: String,
    distro_id: String,
    stage_name: String,
    stage_slug: String,
    status: String,
    created_at_utc: String,
    finished_at_utc: Option<String>,
    stage_root_dir: String,
    stage_output_dir: String,
    iso_path: String,
}

#[derive(Debug, Deserialize)]
struct StageRunMetadataFile {
    run_id: String,
    status: String,
    created_at_utc: String,
    finished_at_utc: Option<String>,
}

fn usage() -> &'static str {
    "Usage:\n  distro-builder iso build [<distro_id|stage>] [<distro_id|stage>] \n    stage defaults to 00Build, distro defaults to levitate\n    stage aliases: 0|00|01|1|02|2\n  distro-builder iso build-all [00Build|01Boot|02LiveTools]\n  distro-builder artifact build-rootfs-erofs <source_dir> <output>\n  distro-builder artifact build-overlayfs-erofs <source_dir> <output>\n  distro-builder artifact prepare-stage-inputs <stage> <distro_id> <output_dir>\n  distro-builder artifact prepare-s00-build-inputs <distro_id> <output_dir>\n  distro-builder artifact prepare-s01-boot-inputs <distro_id> <output_dir>\n  distro-builder artifact prepare-s02-live-tools-inputs <distro_id> <output_dir>"
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if is_iso_build_invocation(&args) {
        return run_iso_build_command(&args);
    }

    enforce_legacy_binding_policy_guard()?;

    match args.as_slice() {
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
        [artifact, prepare_stage, stage, distro, output_dir]
            if artifact == "artifact" && prepare_stage == "prepare-stage-inputs" =>
        {
            prepare_stage_inputs_cmd(stage, distro, Path::new(output_dir))
        }
        [artifact, prepare_s01, distro, output_dir]
            if artifact == "artifact" && prepare_s01 == "prepare-s01-boot-inputs" =>
        {
            prepare_stage_inputs_cmd(STAGE01_CANONICAL, distro, Path::new(output_dir))
        }
        [artifact, prepare_s02, distro, output_dir]
            if artifact == "artifact" && prepare_s02 == "prepare-s02-live-tools-inputs" =>
        {
            prepare_stage_inputs_cmd(STAGE02_CANONICAL, distro, Path::new(output_dir))
        }
        [artifact, prepare_s00, distro, output_dir]
            if artifact == "artifact" && prepare_s00 == "prepare-s00-build-inputs" =>
        {
            prepare_stage_inputs_cmd(STAGE00_CANONICAL, distro, Path::new(output_dir))
        }
        _ => bail!(usage()),
    }
}

fn is_iso_build_invocation(args: &[String]) -> bool {
    matches!(
        args,
        [iso, build] | [iso, build, _] | [iso, build, _, _] if iso == "iso" && build == "build"
    )
}

fn run_iso_build_command(args: &[String]) -> Result<()> {
    let repo_root = locate_repo_root()?;
    let build_args: Vec<&String> = match args {
        [_, _] => vec![],
        [_, _, arg1] => vec![arg1],
        [_, _, arg1, arg2] => vec![arg1, arg2],
        _ => bail!(usage()),
    };

    let (distro_id, stage) = parse_build_command(build_args, &repo_root)?;
    preflight_iso_build(&repo_root, &distro_id, stage)?;
    enforce_legacy_binding_policy_guard()?;
    build_one(&distro_id, stage)
}

fn preflight_iso_build(repo_root: &Path, distro_id: &str, stage: BuildStage) -> Result<()> {
    if stage.slug == STAGE00_SLUG {
        return Ok(());
    }

    if stage.slug == STAGE01_SLUG {
        let s00_root = repo_root
            .join(".artifacts/out")
            .join(distro_id)
            .join(STAGE00_DIRNAME);
        let run_id = latest_successful_stage_run_id(&s00_root)?.ok_or_else(|| {
            anyhow::anyhow!(
                "preflight failed for '{}' {}: no successful Stage 00 runs found under '{}'.\n\
                 Build Stage 00 first: `just build 0 {}`",
                distro_id,
                stage.canonical,
                s00_root.display(),
                distro_id
            )
        })?;
        let parent_rootfs = s00_root.join(&run_id).join("s00-filesystem.erofs");
        if !parent_rootfs.is_file() {
            bail!(
                "preflight failed for '{}' {}: missing Stage 00 rootfs image '{}'.\n\
                 Build Stage 00 first: `just build 0 {}`",
                distro_id,
                stage.canonical,
                parent_rootfs.display(),
                distro_id
            );
        }
    }

    Ok(())
}

fn enforce_legacy_binding_policy_guard() -> Result<()> {
    let repo_root = locate_repo_root()?;
    let status = Command::new("cargo")
        .current_dir(&repo_root)
        .args(["xtask", "policy", "audit-legacy-bindings"])
        .status()
        .context("running legacy-binding policy guard via `cargo xtask`")?;

    if status.success() {
        return Ok(());
    }

    bail!(
        "policy guard failed before distro-builder execution (exit: {}). \
Run `cargo xtask policy audit-legacy-bindings` and fix violations first.",
        status
    )
}

fn locate_repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in manifest_dir.ancestors() {
        let candidate = Path::new(ancestor);
        if candidate.join("xtask").is_dir() && candidate.join("distro-variants").is_dir() {
            return Ok(candidate.to_path_buf());
        }
    }
    bail!(
        "unable to locate repository root from '{}' for policy guard",
        manifest_dir.display()
    )
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

fn prepare_stage_inputs_cmd(stage: &str, distro_id: &str, output_dir: &Path) -> Result<()> {
    let stage = parse_stage(Some(stage))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;

    let (prepared_rootfs_source, prepared_live_overlay, stage_label, stage_artifact_tag) =
        match stage.slug {
            STAGE00_SLUG => {
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
                (
                    prepared.rootfs_source_dir,
                    prepared.live_overlay_dir,
                    STAGE00_CANONICAL,
                    STAGE00_ARTIFACT_TAG,
                )
            }
            STAGE01_SLUG => {
                let s01_spec =
                    load_s01_boot_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
                        .with_context(|| format!("loading 01Boot config for '{}'", distro_id))?;
                let prepared = prepare_s01_boot_inputs_for_distro(&s01_spec, output_dir)
                    .with_context(|| format!("preparing 01Boot inputs for '{}'", distro_id))?;
                (
                    prepared.rootfs_source_dir,
                    prepared.live_overlay_dir,
                    STAGE01_CANONICAL,
                    STAGE01_ARTIFACT_TAG,
                )
            }
            STAGE02_SLUG => {
                let s02_spec = load_s02_live_tools_input_spec(
                    &bundle.repo_root,
                    &bundle.variant_dir,
                    distro_id,
                )
                .with_context(|| format!("loading 02LiveTools config for '{}'", distro_id))?;
                let prepared = prepare_s02_live_tools_inputs_for_distro(&s02_spec, output_dir)
                    .with_context(|| format!("preparing 02LiveTools inputs for '{}'", distro_id))?;
                (
                    prepared.rootfs_source_dir,
                    prepared.live_overlay_dir,
                    STAGE02_CANONICAL,
                    STAGE02_ARTIFACT_TAG,
                )
            }
            _ => unreachable!("validated in parse_stage"),
        };

    let rootfs_source = format!("{}\n", prepared_rootfs_source.display());
    let source_path_file =
        output_dir.join(format!(".{}-live-rootfs-source.path", stage_artifact_tag));
    std::fs::write(&source_path_file, &rootfs_source).with_context(|| {
        format!(
            "writing Stage {} rootfs source path file '{}'",
            stage_label,
            source_path_file.display()
        )
    })?;

    println!("{} inputs prepared:", stage_label);
    println!("  rootfs source: {}", prepared_rootfs_source.display());
    println!("  live overlay:  {}", prepared_live_overlay.display());
    println!("  source path:   {}", source_path_file.display());
    Ok(())
}

fn parse_build_command(args: Vec<&String>, repo_root: &Path) -> Result<(String, BuildStage)> {
    let known_distros = discover_distro_ids(repo_root)?;

    match args.as_slice() {
        [] => Ok((DEFAULT_DISTRO_ID.to_string(), parse_stage(None)?)),
        [arg] => parse_build_one_arg(arg, &known_distros),
        [arg1, arg2] => parse_build_two_args(arg1, arg2, &known_distros),
        _ => bail!(
            "unsupported positional arguments for `iso build`; expected `[stage_or_distro] [stage_or_distro]`, \
             max 2 args"
        ),
    }
}

fn parse_build_one_arg(arg: &str, known_distros: &[String]) -> Result<(String, BuildStage)> {
    if let Ok(distro_id) = parse_distro_id(arg, known_distros) {
        return Ok((distro_id, parse_stage(None)?));
    }

    let stage = parse_stage(Some(arg))?;
    Ok((DEFAULT_DISTRO_ID.to_string(), stage))
}

fn parse_build_two_args(
    arg1: &str,
    arg2: &str,
    known_distros: &[String],
) -> Result<(String, BuildStage)> {
    if let Ok(distro_id) = parse_distro_id(arg1, known_distros) {
        if let Ok(stage) = parse_stage(Some(arg2)) {
            return Ok((distro_id, stage));
        }
    }

    if let Ok(stage) = parse_stage(Some(arg1)) {
        let distro_id = parse_distro_id(arg2, known_distros)?;
        return Ok((distro_id, stage));
    }

    let known_distros = known_distros.join(", ");
    bail!(
        "invalid `iso build` arguments: '{}' '{}'. Expected `<distro> <stage>` or `<stage> <distro>`.\n\
         `stage` supports aliases: 0, 00, 01, 1, 02, 2.\n\
         available distros: {}",
        arg1,
        arg2,
        known_distros
    )
}

fn parse_distro_id(value: &str, known_distros: &[String]) -> Result<String> {
    if let Some(distro_id) = known_distros.iter().find(|d| d.as_str() == value) {
        return Ok(distro_id.to_string());
    }

    let supported = known_distros.join(", ");
    bail!(
        "unsupported distro '{}'; expected one of: {}",
        value,
        supported
    )
}

fn parse_stage(value: Option<&str>) -> Result<BuildStage> {
    match value.unwrap_or(STAGE00_CANONICAL) {
        STAGE00_CANONICAL | "0" | "00" => Ok(BuildStage {
            canonical: STAGE00_CANONICAL,
            slug: STAGE00_SLUG,
            dir_name: STAGE00_DIRNAME,
        }),
        STAGE01_CANONICAL | "1" | "01" => Ok(BuildStage {
            canonical: STAGE01_CANONICAL,
            slug: STAGE01_SLUG,
            dir_name: STAGE01_DIRNAME,
        }),
        STAGE02_CANONICAL | "2" | "02" => Ok(BuildStage {
            canonical: STAGE02_CANONICAL,
            slug: STAGE02_SLUG,
            dir_name: STAGE02_DIRNAME,
        }),
        other => bail!(
            "unsupported stage '{}'; expected one of: '{}', '{}', '{}'; aliases: 0|00|01|1, 02|2",
            other,
            STAGE00_CANONICAL,
            STAGE01_CANONICAL,
            STAGE02_CANONICAL
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

    let kernel_output_dir = kernel_output_dir_for(&bundle.repo_root, distro_id);
    std::fs::create_dir_all(&kernel_output_dir).with_context(|| {
        format!(
            "creating kernel output directory '{}'",
            kernel_output_dir.display()
        )
    })?;
    let stage_layout = stage_output_layout_for(&bundle.repo_root, distro_id, stage)?;
    let stage_output_dir = stage_layout.stage_output_dir.clone();

    let kernel_spec = S00BuildKernelSpec {
        recipe_kernel_script: bundle
            .contract
            .stages
            .stage_00_build
            .recipe_kernel_script
            .clone(),
        kernel_kconfig_path: bundle
            .contract
            .stages
            .stage_00_build
            .kernel_kconfig_path
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

    let created_at_utc = now_utc_compact()?;
    let iso_path = stage_output_dir.join(iso_filename_for_stage(
        &bundle.contract.artifacts.iso_filename,
        stage,
    ));

    if let Some(run_id) = stage_layout.run_id.as_deref() {
        let metadata_path = stage_run_manifest_path(&stage_output_dir);
        write_stage_run_metadata(
            &metadata_path,
            &StageRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                stage_name: stage.canonical.to_string(),
                stage_slug: stage.slug.to_string(),
                status: "building".to_string(),
                created_at_utc: created_at_utc.clone(),
                finished_at_utc: None,
                stage_root_dir: stage_layout.stage_root_dir.display().to_string(),
                stage_output_dir: stage_output_dir.display().to_string(),
                iso_path: iso_path.display().to_string(),
            },
        )?;
    }

    let build_result = (|| -> Result<()> {
        match ensure_kernel_installed_via_recipe(
            &bundle.repo_root,
            &bundle.variant_dir,
            distro_id,
            &kernel_output_dir,
            &kernel_spec,
        )
        .with_context(|| format!("ensuring kernel artifacts for '{distro_id}'"))?
        {
            S00BuildKernelEnsureOutcome::AlreadyInstalled => {
                println!("[iso:{}:{distro_id}] kernel already installed", stage.slug);
            }
        }
        ensure_iso_exists(&bundle, distro_id, &kernel_output_dir, &stage_layout, stage)?;

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
    })();

    if let Some(run_id) = stage_layout.run_id.as_deref() {
        let metadata_path = stage_run_manifest_path(&stage_output_dir);
        let finished_at_utc = Some(now_utc_compact()?);
        let status = if build_result.is_ok() {
            "success".to_string()
        } else {
            "failed".to_string()
        };
        let metadata_result = write_stage_run_metadata(
            &metadata_path,
            &StageRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                stage_name: stage.canonical.to_string(),
                stage_slug: stage.slug.to_string(),
                status,
                created_at_utc,
                finished_at_utc,
                stage_root_dir: stage_layout.stage_root_dir.display().to_string(),
                stage_output_dir: stage_output_dir.display().to_string(),
                iso_path: iso_path.display().to_string(),
            },
        );
        if let Err(err) = metadata_result {
            if build_result.is_ok() {
                return Err(err);
            }
            eprintln!(
                "[iso:{}:{distro_id}] warning: failed to persist stage run metadata: {err:#}",
                stage.slug
            );
        }

        if build_result.is_ok() {
            prune_old_stage_runs(&stage_layout.stage_root_dir, S00_RUN_RETENTION_COUNT)?;
        }
    }

    build_result
}

fn ensure_iso_exists(
    bundle: &LoadedVariantContract,
    distro_id: &str,
    kernel_output_dir: &Path,
    stage_layout: &StageOutputLayout,
    stage: BuildStage,
) -> Result<()> {
    let stage_output_dir = &stage_layout.stage_output_dir;
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
        .env("DISTRO_ID", distro_id)
        .env("IDENTITY_OS_NAME", &bundle.contract.identity.os_name)
        .env("IDENTITY_OS_ID", &bundle.contract.identity.os_id)
        .env("IDENTITY_OS_VERSION", &bundle.contract.identity.os_version)
        .env("IDENTITY_ISO_LABEL", &bundle.contract.identity.iso_label)
        .env(
            "S00_LIVE_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .live_uki_filename,
        )
        .env(
            "S00_EMERGENCY_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .emergency_uki_filename,
        )
        .env(
            "S00_DEBUG_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .debug_uki_filename,
        )
        .env(
            "S00_LIVE_CMDLINE",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .live_cmdline,
        )
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .env("ISO_FILENAME", &iso_filename)
        .env("BUILD_STAGE_NAME", stage.canonical)
        .env("BUILD_STAGE_SLUG", stage.slug)
        .env("BUILD_STAGE_DIRNAME", stage.dir_name)
        .env("STAGE_ROOT_DIR", &stage_layout.stage_root_dir)
        .env("STAGE_RUN_DIR", stage_output_dir)
        .env(
            "STAGE_REQUIRED_KERNEL_CMDLINE",
            stage_required_kernel_cmdline(bundle, stage),
        )
        .env("KERNEL_OUTPUT_DIR", kernel_output_dir)
        .env("STAGE_OUTPUT_DIR", stage_output_dir)
        .env("BUILD_RUN_ID", stage_layout.run_id.as_deref().unwrap_or(""))
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

fn stage_required_kernel_cmdline(bundle: &LoadedVariantContract, stage: BuildStage) -> String {
    match stage.slug {
        STAGE01_SLUG | STAGE02_SLUG => bundle
            .contract
            .stages
            .stage_01_live_boot
            .required_kernel_cmdline
            .join(" "),
        _ => String::new(),
    }
}

fn native_build_script_filename(stage: BuildStage) -> &'static str {
    match stage.slug {
        STAGE00_SLUG => STAGE00_NATIVE_BUILD_SCRIPT,
        STAGE01_SLUG => STAGE01_NATIVE_BUILD_SCRIPT,
        STAGE02_SLUG => STAGE02_NATIVE_BUILD_SCRIPT,
        _ => unreachable!("validated in parse_stage"),
    }
}

fn iso_filename_for_stage(stage00_iso_filename: &str, stage: BuildStage) -> String {
    match stage.slug {
        STAGE00_SLUG => stage00_iso_filename.to_string(),
        STAGE01_SLUG | STAGE02_SLUG => derive_stage_iso_filename(stage00_iso_filename, stage.slug),
        _ => unreachable!("validated in parse_stage"),
    }
}

fn derive_stage_iso_filename(stage00_iso_filename: &str, stage_slug: &str) -> String {
    if stage00_iso_filename.contains(STAGE00_SLUG) {
        return stage00_iso_filename.replacen(STAGE00_SLUG, stage_slug, 1);
    }
    if let Some(base) = stage00_iso_filename.strip_suffix(".iso") {
        return format!("{base}-{stage_slug}.iso");
    }
    format!("{stage00_iso_filename}-{stage_slug}.iso")
}

fn output_dir_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root.join(".artifacts/out").join(distro_id)
}

fn stage_output_dir_for(repo_root: &Path, distro_id: &str, stage: BuildStage) -> PathBuf {
    output_dir_for(repo_root, distro_id).join(stage.dir_name)
}

fn kernel_output_dir_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root
        .join(".artifacts/kernel")
        .join(distro_id)
        .join("current")
}

fn stage_output_layout_for(
    repo_root: &Path,
    distro_id: &str,
    stage: BuildStage,
) -> Result<StageOutputLayout> {
    let stage_root_dir = stage_output_dir_for(repo_root, distro_id, stage);
    fs::create_dir_all(&stage_root_dir).with_context(|| {
        format!(
            "creating stage output root directory '{}'",
            stage_root_dir.display()
        )
    })?;

    if stage.slug != STAGE00_SLUG {
        return Ok(StageOutputLayout {
            stage_root_dir: stage_root_dir.clone(),
            stage_output_dir: stage_root_dir,
            run_id: None,
        });
    }

    let (run_id, run_root) = allocate_stage_run_dir(&stage_root_dir)?;

    Ok(StageOutputLayout {
        stage_root_dir,
        stage_output_dir: run_root,
        run_id: Some(run_id),
    })
}

fn generate_stage_run_id() -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX_EPOCH while generating stage run id")?
        .as_nanos();
    let entropy = nanos ^ ((std::process::id() as u128) << 32);
    let suffix_full = base62_encode_u128(entropy);
    let suffix = if suffix_full.len() > 10 {
        &suffix_full[suffix_full.len() - 10..]
    } else {
        &suffix_full
    };
    Ok(suffix.to_string())
}

fn base62_encode_u128(mut value: u128) -> String {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut bytes = Vec::new();
    while value > 0 {
        let idx = (value % 62) as usize;
        bytes.push(ALPHABET[idx] as char);
        value /= 62;
    }
    bytes.iter().rev().collect()
}

fn now_utc_compact() -> Result<String> {
    let now = OffsetDateTime::now_utc();
    Ok(format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    ))
}

fn write_stage_run_metadata(path: &Path, metadata: &StageRunMetadata) -> Result<()> {
    write_json_atomic(path, metadata)
        .with_context(|| format!("writing stage run metadata '{}'", path.display()))
}

fn stage_run_manifest_path(stage_run_dir: &Path) -> PathBuf {
    stage_run_dir.join(RUN_MANIFEST_FILENAME)
}

fn prune_old_stage_runs(stage_root_dir: &Path, keep: usize) -> Result<()> {
    let mut runs = load_stage_runs_metadata(stage_root_dir)?;
    runs.sort_by_key(|run| Reverse(run_sort_key(run)));
    for run in runs.into_iter().skip(keep) {
        let path = stage_root_dir.join(&run.run_id);
        fs::remove_dir_all(&path).with_context(|| {
            format!("removing expired stage run directory '{}'", path.display())
        })?;
    }
    Ok(())
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path without parent '{}'", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating parent directory '{}'", parent.display()))?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let payload =
        serde_json::to_vec_pretty(value).with_context(|| "serializing stage run metadata")?;
    fs::write(&tmp, payload).with_context(|| format!("writing temp file '{}'", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "renaming temp file '{}' to '{}'",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn allocate_stage_run_dir(stage_root_dir: &Path) -> Result<(String, PathBuf)> {
    for _ in 0..32 {
        let run_id = generate_stage_run_id()?;
        let run_root = stage_root_dir.join(&run_id);
        if run_root.exists() {
            continue;
        }
        fs::create_dir_all(&run_root).with_context(|| {
            format!(
                "creating stage run output directory '{}'",
                run_root.display()
            )
        })?;
        return Ok((run_id, run_root));
    }
    bail!(
        "failed allocating unique stage run directory under '{}'",
        stage_root_dir.display()
    )
}

fn latest_successful_stage_run_id(stage_root_dir: &Path) -> Result<Option<String>> {
    let mut runs = load_stage_runs_metadata(stage_root_dir)?;
    runs.retain(|r| r.status == "success");
    runs.sort_by_key(|run| Reverse(run_sort_key(run)));
    Ok(runs.first().map(|r| r.run_id.clone()))
}

fn run_sort_key(run: &StageRunMetadataFile) -> String {
    run.finished_at_utc
        .clone()
        .unwrap_or_else(|| run.created_at_utc.clone())
}

fn load_stage_runs_metadata(stage_root_dir: &Path) -> Result<Vec<StageRunMetadataFile>> {
    if !stage_root_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(stage_root_dir).with_context(|| {
        format!(
            "reading stage runs directory '{}'",
            stage_root_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "iterating stage runs directory '{}'",
                stage_root_dir.display()
            )
        })?;
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }
        let Some(run_name) = run_dir.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        if run_name.starts_with('.') {
            continue;
        }
        let path = stage_run_manifest_path(&run_dir);
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("reading stage run metadata '{}'", path.display()))?;
        let parsed: StageRunMetadataFile = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing stage run metadata '{}'", path.display()))?;
        runs.push(parsed);
    }
    Ok(runs)
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
