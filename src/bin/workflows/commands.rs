use anyhow::{bail, Context, Result};
use std::path::Path;

pub(crate) fn is_release_build_invocation(args: &[String]) -> bool {
    matches!(
        args,
        [release, build, iso]
            | [release, build, iso, _]
            | [release, build, iso, _, _]
                if release == "release" && build == "build" && iso == "iso"
    ) || matches!(
        args,
        [iso, build] | [iso, build, _] | [iso, build, _, _] if iso == "iso" && build == "build"
    )
}

pub(crate) fn run_release_build_command(args: &[String]) -> Result<()> {
    let repo_root = crate::workflows::locate_repo_root()?;
    let build_args: Vec<&String> = match args {
        [release, build, iso] if release == "release" && build == "build" && iso == "iso" => {
            vec![]
        }
        [release, build, iso, arg1] if release == "release" && build == "build" && iso == "iso" => {
            vec![arg1]
        }
        [release, build, iso, arg1, arg2]
            if release == "release" && build == "build" && iso == "iso" =>
        {
            vec![arg1, arg2]
        }
        [iso, build] if iso == "iso" && build == "build" => vec![],
        [iso, build, arg1] if iso == "iso" && build == "build" => vec![arg1],
        [iso, build, arg1, arg2] if iso == "iso" && build == "build" => vec![arg1, arg2],
        _ => bail!(crate::usage()),
    };

    let (distro_id, product) =
        crate::workflows::parse_release_build_command(build_args, &repo_root)?;
    crate::workflows::preflight_iso_build(&repo_root, &distro_id, product)?;
    crate::workflows::enforce_legacy_binding_policy_guard()?;
    crate::workflows::build_one(&distro_id, product)
}

pub(crate) fn dispatch_non_release_command(args: &[String]) -> Result<()> {
    let command = match args {
        [release, build_all_cmd, iso]
            if release == "release" && build_all_cmd == "build-all" && iso == "iso" =>
        {
            crate::workflows::build_all(crate::workflows::parse_product(None)?)
        }
        [release, build_all_cmd, iso, product]
            if release == "release" && build_all_cmd == "build-all" && iso == "iso" =>
        {
            crate::workflows::build_all(crate::workflows::parse_product(Some(product))?)
        }
        [iso, build_all_cmd] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_product(None)?)
        }
        [iso, build_all_cmd, stage] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_product(Some(stage))?)
        }
        [product, prepare, product_name, distro, output_dir]
            if product == "product" && prepare == "prepare" =>
        {
            crate::workflows::prepare_product_cmd(product_name, distro, Path::new(output_dir))
        }
        [transform, build, rootfs, source_dir, output]
            if transform == "transform" && build == "build" && rootfs == "rootfs-erofs" =>
        {
            crate::workflows::build_rootfs_erofs(Path::new(source_dir), Path::new(output))
        }
        [transform, build, overlay, source_dir, output]
            if transform == "transform" && build == "build" && overlay == "overlayfs-erofs" =>
        {
            crate::workflows::build_overlayfs_erofs(Path::new(source_dir), Path::new(output))
        }
        [transform, build, product_erofs, product_name, distro]
            if transform == "transform" && build == "build" && product_erofs == "product-erofs" =>
        {
            crate::workflows::build_product_erofs_cmd(product_name, distro)
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
        [artifact, build_stage_erofs, stage, distro]
            if artifact == "artifact" && build_stage_erofs == "build-stage-erofs" =>
        {
            crate::workflows::build_stage_erofs_cmd(stage, distro)
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
        [artifact, preseed_stage01, distro]
            if artifact == "artifact" && preseed_stage01 == "preseed-stage01-source" =>
        {
            crate::workflows::preseed_stage01_source_cmd(distro, false)
        }
        [artifact, preseed_stage01, distro, refresh]
            if artifact == "artifact"
                && preseed_stage01 == "preseed-stage01-source"
                && refresh == "--refresh" =>
        {
            crate::workflows::preseed_stage01_source_cmd(distro, true)
        }
        [artifact, materialize_stage01, distro]
            if artifact == "artifact"
                && materialize_stage01 == "materialize-stage01-source-rootfs" =>
        {
            crate::workflows::materialize_stage01_source_rootfs_cmd(distro)
        }
        _ => bail!(crate::usage()),
    };
    command.with_context(|| format!("dispatching workflow for '{}'", args.join(" ")))
}
