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
            crate::workflows::build_all(crate::workflows::parse_release_product(None)?)
        }
        [release, build_all_cmd, iso, product]
            if release == "release" && build_all_cmd == "build-all" && iso == "iso" =>
        {
            crate::workflows::build_all(crate::workflows::parse_release_product(Some(product))?)
        }
        [iso, build_all_cmd] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_release_product(None)?)
        }
        [iso, build_all_cmd, stage] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_release_product(Some(stage))?)
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
        [transform, build, product_erofs, prepared_dir]
            if transform == "transform" && build == "build" && product_erofs == "product-erofs" =>
        {
            crate::workflows::build_prepared_product_erofs_cmd(Path::new(prepared_dir))
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
        [artifact, preseed_stage01, distro]
            if artifact == "artifact" && preseed_stage01 == "preseed-rootfs-source" =>
        {
            crate::workflows::preseed_rootfs_source_cmd(distro, false)
        }
        [artifact, preseed_stage01, distro, refresh]
            if artifact == "artifact"
                && preseed_stage01 == "preseed-rootfs-source"
                && refresh == "--refresh" =>
        {
            crate::workflows::preseed_rootfs_source_cmd(distro, true)
        }
        [artifact, materialize_stage01, distro]
            if artifact == "artifact" && materialize_stage01 == "materialize-rootfs-source" =>
        {
            crate::workflows::materialize_rootfs_source_cmd(distro)
        }
        _ => bail!(crate::usage()),
    };
    command.with_context(|| format!("dispatching workflow for '{}'", args.join(" ")))
}
