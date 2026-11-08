use anyhow::{bail, Context, Result};
use distro_builder::product::ProductId;
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
    let repo_root = crate::workflows::repo_root()?;
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
        _ => bail!("{}", crate::usage()),
    };

    let (distro_id, product) =
        crate::workflows::parse_release_build_command(build_args, &repo_root)?;
    crate::workflows::ensure_release_prerequisites(&repo_root, &distro_id, product.product)?;
    crate::workflows::build_one(&distro_id, product)
}

pub(crate) fn dispatch_non_release_command(args: &[String]) -> Result<()> {
    let command = match args {
        [release, build_all_cmd, iso]
            if release == "release" && build_all_cmd == "build-all" && iso == "iso" =>
        {
            crate::workflows::build_all(crate::workflows::parse_release_product(
                ProductId::BaseRootfs.name(),
            )?)
        }
        [release, build_all_cmd, iso, product]
            if release == "release" && build_all_cmd == "build-all" && iso == "iso" =>
        {
            crate::workflows::build_all(crate::workflows::parse_release_product(product)?)
        }
        [iso, build_all_cmd] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_release_product(
                ProductId::BaseRootfs.name(),
            )?)
        }
        [iso, build_all_cmd, product] if iso == "iso" && build_all_cmd == "build-all" => {
            crate::workflows::build_all(crate::workflows::parse_release_product(product)?)
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
        [artifact, prepare] if artifact == "artifact" && prepare == "prepare-qemu-tools" => {
            let repo = crate::workflows::repo_root()?;
            let tools = distro_builder::recipe::qemu::prepare_qemu_tools(&repo)?;
            println!("{}", tools.display());
            Ok(())
        }
        [artifact, prepare_source, package, output]
            if artifact == "artifact" && prepare_source == "prepare-tool-source" =>
        {
            let repo = crate::workflows::repo_root()?;
            let output = Path::new(output);
            distro_builder::recipe::tools::prepare_tool_source_into(&repo, package, output)?;
            println!("{}", output.display());
            Ok(())
        }
        [artifact, preseed_source, distro]
            if artifact == "artifact" && preseed_source == "preseed-rootfs-source" =>
        {
            crate::workflows::preseed_rootfs_source_cmd(distro, false)
        }
        [artifact, preseed_source, distro, refresh]
            if artifact == "artifact"
                && preseed_source == "preseed-rootfs-source"
                && refresh == "--refresh" =>
        {
            crate::workflows::preseed_rootfs_source_cmd(distro, true)
        }
        [artifact, materialize_source, distro]
            if artifact == "artifact" && materialize_source == "materialize-rootfs-source" =>
        {
            crate::workflows::materialize_rootfs_source_cmd(distro)
        }
        _ => bail!("{}", crate::usage()),
    };
    command.with_context(|| format!("dispatching workflow for '{}'", args.join(" ")))
}
