use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::config::{load_boot_config, load_boot_payload_config, load_live_tools_config};
use crate::pipeline::io::{
    create_empty_overlay_dir, create_unique_output_dir, extract_erofs_rootfs,
    resolve_parent_product_rootfs_image_for_distro,
};
use crate::pipeline::live_tools::{add_required_tools, InstallExperience};
use crate::pipeline::overlay::{
    create_live_overlay, ensure_openrc_shell, ensure_required_service_wiring,
    ensure_systemd_default_target, ensure_systemd_locale_completeness, ensure_systemd_sshd_dirs,
    S01OverlayPolicy,
};
#[cfg(test)]
use crate::pipeline::plan::ensure_non_legacy_rootfs_source;
use crate::pipeline::plan::{
    apply_producer_plan, boot_baseline_producers, build_baseline_producers, ProducerPlan,
    RootfsProducer,
};
use crate::pipeline::scripts::install_stage_test_scripts;
use crate::pipeline::source::{
    cleanup_legacy_provider_dir, materialize_source_rootfs, S01RootfsSourcePolicy,
};
use crate::recipe::alpine_stage01::is_alpine_stage01_recipe;

#[derive(Debug, Clone)]
pub struct BaseRootfsProduct {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LiveBootProduct {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LiveToolsProduct {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct InstalledBootProduct {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BaseRootfsProductSpec {
    pub distro_id: String,
    pub os_name: String,
    pub os_id: String,
    pub rootfs_source_dir: PathBuf,
    live_overlay_dir_name: String,
    plan: ProducerPlan,
}

#[derive(Debug, Clone)]
pub struct BaseProductLayout {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir_name: String,
}

#[derive(Debug, Clone)]
pub struct ParentRootfsInput {
    pub release_dir_name: String,
    pub producer_label: String,
    pub rootfs_filename: String,
}

#[derive(Debug, Clone)]
pub struct OverlayLayout {
    pub issue_banner_label: String,
    pub dir_name: String,
}

#[derive(Debug, Clone)]
pub struct DerivedProductLayout {
    pub rootfs_source_dir: PathBuf,
    pub parent_rootfs: ParentRootfsInput,
    pub live_overlay: OverlayLayout,
}

#[derive(Debug, Clone)]
pub struct LiveBootProductSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    pub rootfs_source_dir: PathBuf,
    parent_rootfs: ParentRootfsInput,
    live_overlay: OverlayLayout,
    add_plan: ProducerPlan,
    required_services: Vec<String>,
    rootfs_source_policy: Option<S01RootfsSourcePolicy>,
    pub overlay: S01OverlayPolicy,
}

impl LiveBootProductSpec {
    pub fn required_services(&self) -> &[String] {
        &self.required_services
    }

    pub fn uses_rpm_dvd_rootfs_source(&self) -> bool {
        matches!(
            self.rootfs_source_policy,
            Some(S01RootfsSourcePolicy::RecipeRpmDvd { .. })
        )
    }

    pub fn rpm_dvd_preseed_recipe_script(&self) -> Option<&Path> {
        let Some(S01RootfsSourcePolicy::RecipeRpmDvd {
            preseed_recipe_script,
            ..
        }) = &self.rootfs_source_policy
        else {
            return None;
        };
        Some(preseed_recipe_script.as_path())
    }

    pub fn uses_alpine_stage01_rootfs_source(&self) -> bool {
        let Some(S01RootfsSourcePolicy::RecipeCustom { recipe_script, .. }) =
            &self.rootfs_source_policy
        else {
            return false;
        };
        is_alpine_stage01_recipe(recipe_script)
    }
}

#[derive(Debug, Clone)]
pub struct LiveToolsProductSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    install_experience: InstallExperience,
    pub rootfs_source_dir: PathBuf,
    parent_rootfs: ParentRootfsInput,
    live_overlay: OverlayLayout,
    overlay: S01OverlayPolicy,
    required_services: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InstalledBootProductSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub rootfs_source_dir: PathBuf,
    parent_rootfs: ParentRootfsInput,
    live_overlay_dir_name: String,
    add_plan: ProducerPlan,
    rootfs_source_policy: Option<S01RootfsSourcePolicy>,
}

pub fn load_base_rootfs_product_spec(
    distro_id: &str,
    os_name: &str,
    os_id: &str,
    _output_root: &Path,
    layout: BaseProductLayout,
) -> Result<BaseRootfsProductSpec> {
    Ok(BaseRootfsProductSpec {
        distro_id: distro_id.to_string(),
        os_name: os_name.to_string(),
        os_id: os_id.to_string(),
        rootfs_source_dir: layout.rootfs_source_dir,
        live_overlay_dir_name: layout.live_overlay_dir_name,
        plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: build_baseline_producers(distro_id, os_name, os_id),
        },
    })
}

pub fn prepare_base_rootfs_product(
    spec: &BaseRootfsProductSpec,
    output_dir: &Path,
) -> Result<BaseRootfsProduct> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating base rootfs product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    apply_producer_plan(&spec.plan, &rootfs_source_dir)
        .with_context(|| format!("materializing base rootfs for '{}'", spec.distro_id))?;

    let live_overlay_dir = create_empty_overlay_dir(output_dir, &spec.live_overlay_dir_name)
        .with_context(|| format!("creating empty overlay for {}", spec.distro_id))?;

    Ok(BaseRootfsProduct {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub fn load_live_boot_product_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    layout: DerivedProductLayout,
) -> Result<LiveBootProductSpec> {
    let loaded = load_boot_config(repo_root, variant_dir, distro_id)?;

    let mut add_producers = boot_baseline_producers(match &loaded.overlay {
        S01OverlayPolicy::Systemd { .. } => "systemd",
        S01OverlayPolicy::OpenRc { .. } => "openrc",
    });
    if loaded.rootfs_source_policy.is_none() {
        add_producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
    }

    Ok(LiveBootProductSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: loaded.os_name,
        rootfs_source_dir: layout.rootfs_source_dir,
        parent_rootfs: layout.parent_rootfs,
        live_overlay: layout.live_overlay,
        add_plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: add_producers,
        },
        required_services: loaded.required_services,
        rootfs_source_policy: loaded.rootfs_source_policy,
        overlay: loaded.overlay,
    })
}

pub fn prepare_live_boot_product(
    spec: &LiveBootProductSpec,
    output_dir: &Path,
) -> Result<LiveBootProduct> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating live boot product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    cleanup_legacy_provider_dir(output_dir).with_context(|| {
        format!(
            "cleaning legacy live boot provider directory under '{}'",
            output_dir.display()
        )
    })?;
    let parent_rootfs = resolve_parent_product_rootfs_image_for_distro(
        &spec.repo_root,
        &spec.distro_id,
        &spec.parent_rootfs.release_dir_name,
        &spec.parent_rootfs.producer_label,
        &spec.parent_rootfs.rootfs_filename,
    )?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent rootfs for live boot product from '{}'",
            parent_rootfs.display()
        )
    })?;

    let mut add_plan = spec.add_plan.clone();
    if add_plan.producers.iter().any(|producer| {
        matches!(
            producer,
            RootfsProducer::CopyTree { .. }
                | RootfsProducer::CopySymlink { .. }
                | RootfsProducer::CopyFile { .. }
        )
    }) {
        let source_rootfs_dir = materialize_source_rootfs(
            &spec.repo_root,
            &spec.distro_id,
            &spec.rootfs_source_policy,
        )?;
        add_plan.source_rootfs_dir = Some(source_rootfs_dir);
    }

    apply_producer_plan(&add_plan, &rootfs_source_dir).with_context(|| {
        format!(
            "applying live boot product additive producers for '{}'",
            spec.distro_id
        )
    })?;
    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into live boot rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_shell(&rootfs_source_dir, &spec.os_name, inittab).with_context(|| {
            format!(
                "ensuring OpenRC live boot serial shell for '{}'",
                spec.distro_id
            )
        })?;
    }
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_default_target(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live boot default target for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_sshd_dirs(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live boot sshd directories for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live boot locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let live_overlay_dir = create_live_overlay(
        output_dir,
        &spec.distro_id,
        &spec.os_name,
        &spec.live_overlay.issue_banner_label,
        &spec.live_overlay.dir_name,
        &spec.overlay,
    )?;

    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_shell(&live_overlay_dir, &spec.os_name, inittab).with_context(|| {
            format!(
                "ensuring OpenRC live overlay serial shell for '{}'",
                spec.distro_id
            )
        })?;
    }
    ensure_required_service_wiring(&live_overlay_dir, &spec.overlay, &spec.required_services)
        .with_context(|| {
            format!(
                "ensuring live boot required service wiring for '{}'",
                spec.distro_id
            )
        })?;

    Ok(LiveBootProduct {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub fn materialize_live_boot_source_rootfs(spec: &LiveBootProductSpec) -> Result<PathBuf> {
    materialize_source_rootfs(&spec.repo_root, &spec.distro_id, &spec.rootfs_source_policy)
}

pub fn load_live_tools_product_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    layout: DerivedProductLayout,
) -> Result<LiveToolsProductSpec> {
    let loaded = load_live_tools_config(variant_dir)
        .with_context(|| format!("loading live-tools config for '{}'", distro_id))?;

    let live_boot_spec =
        load_live_boot_product_spec(repo_root, variant_dir, distro_id, layout.clone())
            .with_context(|| {
                format!(
                    "loading live boot baseline while preparing live tools for '{}'",
                    distro_id
                )
            })?;

    Ok(LiveToolsProductSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: loaded.os_name,
        install_experience: loaded.install_experience,
        rootfs_source_dir: layout.rootfs_source_dir,
        parent_rootfs: layout.parent_rootfs,
        live_overlay: layout.live_overlay,
        overlay: live_boot_spec.overlay.clone(),
        required_services: live_boot_spec.required_services().to_vec(),
    })
}

pub fn load_installed_boot_product_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    layout: DerivedProductLayout,
) -> Result<InstalledBootProductSpec> {
    let loaded = load_boot_payload_config(repo_root, variant_dir, distro_id)?;

    let mut add_producers = boot_baseline_producers(match &loaded.overlay {
        S01OverlayPolicy::Systemd { .. } => "systemd",
        S01OverlayPolicy::OpenRc { .. } => "openrc",
    });
    if loaded.rootfs_source_policy.is_none() {
        add_producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
    }

    Ok(InstalledBootProductSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        rootfs_source_dir: layout.rootfs_source_dir,
        parent_rootfs: layout.parent_rootfs,
        live_overlay_dir_name: layout.live_overlay.dir_name,
        add_plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: add_producers,
        },
        rootfs_source_policy: loaded.rootfs_source_policy,
    })
}

pub fn prepare_live_tools_product(
    spec: &LiveToolsProductSpec,
    output_dir: &Path,
) -> Result<LiveToolsProduct> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating live tools product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    let parent_rootfs = resolve_parent_product_rootfs_image_for_distro(
        &spec.repo_root,
        &spec.distro_id,
        &spec.parent_rootfs.release_dir_name,
        &spec.parent_rootfs.producer_label,
        &spec.parent_rootfs.rootfs_filename,
    )?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent rootfs for live tools product from '{}'",
            parent_rootfs.display()
        )
    })?;

    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into live tools rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live tools locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let live_overlay_dir = create_live_overlay(
        output_dir,
        &spec.distro_id,
        &spec.os_name,
        &spec.live_overlay.issue_banner_label,
        &spec.live_overlay.dir_name,
        &spec.overlay,
    )?;

    add_required_tools(
        &spec.repo_root,
        &rootfs_source_dir,
        &live_overlay_dir,
        &spec.distro_id,
        spec.install_experience,
    )
    .with_context(|| format!("adding required live tools for '{}'", spec.distro_id))?;

    ensure_required_service_wiring(&live_overlay_dir, &spec.overlay, &spec.required_services)
        .with_context(|| {
            format!(
                "ensuring live tools required service wiring for '{}'",
                spec.distro_id
            )
        })?;

    Ok(LiveToolsProduct {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub fn prepare_installed_boot_product(
    spec: &InstalledBootProductSpec,
    output_dir: &Path,
) -> Result<InstalledBootProduct> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating installed boot product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    cleanup_legacy_provider_dir(output_dir).with_context(|| {
        format!(
            "cleaning legacy installed boot provider directory under '{}'",
            output_dir.display()
        )
    })?;
    let parent_rootfs = resolve_parent_product_rootfs_image_for_distro(
        &spec.repo_root,
        &spec.distro_id,
        &spec.parent_rootfs.release_dir_name,
        &spec.parent_rootfs.producer_label,
        &spec.parent_rootfs.rootfs_filename,
    )?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent rootfs for installed boot product from '{}'",
            parent_rootfs.display()
        )
    })?;

    let mut add_plan = spec.add_plan.clone();
    if add_plan.producers.iter().any(|producer| {
        matches!(
            producer,
            RootfsProducer::CopyTree { .. }
                | RootfsProducer::CopySymlink { .. }
                | RootfsProducer::CopyFile { .. }
        )
    }) {
        let source_rootfs_dir = materialize_source_rootfs(
            &spec.repo_root,
            &spec.distro_id,
            &spec.rootfs_source_policy,
        )?;
        add_plan.source_rootfs_dir = Some(source_rootfs_dir);
    }

    apply_producer_plan(&add_plan, &rootfs_source_dir).with_context(|| {
        format!(
            "applying installed boot product additive producers for '{}'",
            spec.distro_id
        )
    })?;

    let live_overlay_dir = create_empty_overlay_dir(output_dir, &spec.live_overlay_dir_name)
        .with_context(|| {
            format!(
                "creating empty installed boot overlay for {}",
                spec.distro_id
            )
        })?;

    Ok(InstalledBootProduct {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo_root(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "distro-builder-products-{test_name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write file");
    }

    fn live_tools_layout() -> DerivedProductLayout {
        DerivedProductLayout {
            rootfs_source_dir: PathBuf::from("live-tools-rootfs"),
            parent_rootfs: ParentRootfsInput {
                release_dir_name: "live-boot".to_string(),
                producer_label: "live-boot".to_string(),
                rootfs_filename: "filesystem.erofs".to_string(),
            },
            live_overlay: OverlayLayout {
                issue_banner_label: "Live Tools".to_string(),
                dir_name: "live-overlay".to_string(),
            },
        }
    }

    fn installed_boot_layout() -> DerivedProductLayout {
        DerivedProductLayout {
            rootfs_source_dir: PathBuf::from("installed-boot-rootfs"),
            parent_rootfs: ParentRootfsInput {
                release_dir_name: "base-rootfs".to_string(),
                producer_label: "base-rootfs".to_string(),
                rootfs_filename: "filesystem.erofs".to_string(),
            },
            live_overlay: OverlayLayout {
                issue_banner_label: "Installed Boot".to_string(),
                dir_name: "boot-overlay".to_string(),
            },
        }
    }

    fn write_live_tools_ring_scaffold(variant_dir: &Path) {
        write_file(
            &variant_dir.join("identity.toml"),
            r#"schema_version = 6

[identity]
os_name = "LevitateOS"
os_id = "levitateos"
iso_label = "LEVITATE"
os_version = "0.1.0"
default_hostname = "levitate"
"#,
        );
        write_file(
            &variant_dir.join("scenarios.toml"),
            r#"schema_version = 6

[scenarios.live_environment]
required_services = ["sshd"]

[scenarios.live_tools]
install_experience = "ux"
"#,
        );
        write_file(
            &variant_dir.join("ring2-products.toml"),
            r#"schema_version = 6

[ring2_products.rootfs_base]
logical_name = "product.rootfs.base"
description = "Canonical base root filesystem tree"

[ring2_products.live_overlay]
logical_name = "product.payload.live_overlay"
description = "Read-only live overlay payload tree"
overlay_kind = "systemd"

[ring2_products.boot_live]
logical_name = "product.payload.boot.live"
description = "Live boot payload inputs"
extends = "product.rootfs.base"

[ring2_products.live_tools]
logical_name = "product.payload.live_tools"
description = "Live tools payload tree"
extends = "product.payload.boot.live"

[ring2_products.boot_installed]
logical_name = "product.payload.boot.installed"
description = "Installed-system boot payload inputs"
extends = "product.rootfs.base"

[ring2_products.kernel_staging]
logical_name = "product.kernel.staging"
description = "Kernel image and modules staging product"
"#,
        );
        write_file(
            &variant_dir.join("ring3-sources.toml"),
            r#"schema_version = 6

[ring3_sources.rootfs_source]
kind = "recipe_rpm_dvd"
recipe_script = "distro-builder/recipes/fedora-stage01-rootfs.rhai"
preseed_recipe_script = "distro-builder/recipes/fedora-preseed-iso.rhai"
"#,
        );
    }

    #[test]
    fn base_rootfs_baseline_contains_os_release_files() {
        let producers = build_baseline_producers("levitate", "LevitateOS", "levitateos");
        let paths: Vec<PathBuf> = producers
            .iter()
            .filter_map(|p| match p {
                RootfsProducer::WriteText { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert!(paths.contains(&PathBuf::from("etc/os-release")));
        assert!(paths.contains(&PathBuf::from("usr/lib/product-manifest.json")));
    }

    #[test]
    fn legacy_rootfs_source_is_rejected() {
        let mut legacy = PathBuf::from("/data/vince/LevitateOS");
        for component in ["leviso", "downloads", "rootfs"] {
            legacy.push(component);
        }
        let result = ensure_non_legacy_rootfs_source(&legacy);
        assert!(result.is_err(), "legacy rootfs path must fail policy");
    }

    #[test]
    fn missing_rootfs_source_filters_copy_producers() {
        let mut producers = boot_baseline_producers("openrc");
        producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));

        assert!(!producers.is_empty());
        assert!(producers
            .iter()
            .all(|producer| matches!(producer, RootfsProducer::WriteText { .. })));
    }

    #[test]
    fn stage_scoped_rootfs_source_is_allowed() {
        let stage_scoped = Path::new(
            "/data/vince/LevitateOS/.artifacts/out/levitate/s01-boot/s01-rootfs-source-12345-12345",
        );
        let result = ensure_non_legacy_rootfs_source(stage_scoped);
        assert!(
            result.is_ok(),
            "stage-scoped rootfs path should be accepted"
        );
    }

    #[test]
    fn live_tools_product_spec_loads_without_02livetools_when_ring_files_exist() {
        let repo_root = temp_repo_root("live-tools-no-stage02");
        let variant_dir = repo_root.join("distro-variants/levitate");
        write_live_tools_ring_scaffold(&variant_dir);

        let spec =
            load_live_tools_product_spec(&repo_root, &variant_dir, "levitate", live_tools_layout())
                .expect("load live tools spec from ring owners");

        assert_eq!(spec.os_name, "LevitateOS");
        assert_eq!(spec.install_experience, InstallExperience::Ux);
        assert_eq!(spec.required_services, vec!["sshd".to_string()]);
        assert!(matches!(spec.overlay, S01OverlayPolicy::Systemd { .. }));

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn live_tools_product_spec_rejects_stage02_drift_from_scenarios() {
        let repo_root = temp_repo_root("live-tools-stage02-drift");
        let variant_dir = repo_root.join("distro-variants/levitate");
        write_live_tools_ring_scaffold(&variant_dir);
        write_file(
            &variant_dir.join("02LiveTools.toml"),
            r#"[stage_02.live_tools]
os_name = "LevitateOS"
install_experience = "automated_ssh"
"#,
        );

        let err =
            load_live_tools_product_spec(&repo_root, &variant_dir, "levitate", live_tools_layout())
                .expect_err("drifted stage 02 live-tools config should fail");
        assert!(
            format!("{err:#}").contains("scenario/live-tools parity mismatch"),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn installed_boot_product_spec_loads_from_ring_boot_payload_owners() {
        let repo_root = temp_repo_root("installed-boot-no-stage04");
        let variant_dir = repo_root.join("distro-variants/levitate");
        write_live_tools_ring_scaffold(&variant_dir);

        let spec = load_installed_boot_product_spec(
            &repo_root,
            &variant_dir,
            "levitate",
            installed_boot_layout(),
        )
        .expect("load installed boot spec from ring owners");

        assert_eq!(
            spec.rootfs_source_dir,
            PathBuf::from("installed-boot-rootfs")
        );
        assert_eq!(spec.parent_rootfs.release_dir_name, "base-rootfs");
        assert_eq!(spec.live_overlay_dir_name, "boot-overlay");
        assert!(spec
            .add_plan
            .producers
            .iter()
            .any(|producer| matches!(producer, RootfsProducer::WriteText { .. })));

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }
}
