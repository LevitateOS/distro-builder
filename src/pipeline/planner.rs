use crate::pipeline::io::{
    release_product_rootfs_exists_for_distro, resolve_release_product_rootfs_image_for_distro,
};
use anyhow::{bail, Context, Result};
use distro_contract::ConformanceContract;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const PRODUCT_BASE_ROOTFS: &str = "base-rootfs";
const PRODUCT_LIVE_BOOT: &str = "live-boot";
const PRODUCT_LIVE_TOOLS: &str = "live-tools";
const PRODUCT_INSTALLED_BOOT: &str = "installed-boot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductBuildPlan {
    pub requested_product: String,
    pub ordered_products: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductRealizationStep {
    pub product: String,
    pub resolved_parent_rootfs_image: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductRealizationPlan {
    pub requested_product: String,
    pub ordered_steps: Vec<ProductRealizationStep>,
}

impl ProductRealizationPlan {
    pub fn requested_step(&self) -> Result<&ProductRealizationStep> {
        self.ordered_steps
            .iter()
            .find(|step| step.product == self.requested_product)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "planner bug: requested canonical product '{}' is missing from the realization plan",
                    self.requested_product
                )
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePrerequisiteStep {
    pub product: String,
    pub rootfs_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePrerequisitePlan {
    pub requested_product: String,
    pub ordered_steps: Vec<ReleasePrerequisiteStep>,
}

impl ReleasePrerequisitePlan {
    pub fn missing_products(&self) -> Vec<&str> {
        self.ordered_steps
            .iter()
            .filter(|step| !step.rootfs_exists)
            .map(|step| step.product.as_str())
            .collect()
    }
}

pub fn plan_product_build_chain(
    contract: &ConformanceContract,
    requested_product: &str,
) -> Result<ProductBuildPlan> {
    let requested_product = requested_product.trim();
    if requested_product.is_empty() {
        bail!("requested canonical product must not be empty");
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut ordered_products = Vec::new();
    visit_product(
        contract,
        requested_product,
        &mut visiting,
        &mut visited,
        &mut ordered_products,
    )?;

    Ok(ProductBuildPlan {
        requested_product: requested_product.to_string(),
        ordered_products,
    })
}

pub fn plan_release_prerequisite_products(
    contract: &ConformanceContract,
    requested_product: &str,
) -> Result<Vec<String>> {
    let plan = plan_product_build_chain(contract, requested_product)?;
    let requested = plan.requested_product.as_str();
    Ok(plan
        .ordered_products
        .into_iter()
        .filter(|product| product != requested && is_release_buildable_product(product))
        .collect())
}

pub fn plan_release_prerequisite_realization(
    repo_root: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
    requested_product: &str,
) -> Result<ReleasePrerequisitePlan> {
    let ordered_products = plan_release_prerequisite_products(contract, requested_product)?;
    let mut ordered_steps = Vec::with_capacity(ordered_products.len());

    for product in ordered_products {
        let rootfs_exists = release_product_rootfs_exists_for_distro(
            repo_root,
            distro_id,
            release_dir_name_for_product(&product)?,
            &contract.artifacts.rootfs_name,
        )
        .with_context(|| {
            format!(
                "checking canonical release prerequisite '{}' for '{}'",
                product, distro_id
            )
        })?;
        ordered_steps.push(ReleasePrerequisiteStep {
            product,
            rootfs_exists,
        });
    }

    Ok(ReleasePrerequisitePlan {
        requested_product: requested_product.to_string(),
        ordered_steps,
    })
}

pub fn plan_product_realization(
    repo_root: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
    requested_product: &str,
) -> Result<ProductRealizationPlan> {
    let plan = plan_product_build_chain(contract, requested_product)?;
    let mut ordered_steps = Vec::with_capacity(plan.ordered_products.len());

    for product in &plan.ordered_products {
        let resolved_parent_rootfs_image = parent_product_for(contract, product)?
            .map(|parent_product| {
                resolve_parent_release_rootfs_image(repo_root, distro_id, contract, parent_product)
            })
            .transpose()?;

        ordered_steps.push(ProductRealizationStep {
            product: product.clone(),
            resolved_parent_rootfs_image,
        });
    }

    Ok(ProductRealizationPlan {
        requested_product: plan.requested_product,
        ordered_steps,
    })
}

pub fn is_release_buildable_product(product: &str) -> bool {
    matches!(
        product,
        PRODUCT_BASE_ROOTFS | PRODUCT_LIVE_BOOT | PRODUCT_LIVE_TOOLS
    )
}

fn resolve_parent_release_rootfs_image(
    repo_root: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
    parent_product: &str,
) -> Result<PathBuf> {
    let release_dir_name = release_dir_name_for_product(parent_product)?;
    resolve_release_product_rootfs_image_for_distro(
        repo_root,
        distro_id,
        release_dir_name,
        parent_product,
        &contract.artifacts.rootfs_name,
    )
    .with_context(|| {
        format!(
            "resolving canonical parent release rootfs '{}' for '{}'",
            parent_product, distro_id
        )
    })
}

fn release_dir_name_for_product(product: &str) -> Result<&'static str> {
    match product {
        PRODUCT_BASE_ROOTFS => Ok(PRODUCT_BASE_ROOTFS),
        PRODUCT_LIVE_BOOT => Ok(PRODUCT_LIVE_BOOT),
        PRODUCT_LIVE_TOOLS => Ok(PRODUCT_LIVE_TOOLS),
        PRODUCT_INSTALLED_BOOT => Ok(PRODUCT_INSTALLED_BOOT),
        other => bail!(
            "unsupported canonical product '{}' for planner release dir mapping; expected one of: '{}', '{}', '{}', '{}'",
            other,
            PRODUCT_BASE_ROOTFS,
            PRODUCT_LIVE_BOOT,
            PRODUCT_LIVE_TOOLS,
            PRODUCT_INSTALLED_BOOT
        ),
    }
}

fn visit_product(
    contract: &ConformanceContract,
    product: &str,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered_products: &mut Vec<String>,
) -> Result<()> {
    if visited.contains(product) {
        return Ok(());
    }
    if !visiting.insert(product.to_string()) {
        bail!(
            "cyclic canonical product dependency while planning '{}': product '{}' was visited twice",
            product,
            product
        );
    }

    if let Some(parent) = parent_product_for(contract, product)? {
        visit_product(contract, parent, visiting, visited, ordered_products)?;
    }

    visiting.remove(product);
    visited.insert(product.to_string());
    ordered_products.push(product.to_string());
    Ok(())
}

fn parent_product_for<'a>(
    contract: &'a ConformanceContract,
    product: &str,
) -> Result<Option<&'a str>> {
    let parent_logical_name = match product {
        PRODUCT_BASE_ROOTFS => None,
        PRODUCT_LIVE_BOOT => contract.products.boot_live.extends.as_deref(),
        PRODUCT_LIVE_TOOLS => contract.products.live_tools.extends.as_deref(),
        PRODUCT_INSTALLED_BOOT => contract
            .products
            .boot_installed
            .as_ref()
            .and_then(|product| product.extends.as_deref()),
        other => {
            bail!(
                "unsupported canonical product '{}' for planner; expected one of: '{}', '{}', '{}', '{}'",
                other,
                PRODUCT_BASE_ROOTFS,
                PRODUCT_LIVE_BOOT,
                PRODUCT_LIVE_TOOLS,
                PRODUCT_INSTALLED_BOOT
            )
        }
    };

    parent_logical_name
        .map(product_for_logical_name)
        .transpose()
}

fn product_for_logical_name(logical_name: &str) -> Result<&'static str> {
    match logical_name {
        "product.rootfs.base" => Ok(PRODUCT_BASE_ROOTFS),
        "product.payload.boot.live" => Ok(PRODUCT_LIVE_BOOT),
        "product.payload.live_tools" => Ok(PRODUCT_LIVE_TOOLS),
        "product.payload.boot.installed" => Ok(PRODUCT_INSTALLED_BOOT),
        other => bail!(
            "unsupported canonical product logical name '{}' in planner; expected one of: product.rootfs.base, product.payload.boot.live, product.payload.live_tools, product.payload.boot.installed",
            other
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use distro_contract::load_variant_contract_for_distro_from;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn workspace_contract(distro_id: &str) -> ConformanceContract {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .canonicalize()
            .expect("canonicalize workspace root");
        load_variant_contract_for_distro_from(&repo_root, distro_id)
            .unwrap_or_else(|err| panic!("failed to load {} contract: {}", distro_id, err))
    }

    fn temp_repo_root() -> TempDir {
        tempfile::tempdir().expect("repo tempdir")
    }

    fn write_successful_release_rootfs(
        repo_root: &Path,
        distro_id: &str,
        product: &str,
        rootfs_filename: &str,
    ) -> PathBuf {
        let run_dir = repo_root
            .join(".artifacts")
            .join("out")
            .join(distro_id)
            .join("releases")
            .join(release_dir_name_for_product(product).expect("release dir name"))
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(
            crate::run_history::manifest_path(&run_dir),
            serde_json::to_vec_pretty(&json!({
                "run_id": "run-1",
                "status": "success",
                "created_at_utc": "20260313T120000Z",
                "finished_at_utc": "20260313T120001Z",
            }))
            .expect("serialize manifest"),
        )
        .expect("write run manifest");
        let rootfs = run_dir.join(rootfs_filename);
        fs::write(&rootfs, b"rootfs").expect("write rootfs");
        rootfs
    }

    #[test]
    fn live_tools_plan_closes_over_live_boot_and_base_rootfs() {
        let contract = workspace_contract("levitate");
        let plan =
            plan_product_build_chain(&contract, PRODUCT_LIVE_TOOLS).expect("plan live-tools");
        assert_eq!(
            plan.ordered_products,
            vec![
                PRODUCT_BASE_ROOTFS.to_string(),
                PRODUCT_LIVE_BOOT.to_string(),
                PRODUCT_LIVE_TOOLS.to_string(),
            ]
        );
    }

    #[test]
    fn installed_boot_plan_closes_over_base_rootfs() {
        let contract = workspace_contract("levitate");
        let plan = plan_product_build_chain(&contract, PRODUCT_INSTALLED_BOOT)
            .expect("plan installed-boot");
        assert_eq!(
            plan.ordered_products,
            vec![
                PRODUCT_BASE_ROOTFS.to_string(),
                PRODUCT_INSTALLED_BOOT.to_string(),
            ]
        );
    }

    #[test]
    fn release_prerequisites_skip_requested_product() {
        let contract = workspace_contract("levitate");
        let prerequisites = plan_release_prerequisite_products(&contract, PRODUCT_LIVE_TOOLS)
            .expect("plan live-tools release prerequisites");
        assert_eq!(
            prerequisites,
            vec![
                PRODUCT_BASE_ROOTFS.to_string(),
                PRODUCT_LIVE_BOOT.to_string(),
            ]
        );
    }

    #[test]
    fn release_prerequisites_for_base_rootfs_are_empty() {
        let contract = workspace_contract("levitate");
        let prerequisites = plan_release_prerequisite_products(&contract, PRODUCT_BASE_ROOTFS)
            .expect("plan base-rootfs release prerequisites");
        assert!(prerequisites.is_empty());
    }

    #[test]
    fn release_prerequisite_realization_marks_missing_steps_in_dependency_order() {
        let repo_root = temp_repo_root();
        let contract = workspace_contract("levitate");

        let plan = plan_release_prerequisite_realization(
            repo_root.path(),
            "levitate",
            &contract,
            PRODUCT_LIVE_TOOLS,
        )
        .expect("plan live-tools release prerequisite realization");

        assert_eq!(
            plan.ordered_steps
                .iter()
                .map(|step| (step.product.as_str(), step.rootfs_exists))
                .collect::<Vec<_>>(),
            vec![(PRODUCT_BASE_ROOTFS, false), (PRODUCT_LIVE_BOOT, false)]
        );
        assert_eq!(
            plan.missing_products(),
            vec![PRODUCT_BASE_ROOTFS, PRODUCT_LIVE_BOOT]
        );
    }

    #[test]
    fn release_prerequisite_realization_skips_existing_parent_rootfs() {
        let repo_root = temp_repo_root();
        let contract = workspace_contract("levitate");
        let rootfs_filename = contract.artifacts.rootfs_name.clone();
        write_successful_release_rootfs(
            repo_root.path(),
            "levitate",
            PRODUCT_BASE_ROOTFS,
            &rootfs_filename,
        );

        let plan = plan_release_prerequisite_realization(
            repo_root.path(),
            "levitate",
            &contract,
            PRODUCT_LIVE_TOOLS,
        )
        .expect("plan live-tools release prerequisite realization");

        assert_eq!(
            plan.ordered_steps
                .iter()
                .map(|step| (step.product.as_str(), step.rootfs_exists))
                .collect::<Vec<_>>(),
            vec![(PRODUCT_BASE_ROOTFS, true), (PRODUCT_LIVE_BOOT, false)]
        );
        assert_eq!(plan.missing_products(), vec![PRODUCT_LIVE_BOOT]);
    }

    #[test]
    fn product_realization_resolves_parent_release_rootfs_images() {
        let repo_root = temp_repo_root();
        let contract = workspace_contract("levitate");
        let rootfs_filename = contract.artifacts.rootfs_name.clone();
        let base_rootfs = write_successful_release_rootfs(
            repo_root.path(),
            "levitate",
            PRODUCT_BASE_ROOTFS,
            &rootfs_filename,
        );
        let live_boot_rootfs = write_successful_release_rootfs(
            repo_root.path(),
            "levitate",
            PRODUCT_LIVE_BOOT,
            &rootfs_filename,
        );

        let plan =
            plan_product_realization(repo_root.path(), "levitate", &contract, PRODUCT_LIVE_TOOLS)
                .expect("plan live-tools realization");

        assert_eq!(
            plan.requested_step().expect("requested step").product,
            PRODUCT_LIVE_TOOLS
        );
        assert_eq!(
            plan.ordered_steps
                .iter()
                .map(|step| step.product.as_str())
                .collect::<Vec<_>>(),
            vec![PRODUCT_BASE_ROOTFS, PRODUCT_LIVE_BOOT, PRODUCT_LIVE_TOOLS]
        );
        assert_eq!(plan.ordered_steps[0].resolved_parent_rootfs_image, None);
        assert_eq!(
            plan.ordered_steps[1].resolved_parent_rootfs_image.as_ref(),
            Some(&base_rootfs)
        );
        assert_eq!(
            plan.ordered_steps[2].resolved_parent_rootfs_image.as_ref(),
            Some(&live_boot_rootfs)
        );
    }

    #[test]
    fn product_realization_requires_parent_release_rootfs() {
        let repo_root = temp_repo_root();
        let contract = workspace_contract("levitate");
        let rootfs_filename = contract.artifacts.rootfs_name.clone();
        write_successful_release_rootfs(
            repo_root.path(),
            "levitate",
            PRODUCT_BASE_ROOTFS,
            &rootfs_filename,
        );

        let err =
            plan_product_realization(repo_root.path(), "levitate", &contract, PRODUCT_LIVE_TOOLS)
                .expect_err("missing live-boot release must fail realization planning");

        assert!(
            err.to_string()
                .contains("resolving canonical parent release rootfs 'live-boot'"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn planner_rejects_cycles() {
        let mut contract = workspace_contract("levitate");
        contract.products.boot_live.extends = Some("product.payload.live_tools".to_string());
        let err = plan_product_build_chain(&contract, PRODUCT_LIVE_TOOLS)
            .expect_err("cyclic live-tools plan must fail");
        assert!(
            err.to_string()
                .contains("cyclic canonical product dependency"),
            "unexpected error: {err:#}"
        );
    }
}
