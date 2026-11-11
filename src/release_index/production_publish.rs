//! Production transport and metadata publication. Payload creation stays in Recipe;
//! these operations consume explicit, already signed release sets.
use super::*;
use distro_contract::release::production::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub schema_version: u32,
    pub distro: String,
    pub source_commit: String,
    pub id: String,
    pub tag: String,
    pub entry: ProductionEntry,
}
fn configured(variant: &Variant, audience: Audience) -> Result<ProductionPolicy> {
    variant.contract.release.validate()?;
    let mut policy = variant
        .contract
        .release
        .production
        .clone()
        .context("configure production authority and delivery origins first")?;
    policy.audience = audience;
    policy.validate()?;
    Ok(policy)
}
fn authority(
    variant: &Variant,
    policy: &ProductionPolicy,
    path: &Path,
    at: u64,
    archived: bool,
) -> Result<(Vec<u8>, VerifiedAuthority)> {
    let raw = bytes(path, MAX_AUTHORITY as u64)?;
    let proof = AuthorityProof::parse(&raw)?;
    let auth = if archived {
        proof.verify_archived(
            policy,
            &variant.contract.identity.os_id,
            &variant.contract.release.channel,
        )?
    } else {
        proof.verify(
            policy,
            &variant.contract.identity.os_id,
            &variant.contract.release.channel,
            at,
        )?
    };
    Ok((raw, auth))
}
fn matches_variant(variant: &Variant, manifest: &ReleaseManifest) -> Result<()> {
    ensure!(
        manifest.identity.distro == variant.contract.identity.os_id
            && manifest.identity.architecture == Architecture::X86_64
            && manifest.identity.channel == variant.contract.release.channel
            && manifest.identity.init == variant.contract.profile.init
            && manifest.identity.libc == variant.contract.profile.libc,
        "release differs from configured x86_64 distro/profile"
    );
    manifest.compatibility.check(recab::release::RECAB_VERSION)
}
fn tag(variant: &Variant, manifest: &ReleaseManifest, id: &str) -> String {
    format!(
        "os-{}-x86_64-{}-{}-{}",
        variant.id,
        manifest.identity.flavour,
        manifest.identity.sequence,
        &id[..16]
    )
}
fn immutable(path: &Path, raw: &[u8]) -> Result<()> {
    if path.try_exists()? {
        ensure!(
            bytes(path, raw.len() as u64)? == raw,
            "immutable publication object conflict: {}",
            path.display()
        );
        Ok(())
    } else {
        write_public(path, raw)
    }
}
fn hash_file(path: &Path) -> Result<(u64, String)> {
    use sha2::{Digest, Sha256};
    let mut f = levitate_recipe::fsx::open_regular(path)?;
    let mut hash = Sha256::new();
    let size = std::io::copy(&mut f, &mut hash)?;
    Ok((size, format!("{:x}", hash.finalize())))
}
/// Validate and freeze the genuine producer's release before splitting it. No
/// release key is needed here and no manifest or payload is rebuilt/re-signed.
pub fn prepare(
    variant: &Variant,
    bundle: &Path,
    root: &Path,
    output: &Path,
    commit: &str,
    part_bytes: u64,
) -> Result<Publication> {
    ensure!(
        commit.len() == 40
            && commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "source commit must be a full lowercase Git SHA"
    );
    ensure!(
        (1..=MAX_PART).contains(&part_bytes),
        "part size must be 1..1 GiB"
    );
    let policy = configured(variant, Audience::Candidate)?;
    let raw = bytes(&bundle.join(MANIFEST), MAX_MANIFEST as u64)?;
    let sig = bytes(&bundle.join(SIGNATURE), 64)?;
    let manifest = ReleaseManifest::parse(&raw)?;
    matches_variant(variant, &manifest)?;
    let (proof, auth) = authority(
        variant,
        &policy,
        &variant.path(AUTHORITY_FILE),
        now()?,
        false,
    )?;
    auth.verify(SigningRole::Release, &raw, &sig)?;
    let root_policy: Policy = serde_json::from_slice(&bytes(&root.join(SOURCE_CONFIG), 65536)?)?;
    ensure!(
        root_policy == variant.contract.release.installed(),
        "producer root policy differs from trusted variant"
    );
    let provenance: RootfsProvenance = serde_json::from_slice(&bytes(
        &provenance_path(&bundle.join(&variant.contract.artifacts.rootfs)),
        MAX_INVENTORY + 1024 * 1024,
    )?)?;
    let verified = recab::release::verify_produced_bundle(
        root,
        &bundle.join(MANIFEST),
        None,
        false,
        &provenance,
    )?;
    let id = digest(&raw);
    ensure!(
        verified.id() == id,
        "producer verification changed release identity"
    );
    fs::create_dir_all(output.join("assets"))?;
    let tag = tag(variant, &manifest, &id);
    let mut parts = BTreeMap::new();
    for (artifact, source) in [
        (&manifest.artifacts.rootfs, verified.rootfs()),
        (&manifest.artifacts.inventory, verified.inventory_path()),
        (&manifest.artifacts.kernel, verified.kernel()),
        (&manifest.artifacts.initramfs, verified.initramfs()),
    ] {
        ensure!(
            artifact.size.div_ceil(part_bytes) <= MAX_PARTS as u64,
            "artifact exceeds 64 transport parts"
        );
        let mut input = levitate_recipe::fsx::open_regular(&source)?;
        let mut offset = 0;
        let mut members = Vec::new();
        while offset < artifact.size {
            use sha2::{Digest, Sha256};
            let size = part_bytes.min(artifact.size - offset);
            let mut hash = Sha256::new();
            let mut temp = tempfile::NamedTempFile::new_in(output.join("assets"))?;
            let mut remaining = size;
            let mut buf = [0u8; 128 * 1024];
            while remaining > 0 {
                let n = input.read(&mut buf[..remaining.min(128 * 1024) as usize])?;
                ensure!(
                    n > 0,
                    "verified payload truncated during transport publication"
                );
                hash.update(&buf[..n]);
                temp.write_all(&buf[..n])?;
                remaining -= n as u64;
            }
            let sha256 = format!("{:x}", hash.finalize());
            let name = format!(
                "{}-{}.part",
                sha256,
                Path::new(&artifact.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            );
            let dest = output.join("assets").join(&name);
            if dest.try_exists()? {
                ensure!(
                    hash_file(&dest)? == (size, sha256.clone()),
                    "existing transport part was altered"
                );
            } else {
                temp.as_file()
                    .set_permissions(fs::Permissions::from_mode(0o644))?;
                temp.as_file().sync_all()?;
                temp.persist(&dest)?;
            }
            members.push(TransportPart {
                offset,
                size,
                sha256,
                url: format!("{}{tag}/{name}", policy.asset_prefix),
            });
            offset += size;
        }
        parts.insert(artifact.path.clone(), members);
    }
    let prefix = format!("releases/{id}");
    let entry = ProductionEntry {
        release: IndexEntry {
            identity: manifest.identity.clone(),
            manifest: Artifact {
                path: format!("{prefix}/{MANIFEST}"),
                sha256: id.clone(),
                size: raw.len() as u64,
            },
            signature: Artifact {
                path: format!("{prefix}/{SIGNATURE}"),
                sha256: digest(&sig),
                size: sig.len() as u64,
            },
        },
        parts,
    };
    entry.for_manifest(&manifest, &policy)?;
    immutable(&output.join(MANIFEST), &raw)?;
    immutable(&output.join(SIGNATURE), &sig)?;
    // A resumed upload must retain every original public asset byte.
    immutable(&output.join(AUTHORITY_FILE), &proof)?;
    let publication = Publication {
        schema_version: 1,
        distro: variant.id.clone(),
        source_commit: commit.into(),
        id,
        tag,
        entry,
    };
    immutable(
        &output.join("publication.json"),
        &serde_json::to_vec_pretty(&publication)?,
    )?;
    fs::File::open(output.join("assets"))?.sync_all()?;
    Ok(publication)
}
pub fn read_publication(
    variant: &Variant,
    directory: &Path,
    audience: Audience,
) -> Result<Publication> {
    let policy = configured(variant, audience)?;
    let record: Publication = serde_json::from_slice(&bytes(
        &directory.join("publication.json"),
        MAX_MANIFEST as u64,
    )?)?;
    ensure!(
        record.schema_version == 1
            && record.distro == variant.id
            && record.source_commit.len() == 40
            && record
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid publication identity/source commit"
    );
    let raw = bytes(&directory.join(MANIFEST), MAX_MANIFEST as u64)?;
    let sig = bytes(&directory.join(SIGNATURE), 64)?;
    let manifest = ReleaseManifest::parse(&raw)?;
    matches_variant(variant, &manifest)?;
    let (_, auth) = authority(
        variant,
        &policy,
        &variant.path(AUTHORITY_FILE),
        now()?,
        false,
    )?;
    auth.verify(SigningRole::Release, &raw, &sig)?;
    ensure!(
        record.id == digest(&raw)
            && record.tag == tag(variant, &manifest, &record.id)
            && record.entry.release.manifest.path == format!("releases/{}/{MANIFEST}", record.id)
            && record.entry.release.signature.path == format!("releases/{}/{SIGNATURE}", record.id)
            && record.entry.release.manifest.sha256 == record.id
            && record.entry.release.manifest.size == raw.len() as u64
            && record.entry.release.signature.sha256 == digest(&sig)
            && record.entry.release.signature.size == 64,
        "publication manifest/signature binding differs"
    );
    record.entry.for_manifest(&manifest, &policy)?;
    for part in record.entry.parts.values().flatten() {
        let name = part
            .url
            .strip_prefix(&format!("{}{}/", policy.asset_prefix, record.tag))
            .context("transport tag differs from publication")?;
        ensure!(
            !name.contains('/')
                && name.starts_with(&format!("{}-", part.sha256))
                && name.ends_with(".part"),
            "invalid flat content-addressed asset name"
        );
    }
    Ok(record)
}
fn old_index(
    variant: &Variant,
    feed: &Path,
    policy: &ProductionPolicy,
) -> Result<(ReleaseIndexV2, RollbackFloor)> {
    let raw = bytes(&feed.join(INDEX), MAX_MANIFEST as u64)?;
    let index = ReleaseIndexV2::parse(&raw, policy)?;
    // Refresh may renew expired metadata, but must authenticate its original
    // signature/authority at issue time and preserve every selected release.
    let (_, auth) = authority(
        variant,
        policy,
        &feed.join(AUTHORITY_FILE),
        index.issued_at,
        false,
    )?;
    let sig = bytes(
        &feed
            .join("index-signatures")
            .join(format!("{}.sig", digest(&raw))),
        64,
    )?;
    ReleaseIndexV2::authenticate(&raw, &sig, &auth, policy, index.issued_at)?;
    let mut floor = RollbackFloor::default();
    floor.authority(&auth)?;
    floor.index(&index, &raw, now()?)?;
    Ok((index, floor))
}
/// Sign a new feed or renew only the current references. Feed writers lock the
/// same directory; remote workflows also share one non-cancelling concurrency group.
pub fn publish(
    variant: &Variant,
    publication: Option<&Path>,
    feed: &Path,
    audience: Audience,
    key: &Path,
) -> Result<serde_json::Value> {
    let policy = configured(variant, audience)?;
    let at = now()?;
    fs::create_dir_all(feed)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(feed.join(".index.lock"))?;
    use std::os::fd::AsRawFd;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "cannot lock production feed"
    );
    let (proof, auth) = authority(variant, &policy, &variant.path(AUTHORITY_FILE), at, false)?;
    let secret = crate::release_manifest::read_signing_key(variant, key, &[feed])?;
    auth.sign(SigningRole::Index, b"", &secret, at)?;
    let (mut index, mut floor) = if feed.join(INDEX).try_exists()? {
        old_index(variant, feed, &policy)?
    } else {
        ensure!(publication.is_some(), "cannot refresh a missing feed");
        (
            ReleaseIndexV2 {
                schema_version: 2,
                sequence: 0,
                index_epoch: auth.delegation().index_epoch,
                channel: variant.contract.release.channel.clone(),
                audience,
                issued_at: at,
                expires_at: at + 1,
                releases: vec![],
            },
            RollbackFloor::default(),
        )
    };
    floor.authority(&auth)?;
    let mut revoked = BTreeSet::new();
    let history = feed.join("authorities");
    if history.is_dir() {
        for file in fs::read_dir(history)? {
            let (_, previous) = authority(variant, &policy, &file?.path(), at, true)?;
            ensure!(
                previous.delegation().version <= auth.delegation().version
                    && previous.delegation().root_version <= auth.delegation().root_version
                    && previous.delegation().index_epoch <= auth.delegation().index_epoch,
                "authority history rollback refused"
            );
            ensure!(
                previous.delegation().version != auth.delegation().version
                    || previous.digest() == auth.digest(),
                "authority history equivocation refused"
            );
            revoked.extend(previous.delegation().revoked.iter().cloned());
        }
    }
    for key in auth
        .delegation()
        .index_keys
        .iter()
        .chain(&auth.delegation().release_keys)
    {
        ensure!(
            !revoked.contains(&key.id()?),
            "previously revoked signer cannot be reactivated"
        );
    }
    for old in &index.releases {
        let raw = bytes(&feed.join(&old.release.manifest.path), MAX_MANIFEST as u64)?;
        let sig = bytes(&feed.join(&old.release.signature.path), 64)?;
        ensure!(
            digest(&raw) == old.release.manifest.sha256
                && raw.len() as u64 == old.release.manifest.size
                && digest(&sig) == old.release.signature.sha256
                && sig.len() as u64 == old.release.signature.size,
            "selected metadata was altered"
        );
        auth.verify(SigningRole::Release, &raw, &sig)?;
        old.for_manifest(&ReleaseManifest::parse(&raw)?, &policy)?;
    }
    if let Some(directory) = publication {
        let record = read_publication(variant, directory, audience)?;
        if let Some(old) = index
            .releases
            .iter()
            .find(|e| e.release.identity.flavour == record.entry.release.identity.flavour)
        {
            ensure!(
                record.entry.release.identity.sequence > old.release.identity.sequence
                    || (record.entry.release.identity.sequence == old.release.identity.sequence
                        && record.id == old.release.manifest.sha256),
                "release sequence reuse/downgrade refused"
            );
        }
        immutable(
            &feed.join(&record.entry.release.manifest.path),
            &bytes(&directory.join(MANIFEST), MAX_MANIFEST as u64)?,
        )?;
        immutable(
            &feed.join(&record.entry.release.signature.path),
            &bytes(&directory.join(SIGNATURE), 64)?,
        )?;
        immutable(
            &feed
                .join("releases")
                .join(&record.id)
                .join("publication.json"),
            &serde_json::to_vec_pretty(&record)?,
        )?;
        index
            .releases
            .retain(|e| e.release.identity.flavour != record.entry.release.identity.flavour);
        index.releases.push(record.entry);
    }
    index
        .releases
        .sort_by_key(|e| e.release.identity.flavour.to_string());
    index.sequence = floor
        .index_sequence
        .checked_add(1)
        .context("index sequence exhausted; use root-authorized epoch recovery")?;
    index.index_epoch = auth.delegation().index_epoch;
    index.issued_at = at;
    index.expires_at = at
        .checked_add(MAX_INDEX_AGE)
        .context("clock overflow")?
        .min(auth.delegation().expires_at);
    ensure!(
        index.expires_at > at,
        "authority expires before refreshed index"
    );
    index.validate(&policy)?;
    let raw = serde_json::to_vec_pretty(&index)?;
    let sig = auth.sign(SigningRole::Index, &raw, &secret, at)?;
    floor.index(&index, &raw, at)?;
    // Retain raw historical indices outside the Pages tree in the Git metadata
    // records. Addressed signatures stay public so cached old indices still work.
    if feed.join(INDEX).exists() {
        let old = bytes(&feed.join(INDEX), MAX_MANIFEST as u64)?;
        let history = feed
            .parent()
            .context("feed needs parent")?
            .join(".index-history");
        immutable(&history.join(format!("{}.json", digest(&old))), &old)?;
    }
    immutable(
        &feed
            .join("authorities")
            .join(format!("{}.json", auth.digest())),
        &proof,
    )?;
    immutable(
        &feed
            .join("index-signatures")
            .join(format!("{}.sig", digest(&raw))),
        &sig,
    )?;
    write_public(&feed.join(AUTHORITY_FILE), &proof)?;
    write_public(&feed.join(INDEX), &raw)?; // Reachability pointer last.
    Ok(
        serde_json::json!({"index_sequence":index.sequence,"index_epoch":index.index_epoch,"index_sha256":digest(&raw),"authority_expires_at":auth.delegation().expires_at,"expires_at":index.expires_at,"production_enabled":false}),
    )
}
/// Hash public objects through the existing Recipe producer. Tokens and signing
/// keys are never inherited by the worker. Only explicit delivery origins apply.
pub fn verify_remote(
    variant: &Variant,
    directory: &Path,
    representative: bool,
) -> Result<serde_json::Value> {
    ensure!(
        [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "RECAB_INDEX_KEY_BASE64",
            "RECAB_LEVITATE_INDEX_KEY_BASE64",
            "RECAB_ACORN_INDEX_KEY_BASE64"
        ]
        .iter()
        .all(|k| std::env::var_os(k).is_none()),
        "public Recipe verification must run without GitHub/signing credentials"
    );
    let policy = configured(variant, Audience::Candidate)?;
    let record = read_publication(variant, directory, Audience::Candidate)?;
    let work = tempfile::tempdir()?;
    let mut verified = 0u64;
    for (member, parts) in &record.entry.parts {
        use sha2::{Digest, Sha256};
        let mut whole = Sha256::new();
        let mut size = 0;
        for part in parts {
            let scope = tempfile::tempdir_in(work.path())?;
            let file = acquire_object(
                variant,
                &policy,
                &part.url,
                &part.sha256,
                part.size,
                scope.path(),
            )?;
            let mut input = levitate_recipe::fsx::open_regular(&file)?;
            let mut hash = Sha256::new();
            let mut buf = [0u8; 128 * 1024];
            let mut nbytes = 0;
            loop {
                let n = input.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                nbytes += n as u64;
                hash.update(&buf[..n]);
                whole.update(&buf[..n]);
            }
            ensure!(
                nbytes == part.size && format!("{:x}", hash.finalize()) == part.sha256,
                "public transport part differs"
            );
            size += nbytes;
            verified += 1;
            if representative {
                break;
            }
        }
        if !representative {
            let manifest =
                ReleaseManifest::parse(&bytes(&directory.join(MANIFEST), MAX_MANIFEST as u64)?)?;
            let artifact = manifest
                .artifacts
                .all()
                .into_iter()
                .find(|a| a.path == *member)
                .unwrap();
            ensure!(
                size == artifact.size && format!("{:x}", whole.finalize()) == artifact.sha256,
                "public assembled artifact differs"
            );
        }
    }
    Ok(
        serde_json::json!({"release_id":record.id,"verified_parts":verified,"all_payloads_verified":!representative,"production_enabled":false}),
    )
}

fn acquire_object(
    variant: &Variant,
    policy: &ProductionPolicy,
    url: &str,
    sha256: &str,
    size: u64,
    work: &Path,
) -> Result<std::path::PathBuf> {
    let origins = policy.request_origins(url)?;
    let grants: Vec<_> = if policy.metadata_url(url).is_ok() {
        vec![policy.metadata_base.clone()]
    } else {
        std::iter::once(policy.asset_prefix.clone())
            .chain(policy.redirect_origins.iter().map(|o| format!("{o}/")))
            .collect()
    };
    let endpoints: BTreeSet<_> = origins.iter().map(|u| endpoint(u)).collect::<Result<_>>()?;
    let template = fs::read_to_string(
        variant
            .repo_root
            .join("distro/builder/os-recipes/update-acquire.rhai"),
    )?;
    let recipe = work.join("acquire.rhai");
    fs::write(
        &recipe,
        template.replace(
            "network: [] /* recab configured endpoint */",
            &format!("network: {}", serde_json::to_string(&endpoints)?),
        ),
    )?;
    let request = work.join("request.json");
    fs::write(
        &request,
        serde_json::to_vec(
            &serde_json::json!({"url":url,"size":size,"sha256":sha256,"prefixes":grants}),
        )?,
    )?;
    let mut args = vec![
        "--no-persist-state".to_string(),
        "--force-install".into(),
        "--network-max-bytes".into(),
        size.saturating_add(size / 50)
            .saturating_add(1024 * 1024)
            .to_string(),
    ];
    for endpoint in endpoints {
        args.extend(["--network-host".into(), endpoint]);
    }
    crate::recipe::Invocation::new(&variant.repo_root, "install", &recipe, work)
        .working_directory(work)
        .args(args)
        .input("request", &request)
        .run()?
        .file("artifact")
}
/// Verify exact deployed Pages bytes, signatures and representative assets.
/// The workflow checks the latest committed metadata rather than accepting a
/// stale but otherwise valid deployment as evidence of successful publication.
pub fn verify_online(
    variant: &Variant,
    feed: &Path,
    audience: Audience,
    representative: bool,
) -> Result<serde_json::Value> {
    ensure!(
        [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "RECAB_LEVITATE_INDEX_KEY_BASE64",
            "RECAB_ACORN_INDEX_KEY_BASE64"
        ]
        .iter()
        .all(|k| std::env::var_os(k).is_none()),
        "public Recipe verification requires no credentials"
    );
    let mut variant = variant.clone();
    variant
        .contract
        .release
        .production
        .as_mut()
        .context("production not configured")?
        .audience = audience;
    let value = super::verify_feed(&variant, feed)?;
    let index = ReleaseIndexV2::parse(
        &bytes(&feed.join(INDEX), MAX_MANIFEST as u64)?,
        variant.contract.release.production.as_ref().unwrap(),
    )?;
    let policy = configured(&variant, audience)?;
    let audience_name = if audience == Audience::Candidate {
        "candidate"
    } else {
        "stable"
    };
    let prefix = format!(
        "{}{}/{audience_name}/x86_64/",
        policy.metadata_base, variant.id
    );
    let raw = bytes(&feed.join(INDEX), MAX_MANIFEST as u64)?;
    let mut names = vec![
        AUTHORITY_FILE.to_string(),
        format!("index-signatures/{}.sig", digest(&raw)),
        INDEX.to_string(),
    ];
    for entry in &index.releases {
        names.extend([
            entry.release.manifest.path.clone(),
            entry.release.signature.path.clone(),
        ]);
    }
    let work = tempfile::tempdir()?;
    for name in names {
        let raw = bytes(&feed.join(&name), MAX_MANIFEST as u64)?;
        let scope = tempfile::tempdir_in(work.path())?;
        let downloaded = acquire_object(
            &variant,
            &policy,
            &format!("{prefix}{name}"),
            &digest(&raw),
            raw.len() as u64,
            scope.path(),
        )?;
        ensure!(
            bytes(&downloaded, MAX_MANIFEST as u64)? == raw,
            "deployed metadata differs from current default-branch publication"
        );
    }
    if representative {
        for entry in &index.releases {
            verify_remote(
                &variant,
                &feed.join("releases").join(&entry.release.manifest.sha256),
                true,
            )?;
        }
    }
    Ok(
        serde_json::json!({"deployed_metadata_verified":true,"representative_assets_verified":representative,"metadata":value,"production_enabled":false}),
    )
}
