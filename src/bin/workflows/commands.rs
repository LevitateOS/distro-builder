use anyhow::{bail, Context, Result};
use std::path::Path;

pub(crate) fn is_iso_build_invocation(args: &[String]) -> bool {
    matches!(
        args,
        [iso, build] | [iso, build, _] | [iso, build, _, _] if iso == "iso" && build == "build"
    )
}

pub(crate) fn run_iso_build_command(args: &[String]) -> Result<()> {
    let repo_root = crate::workflows::locate_repo_root()?;
    let build_args: Vec<&String> = match args {
        [_, _] => vec![],
        [_, _, arg1] => vec![arg1],
        [_, _, arg1, arg2] => vec![arg1, arg2],
        _ => bail!(crate::usage()),
    };

    let (distro_id, stage) = crate::workflows::parse_build_command(build_args, &repo_root)?;
    crate::workflows::preflight_iso_build(&repo_root, &distro_id, stage)?;
    crate::workflows::enforce_legacy_binding_policy_guard()?;
    crate::workflows::build_one(&distro_id, stage)
}

pub(crate) fn dispatch_non_iso_command(args: &[String]) -> Result<()> {
    let command = match args {
        [iso, build_all_cmd] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_stage(None)?)
        }
        [iso, build_all_cmd, stage] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_stage(Some(stage))?)
        }
        [artifact, build_rootfs, source_dir, output]
            if artifact == "artifact" && build_rootfs == "build-rootfs-erofs" =>
        {
            crate::workflows::build_rootfs_erofs(Path::new(source_dir), Path::new(output))
        }
        [artifact, build_overlay, source_dir, output]
            if artifact == "artifact" && build_overlay == "build-overlayfs-erofs" =>
        {
            crate::workflows::build_overlayfs_erofs(Path::new(source_dir), Path::new(output))
        }
        [artifact, prepare_stage, stage, distro, output_dir]
            if artifact == "artifact" && prepare_stage == "prepare-stage-inputs" =>
        {
            crate::workflows::prepare_stage_inputs_cmd(stage, distro, Path::new(output_dir))
        }
        [artifact, prepare_s01, distro, output_dir]
            if artifact == "artifact" && prepare_s01 == "prepare-s01-boot-inputs" =>
        {
            crate::workflows::prepare_stage_inputs_cmd(
                crate::STAGE01_CANONICAL,
                distro,
                Path::new(output_dir),
            )
        }
        [artifact, prepare_s02, distro, output_dir]
            if artifact == "artifact" && prepare_s02 == "prepare-s02-live-tools-inputs" =>
        {
            crate::workflows::prepare_stage_inputs_cmd(
                crate::STAGE02_CANONICAL,
                distro,
                Path::new(output_dir),
            )
        }
        [artifact, prepare_s00, distro, output_dir]
            if artifact == "artifact" && prepare_s00 == "prepare-s00-build-inputs" =>
        {
            crate::workflows::prepare_stage_inputs_cmd(
                crate::STAGE00_CANONICAL,
                distro,
                Path::new(output_dir),
            )
        }
        _ => bail!(crate::usage()),
    };
    command.with_context(|| format!("dispatching non-iso workflow for '{}'", args.join(" ")))
}
