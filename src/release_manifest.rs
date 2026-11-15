//! The release set of a system part: its rootfs image, kernel and installed
//! initramfs under their ISO paths, and `release.json`
//! (`distro_contract::release::ReleaseManifest`) naming each with its size and
//! SHA-256. `write` publishes it from already-built artifacts; `verify`
//! checks a set before the combiner puts it on the ISO.
use anyhow::{ensure, Context, Result};
use distro_contract::{release::*, Variant};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// A release set that `verify` checked: its manifest and where its members
/// are.
pub struct VerifiedRelease {
    dir: PathBuf,
    manifest: ReleaseManifest,
}

impl VerifiedRelease {
    pub fn manifest(&self) -> &ReleaseManifest {
        &self.manifest
    }
    pub fn manifest_path(&self) -> PathBuf {
        self.dir.join(MANIFEST)
    }
    pub fn rootfs(&self) -> PathBuf {
        self.dir.join(&self.manifest.artifacts.rootfs.path)
    }
    pub fn kernel(&self) -> PathBuf {
        self.dir.join(&self.manifest.artifacts.kernel.path)
    }
    pub fn initramfs(&self) -> PathBuf {
        self.dir.join(&self.manifest.artifacts.initramfs.path)
    }
}

fn artifact(base: &Path, path: &str) -> Result<Artifact> {
    let mut file = levitate_recipe::fsx::open_regular(&base.join(path))
        .with_context(|| format!("opening release member {path}"))?;
    let size = file.metadata()?.len();
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)?;
    Ok(Artifact {
        path: path.into(),
        size,
        sha256: format!("{:x}", hash.finalize()),
    })
}

fn link(source: &Path, destination: &Path) -> Result<()> {
    if source == destination {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    // Build outputs are owned by this release run. Replace only this alias.
    if fs::symlink_metadata(destination).is_ok() {
        fs::remove_file(destination)?;
    }
    fs::hard_link(source, destination)
        .with_context(|| format!("linking release artifact {}", destination.display()))
}

/// The system identity `variant`'s contract gives its system.
fn identity(variant: &Variant) -> Result<Identity> {
    let contract = &variant.contract;
    Ok(Identity {
        distro: contract.identity.os_id.clone(),
        architecture: match std::env::consts::ARCH {
            "x86_64" => Architecture::X86_64,
            "aarch64" => Architecture::Aarch64,
            _ => anyhow::bail!("unsupported release architecture"),
        },
        init: contract.profile.init,
        libc: contract.profile.libc,
        version: contract.identity.os_version.clone(),
    })
}

/// The digest of the module tree `root` carries for `release`.
fn modules_digest(root: &Path, release: &str) -> Result<String> {
    recinit::installed::modules_digest(&root.join("usr/lib/modules").join(release))
        .with_context(|| format!("hashing the {release} module tree of {}", root.display()))
}

/// Publishes the release set of the system built from the tree
/// `root` in `dir`: links the rootfs image and the installed initramfs and
/// copies the kernel to their ISO paths, then writes `release.json`. Builds
/// nothing.
pub fn write(
    variant: &Variant,
    root: &Path,
    dir: &Path,
    kernel_release: &str,
    kernel: &Path,
    initramfs: &Path,
) -> Result<PathBuf> {
    let contract = &variant.contract;
    let sources = crate::pipeline::sources::recorded(dir)
        .context("release has no system-sources.sha256 provenance record")?;
    hash(&sources)?;
    link(
        &dir.join(&contract.artifacts.rootfs),
        &dir.join(recinit::live::ROOTFS_ISO_PATH),
    )?;
    let media_kernel = dir.join(recinit::live::KERNEL_ISO_PATH);
    fs::create_dir_all(media_kernel.parent().unwrap())?;
    if fs::canonicalize(kernel)? != fs::canonicalize(&media_kernel).unwrap_or_default() {
        fs::copy(kernel, &media_kernel)?;
    }
    link(
        initramfs,
        &dir.join(recinit::live::INITRAMFS_INSTALLED_ISO_PATH),
    )?;
    let manifest = ReleaseManifest::new(
        identity(variant)?,
        sources,
        Kernel {
            release: kernel_release.into(),
            modules_sha256: modules_digest(root, kernel_release)?,
        },
        Artifacts {
            rootfs: artifact(dir, recinit::live::ROOTFS_ISO_PATH)?,
            kernel: artifact(dir, recinit::live::KERNEL_ISO_PATH)?,
            initramfs: artifact(dir, recinit::live::INITRAMFS_INSTALLED_ISO_PATH)?,
        },
    );
    manifest.validate()?;
    let destination = dir.join(MANIFEST);
    distro_contract::runs::write_atomic(&destination, &serde_json::to_vec_pretty(&manifest)?)?;
    Ok(destination)
}

/// Verifies the release set published in `dir`: its manifest names this
/// variant's system, every member has its recorded size and SHA-256, and
/// `root`, the tree it was built from, carries the module tree it binds.
pub fn verify(variant: &Variant, root: &Path, dir: &Path) -> Result<VerifiedRelease> {
    let raw = fs::read(dir.join(MANIFEST))
        .with_context(|| format!("reading {}", dir.join(MANIFEST).display()))?;
    let manifest = ReleaseManifest::parse(&raw)?;
    let expected = identity(variant)?;
    ensure!(
        manifest.identity == expected,
        "the release set names another system than {} ({:?})",
        variant.id,
        manifest.identity
    );
    for member in manifest.artifacts.all() {
        let found = artifact(dir, &member.path)?;
        ensure!(
            found.size == member.size && found.sha256 == member.sha256,
            "release member {} differs from release.json",
            member.path
        );
    }
    ensure!(
        modules_digest(root, &manifest.kernel.release)? == manifest.kernel.modules_sha256,
        "the system tree's {} module tree differs from release.json",
        manifest.kernel.release
    );
    Ok(VerifiedRelease {
        dir: dir.to_path_buf(),
        manifest,
    })
}
