use anyhow::{Context, Result};
use distro_contract::ConformanceContract;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::config::{
    load_boot_config_from_contract, load_installed_boot_payload_config_from_contract,
    load_live_tools_config_from_contract,
};
use crate::pipeline::io::{
    create_empty_overlay_dir, create_unique_output_dir, extract_erofs_rootfs,
};
use crate::pipeline::live_tools::{add_required_tools, InstallExperience, LiveToolsRuntimeAction};
use crate::pipeline::overlay::{
    create_live_overlay, ensure_openrc_shell, ensure_required_service_wiring,
    ensure_systemd_default_target, ensure_systemd_locale_completeness, ensure_systemd_sshd_dirs,
    BootOverlayPolicy,
};

use crate::pipeline::plan::{
    apply_producer_plan, build_baseline_producers, ProducerPlan, RootfsProducer,
};
use crate::pipeline::scripts::install_scenario_test_scripts;
use crate::pipeline::source::{
    cleanup_legacy_provider_dir, materialize_source_rootfs, RootfsSourcePolicy,
};
use crate::recipe::alpine_rootfs_source::is_alpine_rootfs_source_recipe;

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
    resolved_parent_rootfs_image: Option<PathBuf>,
    live_overlay: OverlayLayout,
    add_plan: ProducerPlan,
    required_services: Vec<String>,
    rootfs_source_policy: Option<RootfsSourcePolicy>,
    pub overlay: BootOverlayPolicy,
}

impl LiveBootProductSpec {
    pub fn required_services(&self) -> &[String] {
        &self.required_services
    }

    pub fn uses_rpm_dvd_rootfs_source(&self) -> bool {
        matches!(
            self.rootfs_source_policy,
            Some(RootfsSourcePolicy::RecipeRpmDvd { .. })
        )
    }

    pub fn rpm_dvd_preseed_recipe_script(&self) -> Option<&Path> {
        let Some(RootfsSourcePolicy::RecipeRpmDvd {
            preseed_recipe_script,
            ..
        }) = &self.rootfs_source_policy
        else {
            return None;
        };
        Some(preseed_recipe_script.as_path())
    }

    pub fn uses_alpine_live_source_rootfs(&self) -> bool {
        let Some(RootfsSourcePolicy::RecipeCustom { recipe_script, .. }) =
            &self.rootfs_source_policy
        else {
            return false;
        };
        is_alpine_rootfs_source_recipe(recipe_script)
    }

    pub fn with_resolved_parent_rootfs_image(mut self, image: PathBuf) -> Self {
        self.resolved_parent_rootfs_image = Some(image);
        self
    }
}

#[derive(Debug, Clone)]
pub struct LiveToolsProductSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    install_experience: InstallExperience,
    runtime_actions: Vec<LiveToolsRuntimeAction>,
    pub rootfs_source_dir: PathBuf,
    parent_rootfs: ParentRootfsInput,
    resolved_parent_rootfs_image: Option<PathBuf>,
    live_overlay: OverlayLayout,
    overlay: BootOverlayPolicy,
    required_services: Vec<String>,
}

impl LiveToolsProductSpec {
    pub fn with_resolved_parent_rootfs_image(mut self, image: PathBuf) -> Self {
        self.resolved_parent_rootfs_image = Some(image);
        self
    }
}

#[derive(Debug, Clone)]
pub struct InstalledBootProductSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub rootfs_source_dir: PathBuf,
    parent_rootfs: ParentRootfsInput,
    resolved_parent_rootfs_image: Option<PathBuf>,
    live_overlay_dir_name: String,
    add_plan: ProducerPlan,
    rootfs_source_policy: Option<RootfsSourcePolicy>,
}

impl InstalledBootProductSpec {
    pub fn with_resolved_parent_rootfs_image(mut self, image: PathBuf) -> Self {
        self.resolved_parent_rootfs_image = Some(image);
        self
    }
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
    _variant_dir: &Path,
    contract: &ConformanceContract,
    distro_id: &str,
    layout: DerivedProductLayout,
) -> Result<LiveBootProductSpec> {
    let loaded = load_boot_config_from_contract(repo_root, distro_id, contract)?;

    let mut add_producers = loaded.payload_producers.clone();
    if loaded.rootfs_source_policy.is_none() {
        add_producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
    }

    Ok(LiveBootProductSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: loaded.os_name,
        rootfs_source_dir: layout.rootfs_source_dir,
        parent_rootfs: layout.parent_rootfs,
        resolved_parent_rootfs_image: None,
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
    let parent_rootfs = resolved_parent_rootfs_image(
        "live-boot",
        &spec.distro_id,
        &spec.parent_rootfs,
        spec.resolved_parent_rootfs_image.as_deref(),
    )?;
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
    extract_erofs_rootfs(parent_rootfs, &rootfs_source_dir).with_context(|| {
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
    install_scenario_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing scenario test scripts into live boot rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if let BootOverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_shell(&rootfs_source_dir, &spec.os_name, inittab).with_context(|| {
            format!(
                "ensuring OpenRC live boot serial shell for '{}'",
                spec.distro_id
            )
        })?;
    }
    if matches!(&spec.overlay, BootOverlayPolicy::Systemd { .. }) {
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

    if let BootOverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
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
    contract: &ConformanceContract,
    distro_id: &str,
    layout: DerivedProductLayout,
) -> Result<LiveToolsProductSpec> {
    let loaded = load_live_tools_config_from_contract(contract)
        .with_context(|| format!("loading live-tools config for '{}'", distro_id))?;

    let live_boot_spec =
        load_live_boot_product_spec(repo_root, variant_dir, contract, distro_id, layout.clone())
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
        runtime_actions: loaded.runtime_actions,
        rootfs_source_dir: layout.rootfs_source_dir,
        parent_rootfs: layout.parent_rootfs,
        resolved_parent_rootfs_image: None,
        live_overlay: layout.live_overlay,
        overlay: live_boot_spec.overlay.clone(),
        required_services: live_boot_spec.required_services().to_vec(),
    })
}

pub fn load_installed_boot_product_spec(
    repo_root: &Path,
    _variant_dir: &Path,
    contract: &ConformanceContract,
    distro_id: &str,
    layout: DerivedProductLayout,
) -> Result<InstalledBootProductSpec> {
    let loaded = load_installed_boot_payload_config_from_contract(repo_root, distro_id, contract)?;

    let mut add_producers = loaded.payload_producers.clone();
    if loaded.rootfs_source_policy.is_none() {
        add_producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
    }

    Ok(InstalledBootProductSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        rootfs_source_dir: layout.rootfs_source_dir,
        parent_rootfs: layout.parent_rootfs,
        resolved_parent_rootfs_image: None,
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
    let parent_rootfs = resolved_parent_rootfs_image(
        "live-tools",
        &spec.distro_id,
        &spec.parent_rootfs,
        spec.resolved_parent_rootfs_image.as_deref(),
    )?;
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating live tools product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    extract_erofs_rootfs(parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent rootfs for live tools product from '{}'",
            parent_rootfs.display()
        )
    })?;

    install_scenario_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing scenario test scripts into live tools rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if matches!(&spec.overlay, BootOverlayPolicy::Systemd { .. }) {
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
        &spec.runtime_actions,
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
    let parent_rootfs = resolved_parent_rootfs_image(
        "installed-boot",
        &spec.distro_id,
        &spec.parent_rootfs,
        spec.resolved_parent_rootfs_image.as_deref(),
    )?;
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
    extract_erofs_rootfs(parent_rootfs, &rootfs_source_dir).with_context(|| {
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

fn resolved_parent_rootfs_image<'a>(
    product: &str,
    distro_id: &str,
    parent_rootfs: &ParentRootfsInput,
    resolved_parent_rootfs_image: Option<&'a Path>,
) -> Result<&'a Path> {
    resolved_parent_rootfs_image.ok_or_else(|| {
        anyhow::anyhow!(
            "missing resolved parent rootfs image for canonical product '{}' on '{}': workflow must resolve parent release '{}' before preparation\nRemediation: use the planner-backed `distro-builder product prepare {} {} <output_dir>` entrypoint or inject the parent rootfs path into the product spec before calling the preparer",
            product,
            distro_id,
            parent_rootfs.producer_label,
            product,
            distro_id
        )
    })
}
