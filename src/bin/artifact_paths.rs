use std::path::{Path, PathBuf};

use distro_builder::run_history;

pub fn distro_output_root_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    run_history::distro_output_dir(repo_root, distro_id)
}

pub fn release_product_dir_for(repo_root: &Path, distro_id: &str, product_dir_name: &str) -> PathBuf {
    run_history::release_product_dir(repo_root, distro_id, product_dir_name)
}

pub fn kernel_output_dir_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    run_history::kernel_output_dir(repo_root, distro_id)
}
