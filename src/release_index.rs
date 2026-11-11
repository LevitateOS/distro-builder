//! Explicit local publication. No endpoint or signing key is invented here.
use anyhow::{ensure, Context, Result};
use distro_contract::{release::*, Variant};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
};
fn write_public(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("publication needs a parent")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o644))?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
fn bytes(path: &Path, bound: u64) -> Result<Vec<u8>> {
    let file = levitate_recipe::fsx::open_regular(path)?;
    ensure!(
        file.metadata()?.len() <= bound,
        "release metadata exceeds bound"
    );
    let mut bytes = Vec::new();
    file.take(bound + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= bound,
        "release metadata exceeds bound"
    );
    Ok(bytes)
}
fn copy_checked(source: &Path, dest: &Path, artifact: &Artifact) -> Result<()> {
    let mut input = levitate_recipe::fsx::open_regular(source)?;
    ensure!(
        input.metadata()?.len() == artifact.size,
        "publishing payload size mismatch"
    );
    fs::create_dir_all(dest.parent().unwrap())?;
    let mut output = tempfile::NamedTempFile::new_in(dest.parent().unwrap())?;
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    let mut total = 0;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= artifact.size, "publishing payload grew");
        hash.update(&buffer[..n]);
        std::io::Write::write_all(&mut output, &buffer[..n])?;
    }
    ensure!(
        total == artifact.size && format!("{:x}", hash.finalize()) == artifact.sha256,
        "publishing payload digest mismatch"
    );
    output
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o644))?;
    output.as_file().sync_all()?;
    output.persist(dest)?;
    Ok(())
}
pub fn publish(
    variant: &Variant,
    bundle: &Path,
    output: &Path,
    sequence: u64,
    key: &Path,
) -> Result<()> {
    let policy = &variant.contract.release;
    policy.validate()?;
    let trust = policy
        .trust
        .as_ref()
        .context("configure distribution release trust before publishing an index")?;
    let manifest_bytes = bytes(&bundle.join(MANIFEST), MAX_MANIFEST as u64)?;
    let signature = bytes(&bundle.join(SIGNATURE), 64)?;
    verify_signature(&manifest_bytes, &signature, trust)?;
    let manifest = ReleaseManifest::parse(&manifest_bytes)?;
    ensure!(
        manifest.identity.distro == variant.contract.identity.os_id
            && manifest.identity.channel == policy.channel
            && manifest.identity.init == variant.contract.profile.init
            && manifest.identity.libc == variant.contract.profile.libc,
        "published release differs from variant/channel"
    );
    let id = digest(&manifest_bytes);
    fs::create_dir_all(output)?;
    let secret = crate::release_manifest::signing_key(variant, key, &[bundle, output])?;
    fs::set_permissions(output, fs::Permissions::from_mode(0o755))?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(output.join(".index.lock"))?;
    use std::os::fd::AsRawFd;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "cannot lock release publisher: {}",
        std::io::Error::last_os_error()
    );
    let mut index = if output.join(INDEX).exists() {
        let raw = bytes(&output.join(INDEX), MAX_MANIFEST as u64)?;
        verify_signature(
            &raw,
            &bytes(
                &output
                    .join("index-signatures")
                    .join(format!("{}.sig", digest(&raw))),
                64,
            )?,
            trust,
        )?;
        ReleaseIndex::parse(&raw)?
    } else {
        ReleaseIndex {
            schema_version: 1,
            channel: policy.channel.clone(),
            sequence: 0,
            releases: vec![],
        }
    };
    ensure!(
        index.channel == policy.channel && sequence > index.sequence,
        "publishing index sequence must advance within the configured channel"
    );
    for old in &index.releases {
        if old.identity.distro == manifest.identity.distro
            && old.identity.architecture == manifest.identity.architecture
            && old.identity.flavour == manifest.identity.flavour
            && old.identity.init == manifest.identity.init
            && old.identity.libc == manifest.identity.libc
        {
            ensure!(
                manifest.identity.sequence > old.identity.sequence
                    || (manifest.identity.sequence == old.identity.sequence
                        && old.manifest.sha256 == id),
                "release sequence reuse/downgrade refused"
            );
        }
    }
    let prefix = format!("releases/{id}");
    let destination = output.join(&prefix);
    if !destination.exists() {
        let staged = tempfile::Builder::new()
            .prefix(".release-")
            .tempdir_in(output)?;
        for artifact in manifest.artifacts.all() {
            copy_checked(
                &bundle.join(&artifact.path),
                &staged.path().join(&artifact.path),
                artifact,
            )?;
        }
        fs::write(staged.path().join(MANIFEST), &manifest_bytes)?;
        fs::write(staged.path().join(SIGNATURE), &signature)?;
        fs::create_dir_all(destination.parent().unwrap())?;
        fs::set_permissions(
            destination.parent().unwrap(),
            fs::Permissions::from_mode(0o755),
        )?;
        write_public(&staged.path().join(".complete"), id.as_bytes())?;
        // Flush data and parent directory before it becomes index-reachable.
        for item in walkdir::WalkDir::new(staged.path())
            .follow_links(false)
            .contents_first(true)
        {
            let item = item?;
            if item.file_type().is_file() || item.file_type().is_dir() {
                fs::set_permissions(
                    item.path(),
                    fs::Permissions::from_mode(if item.file_type().is_dir() {
                        0o755
                    } else {
                        0o644
                    }),
                )?;
                fs::File::open(item.path())?.sync_all()?;
            }
        }
        fs::rename(staged.path(), &destination)?;
        fs::File::open(destination.parent().unwrap())?.sync_all()?;
    } else {
        ensure!(
            bytes(&destination.join(MANIFEST), MAX_MANIFEST as u64)? == manifest_bytes,
            "published release object conflict"
        );
        ensure!(
            bytes(&destination.join(SIGNATURE), 64)? == signature,
            "published release signature conflict"
        );
        for artifact in manifest.artifacts.all() {
            let mut file = levitate_recipe::fsx::open_regular(&destination.join(&artifact.path))?;
            use sha2::{Digest, Sha256};
            let mut hash = Sha256::new();
            let size = std::io::copy(&mut file, &mut hash)?;
            ensure!(
                size == artifact.size && format!("{:x}", hash.finalize()) == artifact.sha256,
                "published release object was altered"
            );
        }
    }
    index.releases.retain(|old| {
        !(old.identity.distro == manifest.identity.distro
            && old.identity.architecture == manifest.identity.architecture
            && old.identity.flavour == manifest.identity.flavour
            && old.identity.init == manifest.identity.init
            && old.identity.libc == manifest.identity.libc)
    });
    index.releases.push(IndexEntry {
        identity: manifest.identity.clone(),
        manifest: Artifact {
            path: format!("{prefix}/{MANIFEST}"),
            sha256: id,
            size: manifest_bytes.len() as u64,
        },
        signature: Artifact {
            path: format!("{prefix}/{SIGNATURE}"),
            sha256: digest(&signature),
            size: 64,
        },
    });
    index.sequence = sequence;
    index.validate()?;
    let raw = serde_json::to_vec_pretty(&index)?;
    let signature = sign(&raw, &secret, trust)?;
    // The index signature is itself content addressed. Readers request its
    // index-hash name, so concurrent index publication cannot mix generations.
    let signatures = output.join("index-signatures");
    fs::create_dir_all(&signatures)?;
    fs::set_permissions(&signatures, fs::Permissions::from_mode(0o755))?;
    write_public(
        &signatures.join(format!("{}.sig", digest(&raw))),
        &signature,
    )?;
    write_public(&output.join(INDEX_SIGNATURE), &signature)?;
    write_public(&output.join(INDEX), &raw)?;
    Ok(())
}
