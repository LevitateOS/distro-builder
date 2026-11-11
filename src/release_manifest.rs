//! Publish one release set for media and online distribution. This does not
//! build images: it consumes a prepared tree and its already-built artifacts.
use anyhow::{Context, Result, ensure};
use distro_contract::{Variant, release::*};
use distro_profile::Flavour;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Keep the verified input paths alive through ISO assembly, rather than
/// reopening the mutable run-directory artifact names after validation.
pub struct ProducedRelease {
    pub path: PathBuf,
    pub inputs: recab::release::VerifiedRelease,
}

/// Read the explicitly supplied signing key without allowing it to become a
/// checkout input or part of the prepared/published system. Call before writing
/// public release objects; the configured public key remains the authority.
pub(crate) fn signing_key(
    variant: &Variant,
    path: &Path,
    public_roots: &[&Path],
) -> Result<Vec<u8>> {
    let path = fs::canonicalize(path).context("opening distribution signing key")?;
    ensure!(
        !path.starts_with(fs::canonicalize(&variant.repo_root)?),
        "distribution signing key must stay outside the checkout"
    );
    for root in public_roots {
        ensure!(
            !path.starts_with(fs::canonicalize(root)?),
            "distribution signing key must stay outside prepared and published trees"
        );
    }
    let file = levitate_recipe::fsx::open_regular(&path)?;
    ensure!(
        file.metadata()?.len() <= 16384,
        "release signing key exceeds bound"
    );
    let mut secret = Vec::new();
    file.take(16385).read_to_end(&mut secret)?;
    // Validate key format and authority before publishing any release content.
    sign(
        b"",
        &secret,
        variant
            .contract
            .release
            .trust
            .as_ref()
            .context("configure release.trust before signing")?,
    )?;
    Ok(secret)
}

fn artifact(base: &Path, path: &str) -> Result<Artifact> {
    let mut file = levitate_recipe::fsx::open_regular(&base.join(path))?;
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
/// Produces release.json, system-inventory.json and an optional release.sig in
/// `dir`. The boot/live paths also form the online payload layout. Requires root
/// for recab's read-only verification mount; performs no image/ISO/kernel build.
pub fn write(
    variant: &Variant,
    flavour: Flavour,
    root: &Path,
    dir: &Path,
    kernel: &Path,
    initramfs: &Path,
    signing_key: Option<&Path>,
) -> Result<ProducedRelease> {
    let contract = &variant.contract;
    let policy = &contract.release;
    policy.validate()?;
    ensure!(
        policy.sequence > 0,
        "configure a positive release.sequence before producing a release"
    );
    let root_policy: Policy = serde_json::from_slice(&fs::read(root.join(SOURCE_CONFIG))?)?;
    ensure!(
        root_policy == policy.installed(),
        "prepared rootfs release policy differs from the variant contract"
    );
    let secret = match (&policy.trust, signing_key) {
        (Some(_), Some(key)) => Some(self::signing_key(variant, key, &[root, dir])?),
        (Some(_), None) => anyhow::bail!(
            "signed releases require an explicit distribution PKCS#8 signing key"
        ),
        (None, Some(_)) => anyhow::bail!(
            "configure release.trust before signing; the signing key cannot select its own authority"
        ),
        (None, None) => None,
    };
    let sources = crate::pipeline::sources::recorded(dir)
        .context("release has no system-sources.sha256 provenance record")?;
    hash(&sources)?;
    let proof_file = levitate_recipe::fsx::open_regular(&provenance_path(
        &dir.join(&contract.artifacts.rootfs),
    ))?;
    ensure!(
        proof_file.metadata()?.len() <= MAX_INVENTORY + 1024 * 1024,
        "image provenance exceeds bound"
    );
    let proof: RootfsProvenance = serde_json::from_reader(proof_file)?;
    ensure!(
        proof.schema_version == 1,
        "unsupported image provenance version"
    );
    proof.inventory.validate()?;
    ensure!(
        serde_json::to_vec(&proof.inventory)?
            == serde_json::to_vec(&SystemInventory::from_target(
                levitate_recipe::target::inspect(root)?
            )?)?,
        "prepared tree differs from image-writer inventory"
    );
    let inventory = proof.inventory.clone();
    distro_contract::runs::write_atomic(&dir.join(INVENTORY), &serde_json::to_vec(&inventory)?)?;
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
    let binding: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("usr/lib/recab/kernel.json"))?)?;
    let manifest = ReleaseManifest {
        schema_version: 1,
        identity: Identity {
            distro: contract.identity.os_id.clone(),
            architecture: match std::env::consts::ARCH {
                "x86_64" => Architecture::X86_64,
                "aarch64" => Architecture::Aarch64,
                _ => anyhow::bail!("unsupported release architecture"),
            },
            init: contract.profile.init,
            libc: contract.profile.libc,
            flavour,
            channel: policy.channel.clone(),
            sequence: policy.sequence,
            version: contract.identity.os_version.clone(),
        },
        sources_sha256: sources,
        compatibility: Compatibility {
            minimum_recab: recab::release::RECAB_VERSION.into(),
            state_major: STATE_MAJOR,
            state_minor: STATE_MINOR,
            boot_protocol: BOOT_PROTOCOL,
        },
        kernel: Kernel {
            release: binding["release"]
                .as_str()
                .context("kernel release missing")?
                .into(),
            modules_sha256: binding["modules_digest"]
                .as_str()
                .context("module binding missing")?
                .into(),
        },
        artifacts: Artifacts {
            rootfs: artifact(dir, recinit::live::ROOTFS_ISO_PATH)?,
            inventory: artifact(dir, INVENTORY)?,
            kernel: artifact(dir, recinit::live::KERNEL_ISO_PATH)?,
            initramfs: artifact(dir, recinit::live::INITRAMFS_INSTALLED_ISO_PATH)?,
        },
    };
    ensure!(
        manifest.artifacts.rootfs.sha256 == proof.rootfs_sha256,
        "rootfs image differs from image-writer provenance"
    );
    manifest.validate()?;
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    if let (Some(trust), Some(secret)) = (&policy.trust, &secret) {
        let signature = sign(&bytes, secret, trust)?;
        distro_contract::runs::write_atomic(&dir.join(SIGNATURE), &signature)?;
    }
    let mut candidate = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut candidate, &bytes)?;
    candidate.as_file().sync_all()?;
    let verified = recab::release::verify_produced_bundle(
        root,
        candidate.path(),
        policy.trust.as_ref(),
        policy.trust.is_none(),
        &proof,
    )
    .context("validating the produced release set")?;
    ensure!(
        verified.id() == digest(&bytes),
        "release verification identity changed"
    );
    // The manifest is the last publication, after the exact set is validated.
    let destination = dir.join(MANIFEST);
    distro_contract::runs::write_atomic(&destination, &bytes)?;
    Ok(ProducedRelease {
        path: destination,
        inputs: verified,
    })
}
