//! Explicit local publication. No endpoint or signing key is invented here.
use anyhow::{Context, Result, ensure};
use distro_contract::{Variant, release::*};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
};
fn now() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
}

/// Bootstrap public role authorization. Secret bytes stay outside source and output.
pub fn authorize(variant: &Variant, document: &Path, output: &Path, key: &Path) -> Result<()> {
    use production::*;
    let policy = &variant.contract.release;
    policy.validate()?;
    let production = policy
        .production
        .as_ref()
        .context("configure an explicit production root anchor first")?;
    let raw = bytes(document, 16384)?;
    let delegation: KeyDelegation = serde_json::from_slice(&raw)?;
    delegation.validate()?;
    ensure!(
        delegation.root_version == production.root.version,
        "successor-root authorization needs its dual-signed transition chain"
    );
    let parent = output
        .parent()
        .context("authorization output needs a directory")?;
    fs::create_dir_all(parent)?;
    let secret = crate::release_manifest::read_signing_key(variant, key, &[parent])?;
    let proof = AuthorityProof {
        document: String::from_utf8(raw.clone())?,
        signature: sign(&raw, &secret, &production.root.key)?,
        transitions: vec![],
    };
    let verified = proof.verify(
        production,
        &variant.contract.identity.os_id,
        &policy.channel,
        now()?,
    )?;
    if output.try_exists()? {
        let previous = AuthorityProof::parse(&bytes(output, MAX_AUTHORITY as u64)?)?;
        let previous = previous.verify_archived(
            production,
            &variant.contract.identity.os_id,
            &policy.channel,
        )?;
        ensure!(
            delegation.version > previous.delegation().version,
            "key authorization version must advance"
        );
    }
    let _ = verified;
    write_public(output, &serde_json::to_vec_pretty(&proof)?)
}

/// Verify a real staged feed and its manifest/transport bindings, without fetching
/// payloads, starting a build, changing installed state or enabling production.
pub fn verify_feed(variant: &Variant, directory: &Path) -> Result<serde_json::Value> {
    use production::*;
    let policy = &variant.contract.release;
    policy.validate()?;
    let production = policy
        .production
        .as_ref()
        .context("feed verification requires delegated production policy")?;
    let proof = AuthorityProof::parse(&bytes(
        &directory.join(AUTHORITY_FILE),
        MAX_AUTHORITY as u64,
    )?)?;
    let authority = proof.verify(
        production,
        &variant.contract.identity.os_id,
        &policy.channel,
        now()?,
    )?;
    let raw = bytes(&directory.join(INDEX), MAX_MANIFEST as u64)?;
    let signature = bytes(
        &directory
            .join("index-signatures")
            .join(format!("{}.sig", digest(&raw))),
        64,
    )?;
    let index = ReleaseIndexV2::authenticate(&raw, &signature, &authority, production, now()?)?;
    let mut releases = Vec::new();
    for entry in &index.releases {
        let metadata = &entry.release;
        ensure!(
            metadata.identity.distro == variant.contract.identity.os_id
                && metadata.identity.init == variant.contract.profile.init
                && metadata.identity.libc == variant.contract.profile.libc,
            "feed release differs from configured distro/profile"
        );
        let manifest = bytes(
            &directory.join(&metadata.manifest.path),
            MAX_MANIFEST as u64,
        )?;
        let signature = bytes(&directory.join(&metadata.signature.path), 64)?;
        ensure!(
            manifest.len() as u64 == metadata.manifest.size
                && digest(&manifest) == metadata.manifest.sha256
                && signature.len() as u64 == metadata.signature.size
                && digest(&signature) == metadata.signature.sha256,
            "indexed manifest/signature bytes differ"
        );
        authority.verify(SigningRole::Release, &manifest, &signature)?;
        let manifest = ReleaseManifest::parse(&manifest)?;
        manifest
            .compatibility
            .check(recab::release::RECAB_VERSION)?;
        entry.for_manifest(&manifest, production)?;
        releases.push(metadata.manifest.sha256.clone());
    }
    Ok(
        serde_json::json!({"authenticated_metadata":true,"payloads_verified":false,"production_enabled":false,
        "authority_version":authority.delegation().version,"index_sequence":index.sequence,
        "audience":index.audience,"release_ids":releases}),
    )
}
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
