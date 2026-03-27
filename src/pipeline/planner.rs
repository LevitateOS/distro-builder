use anyhow::{bail, Result};
use distro_contract::ConformanceContract;
use std::collections::HashSet;

const PRODUCT_BASE_ROOTFS: &str = "base-rootfs";
const PRODUCT_LIVE_BOOT: &str = "live-boot";
const PRODUCT_LIVE_TOOLS: &str = "live-tools";
const PRODUCT_INSTALLED_BOOT: &str = "installed-boot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductBuildPlan {
    pub requested_product: String,
    pub ordered_products: Vec<String>,
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

pub fn is_release_buildable_product(product: &str) -> bool {
    matches!(
        product,
        PRODUCT_BASE_ROOTFS | PRODUCT_LIVE_BOOT | PRODUCT_LIVE_TOOLS
    )
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
    use std::path::PathBuf;

    fn workspace_contract(distro_id: &str) -> ConformanceContract {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .canonicalize()
            .expect("canonicalize workspace root");
        load_variant_contract_for_distro_from(&repo_root, distro_id)
            .unwrap_or_else(|err| panic!("failed to load {} contract: {}", distro_id, err))
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
