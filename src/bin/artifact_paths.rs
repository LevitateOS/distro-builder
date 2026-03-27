use std::path::{Path, PathBuf};

pub fn output_dir_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root.join(".artifacts").join("out").join(distro_id)
}

pub fn distro_output_root_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    output_dir_for(repo_root, distro_id)
}

pub fn release_product_dir_for(
    repo_root: &Path,
    distro_id: &str,
    product_dir_name: &str,
) -> PathBuf {
    output_dir_for(repo_root, distro_id)
        .join("releases")
        .join(product_dir_name)
}

pub fn kernel_output_dir_for(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root
        .join(".artifacts")
        .join("kernel")
        .join(distro_id)
        .join("current")
}
