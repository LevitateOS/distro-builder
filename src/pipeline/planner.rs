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
