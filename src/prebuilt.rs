//! Prebuilt programs of our own tools and the TUI binary.
//!
//! An OS build needs the same few programs again and again. Each is identified by a
//! **key**: the SHA-256 of everything its compile depends on (the source closure the
//! recipe would compile, the recipe, the compiler image, the target, and Recipe's
//! declared behaviour versions). Equal key means the same compile, so a result can be
//! reused or downloaded instead of compiled:
//!
//! 1. the local cache, `.artifacts/prebuilt/<family>/<key>/` (programs plus
//!    `prebuilt.json`), every file checked against its recorded SHA-256;
//! 2. the release `<family>-<key16>` of the prebuilt repository
//!    ([`DEFAULT_REPO`], or `$LEVITATE_PREBUILT_REPO`), fetched through the
//!    `prebuilt-fetch` recipe and verified again here;
//! 3. a compile, whose result then fills the local cache.
//!
//! Any failure of step 1 or 2 is a miss, never a failed build. Exact builds
//! (`release build --exact`, or any signed build) never use this: they compile and touch
//! no cache. `--no-prebuilt` skips steps 1 and 2 but still fills the cache.
//! `distro-builder prebuilt key|publish` compute a key and upload a cache entry.
//!
//! The key leaves out Ginux and Recipe's bytes on purpose: they change how a compile
//! is supervised, not what comes out of it (their declared behaviour versions are in).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::process::Cmd;

/// Version of the key's encoding and of `prebuilt.json`.
pub const FORMAT: u32 = 1;
/// The repository whose releases hold the prebuilts.
pub const DEFAULT_REPO: &str = "LevitateOS/prebuilt";
/// Overrides [`DEFAULT_REPO`] (`owner/name`).
pub const REPO_ENV: &str = "LEVITATE_PREBUILT_REPO";
/// The cargo target the tool crates compile for (`os-recipes/tool-crate.rhai`).
pub const TOOLS_TARGET: &str = "x86_64-unknown-linux-musl";
/// The archive format of a published family member: `<family>-<name>.tar.gz`.
const ARCHIVE_SUFFIX: &str = "tar.gz";

static NO_PREBUILT: AtomicBool = AtomicBool::new(false);

/// Forces compiling for the rest of this process (`--no-prebuilt`).
pub fn select_no_prebuilt() {
    NO_PREBUILT.store(true, Ordering::SeqCst);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// The install tools (`recab`, `recstrap`, ...), one entry per crate.
    Tools,
    /// The TUI binary (`install-tui`, `system-tui`).
    Tui,
}

impl Family {
    pub const NAMES: &'static str = "tools, tui";

    pub fn name(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Tui => "tui",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "tools" => Some(Self::Tools),
            "tui" => Some(Self::Tui),
            _ => None,
        }
    }
}

/// A prebuilt's identity: the hash and the named inputs it covers.
#[derive(Clone, Debug)]
pub struct Key {
    pub hex: String,
    pub inputs: BTreeMap<String, String>,
}

impl Key {
    /// The key over `fields`, in order, after the format version.
    pub fn new(fields: &[(&str, String)]) -> Self {
        let mut all = vec![("format", FORMAT.to_string())];
        all.extend(fields.iter().map(|(label, value)| (*label, value.clone())));
        let mut hash = Sha256::new();
        for (label, value) in &all {
            hash.update(format!("{label}\0{}\0", value.len()));
            hash.update(value.as_bytes());
            hash.update(b"\n");
        }
        Self {
            hex: hex(&hash.finalize()),
            inputs: all.into_iter().map(|(l, v)| (l.to_string(), v)).collect(),
        }
    }

    /// The first 16 hex digits: the release tag suffix and the printed form.
    pub fn short(&self) -> &str {
        &self.hex[..16]
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArchiveEntry {
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
}

/// `prebuilt.json`: in a cache entry and, with `archive`, as a release asset.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub format: u32,
    pub key: String,
    pub family: String,
    pub name: String,
    /// The named inputs the key covers.
    pub inputs: BTreeMap<String, String>,
    /// Source commits, where known (informational; not part of the key).
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    pub files: Vec<FileEntry>,
    /// `local` (compiled here) or `downloaded`.
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<ArchiveEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_run: Option<String>,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// SHA-256 and size of a file.
pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path).with_context(|| format!("reading '{}'", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((hex(&hash.finalize()), bytes))
}

/// SHA-256 of a file's content, for a key input.
pub fn file_digest(path: &Path) -> Result<String> {
    Ok(sha256_file(path)?.0)
}

/// A digest of a directory tree: every entry's relative path, kind, mode and
/// content (a link's target), in sorted order.
pub fn dir_digest(root: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .sort_by_file_name()
    {
        let entry = entry?;
        let relative = entry.path().strip_prefix(root)?;
        let metadata = entry.path().symlink_metadata()?;
        let mode = metadata.permissions().mode() & 0o7777;
        let (kind, content) = if metadata.file_type().is_symlink() {
            ("l", hex(fs::read_link(entry.path())?.as_os_str().as_encoded_bytes()))
        } else if metadata.is_dir() {
            ("d", String::new())
        } else if metadata.is_file() {
            ("f", file_digest(entry.path())?)
        } else {
            bail!("unsupported entry '{}' in a prebuilt source", entry.path().display());
        };
        hash.update(format!("{kind}\0{}\0{mode:o}\0{content}\n", relative.display()));
    }
    Ok(hex(&hash.finalize()))
}

/// The commit of each git checkout the `dirs` (relative to `repo`) are in, keyed by the
/// checkout's directory; a checkout with uncommitted changes gets `+dirty`.
pub fn source_commits<I: IntoIterator<Item = PathBuf>>(repo: &Path, dirs: I) -> BTreeMap<String, String> {
    let mut commits: BTreeMap<String, String> = BTreeMap::new();
    for dir in dirs {
        let absolute = repo.join(&dir);
        let git = |args: &[&str]| {
            Cmd::new("git")
                .arg("-C")
                .arg(&absolute)
                .args(args)
                .run()
                .map(|output| output.stdout.trim().to_string())
                .ok()
        };
        let (Some(top), Some(head)) = (git(&["rev-parse", "--show-toplevel"]), git(&["rev-parse", "HEAD"])) else {
            continue;
        };
        let top = Path::new(&top)
            .strip_prefix(repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf()))
            .map(|relative| relative.display().to_string())
            .unwrap_or(top);
        let dirty = git(&["status", "--porcelain", "--", "."]).is_some_and(|status| !status.is_empty());
        let value = if dirty { format!("{head}+dirty") } else { head };
        // Several directories may share a checkout: the first answer stands.
        commits.entry(if top.is_empty() { ".".to_string() } else { top }).or_insert(value);
    }
    commits
}

pub fn cache_root(repo: &Path) -> PathBuf {
    repo.join(".artifacts/prebuilt")
}

pub fn entry_dir(repo: &Path, family: Family, key: &Key) -> PathBuf {
    cache_root(repo).join(family.name()).join(&key.hex)
}

fn archive_name(family: Family, name: &str) -> String {
    format!("{}-{name}.{ARCHIVE_SUFFIX}", family.name())
}

fn release_tag(family: Family, key: &Key) -> String {
    format!("{}-{}", family.name(), key.short())
}

/// The prebuilt repository, `owner/name`.
pub fn repository() -> Result<String> {
    let repo = std::env::var(REPO_ENV).unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let valid = repo.split_once('/').is_some_and(|(owner, name)| {
        [owner, name].iter().all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    });
    if !valid {
        bail!("{REPO_ENV} must be owner/name, got '{repo}'");
    }
    Ok(repo)
}

fn plain_name(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Checks `manifest` against what is wanted and every file of `dir` against it.
fn verify_entry(dir: &Path, manifest: &Manifest, family: Family, name: &str, key: &Key, expected: &[&str]) -> Result<()> {
    if manifest.format != FORMAT {
        bail!("prebuilt.json has format {}, expected {FORMAT}", manifest.format);
    }
    if manifest.key != key.hex || manifest.family != family.name() || manifest.name != name {
        bail!("prebuilt.json is for another key, family or name");
    }
    let listed: BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    let wanted: BTreeSet<&str> = expected.iter().copied().collect();
    if listed != wanted || listed.len() != manifest.files.len() {
        bail!("prebuilt.json lists {listed:?}, expected {wanted:?}");
    }
    for file in &manifest.files {
        if !plain_name(&file.path) {
            bail!("prebuilt.json names an unsafe path '{}'", file.path);
        }
        let path = dir.join(&file.path);
        if !fs::symlink_metadata(&path)?.file_type().is_file() {
            bail!("'{}' is not a regular file", path.display());
        }
        let (sha, bytes) = sha256_file(&path)?;
        if sha != file.sha256 || bytes != file.bytes {
            bail!("'{}' does not match its recorded SHA-256", path.display());
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("reading '{}'", path.display()))?)
        .with_context(|| format!("parsing '{}'", path.display()))
}

/// A verified cache entry for `key`, or `None` (also when it fails verification).
fn lookup(repo: &Path, family: Family, name: &str, key: &Key, expected: &[&str]) -> Option<PathBuf> {
    let dir = entry_dir(repo, family, key);
    let json = dir.join("prebuilt.json");
    if !json.is_file() {
        return None;
    }
    let verified = read_manifest(&json).and_then(|m| verify_entry(&dir, &m, family, name, key, expected));
    match verified {
        Ok(()) => Some(dir),
        Err(error) => {
            crate::teprintln!("[prebuilt] {name}: ignoring the local entry for {}: {error:#}", key.short());
            None
        }
    }
}

/// Puts `files` (name, source path) into the cache entry for `key`, atomically: it is
/// built beside the entry and renamed into place, replacing an older one.
#[allow(clippy::too_many_arguments)]
fn store(
    repo: &Path,
    family: Family,
    name: &str,
    key: &Key,
    sources: BTreeMap<String, String>,
    files: &[(String, PathBuf)],
    origin: &str,
    archive: Option<ArchiveEntry>,
) -> Result<PathBuf> {
    let dest = entry_dir(repo, family, key);
    let parent = dest.parent().context("a cache entry has a parent")?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".tmp-").tempdir_in(parent)?;
    let mut entries = Vec::new();
    for (file, source) in files {
        if !plain_name(file) {
            bail!("unsafe prebuilt file name '{file}'");
        }
        let target = staging.path().join(file);
        fs::copy(source, &target).with_context(|| format!("copying '{}'", source.display()))?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
        let (sha256, bytes) = sha256_file(&target)?;
        entries.push(FileEntry { path: file.clone(), sha256, bytes });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = Manifest {
        format: FORMAT,
        key: key.hex.clone(),
        family: family.name().into(),
        name: name.into(),
        inputs: key.inputs.clone(),
        sources,
        files: entries,
        origin: origin.into(),
        archive,
        ci_run: None,
    };
    fs::write(staging.path().join("prebuilt.json"), serde_json::to_vec_pretty(&manifest)?)?;
    fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o755))?;
    let staged = staging.keep();
    let aside = parent.join(format!(".old-{}-{}", std::process::id(), key.short()));
    let had_old = dest.exists();
    let moved = (|| -> std::io::Result<()> {
        if had_old {
            fs::rename(&dest, &aside)?;
        }
        fs::rename(&staged, &dest)
    })();
    if let Err(error) = moved {
        let _ = fs::remove_dir_all(&staged);
        if had_old && !dest.exists() {
            let _ = fs::rename(&aside, &dest);
        }
        return Err(error).with_context(|| format!("publishing the cache entry '{}'", dest.display()));
    }
    if had_old {
        let _ = fs::remove_dir_all(&aside);
    }
    Ok(dest)
}

/// Fetches the release of `key` through the `prebuilt-fetch` recipe, verifies it and
/// stores it in the cache. An error is a miss.
fn fetch(repo: &Path, family: Family, name: &str, key: &Key, expected: &[&str]) -> Result<PathBuf> {
    let slug = repository()?;
    let base = format!("https://github.com/{slug}/releases/download/{}", release_tag(family, key));
    let archive = archive_name(family, name);
    let work = repo
        .join(".artifacts/work/prebuilt-fetch")
        .join(family.name())
        .join(format!("{name}-{}", key.short()));
    let _ = fs::remove_dir_all(&work);
    let outcome = (|| -> Result<PathBuf> {
        let recipe = repo.join("distro/builder/os-recipes/prebuilt-fetch.rhai");
        let result = crate::recipe::Invocation::new(repo, "install", &recipe, &work)
            .define("BASE_URL", &base)
            .define("KEY", &key.hex)
            .define("ARCHIVE", &archive)
            .run()?;
        let files = result.directory("files")?;
        let manifest = read_manifest(&result.file("manifest")?)?;
        verify_entry(&files, &manifest, family, name, key, expected)?;
        let archive_entry = manifest.archive.clone().context("prebuilt.json names no archive")?;
        let sources = manifest.sources.clone();
        let list: Vec<(String, PathBuf)> = manifest.files.iter().map(|f| (f.path.clone(), files.join(&f.path))).collect();
        store(repo, family, name, key, sources, &list, "downloaded", Some(archive_entry))
    })();
    let _ = fs::remove_dir_all(&work);
    outcome
}

/// The programs of one prebuilt, wherever they are.
pub struct Obtained {
    files: BTreeMap<String, PathBuf>,
}

impl Obtained {
    pub fn path(&self, name: &str) -> Result<&Path> {
        self.files.get(name).map(PathBuf::as_path).with_context(|| format!("no prebuilt file '{name}'"))
    }
}

/// The programs `expected` for `family`/`name`: from the local cache, else downloaded,
/// else `compile`d (which returns each file's name and path) and cached. `key` is
/// computed only when needed. See the module docs for exact and `--no-prebuilt`.
pub fn obtain(
    repo: &Path,
    family: Family,
    name: &str,
    key: impl FnOnce() -> Result<Key>,
    expected: &[&str],
    sources: impl FnOnce() -> BTreeMap<String, String>,
    compile: impl FnOnce() -> Result<Vec<(String, PathBuf)>>,
) -> Result<Obtained> {
    let step = crate::timeline::Step::begin("prebuilt", &format!("{} {name}", family.name()));
    let obtained = obtain_inner(repo, family, name, &step, key, expected, sources, compile)?;
    step.finish();
    Ok(obtained)
}

#[allow(clippy::too_many_arguments)]
fn obtain_inner(
    repo: &Path,
    family: Family,
    name: &str,
    step: &crate::timeline::Step,
    key: impl FnOnce() -> Result<Key>,
    expected: &[&str],
    sources: impl FnOnce() -> BTreeMap<String, String>,
    compile: impl FnOnce() -> Result<Vec<(String, PathBuf)>>,
) -> Result<Obtained> {
    let collect = |files: Vec<(String, PathBuf)>| Obtained { files: files.into_iter().collect() };
    if crate::recipe::exact_identity() {
        crate::tprintln!("[prebuilt] {name}: compiling (exact build: nothing prebuilt is used)");
        return Ok(collect(compile()?));
    }
    let key = key()?;
    step.key(key.short());
    let short = key.short().to_string();
    let from_dir = |dir: PathBuf| Obtained {
        files: expected.iter().map(|file| (file.to_string(), dir.join(file))).collect(),
    };
    if NO_PREBUILT.load(Ordering::SeqCst) {
        crate::tprintln!("[prebuilt] {name}: compiling {short} (--no-prebuilt)");
    } else {
        if let Some(dir) = lookup(repo, family, name, &key, expected) {
            crate::tprintln!("[prebuilt] {name}: local {short}");
            return Ok(from_dir(dir));
        }
        match fetch(repo, family, name, &key, expected) {
            Ok(dir) => {
                crate::tprintln!("[prebuilt] {name}: downloaded {short}");
                return Ok(from_dir(dir));
            }
            Err(error) => {
                crate::tprintln!("[prebuilt] {name}: no prebuilt for {short}, compiling");
                let reason = format!("{error:#}");
                crate::teprintln!("  ({})", reason.lines().next().unwrap_or("no reason given"));
            }
        }
    }
    let compiled = compile()?;
    let dir = store(repo, family, name, &key, sources(), &compiled, "local", None)
        .with_context(|| format!("caching the compiled {name}"))?;
    crate::tprintln!("[prebuilt] {name}: compiled {short}");
    Ok(from_dir(dir))
}

/// The key of `family`'s `name` on the checkout at `repo`. `distro` selects the libc
/// of a TUI binary.
pub fn key_for(repo: &Path, family: Family, name: &str, distro: Option<&str>) -> Result<Key> {
    match family {
        Family::Tools => crate::recipe::tools::prebuilt_key(repo, name),
        Family::Tui => {
            let distro = distro.context("a tui key depends on the distro's libc: pass --distro <id>")?;
            let variant = distro_contract::load(repo, distro)?;
            crate::tui_binary::prebuilt_key(repo, name, variant.contract.profile.libc)
        }
    }
}

/// The names `expected` for `family`/`name`: the files its entry holds.
fn expected_files(family: Family, name: &str) -> Result<Vec<String>> {
    match family {
        Family::Tools => crate::recipe::tools::tool_binaries(name),
        Family::Tui => Ok(vec!["program".into()]),
    }
}

/// Uploads the verified cache entry of `key` to the release `<family>-<key16>` of the
/// prebuilt repository with `gh`: creates the release if missing, never replaces an
/// asset with different bytes (it refuses), uploads the archive and then `prebuilt.json`.
pub fn publish(repo: &Path, family: Family, name: &str, key: &Key) -> Result<()> {
    let slug = repository()?;
    let tag = release_tag(family, key);
    let dir = entry_dir(repo, family, key);
    let expected = expected_files(family, name)?;
    let expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
    let mut manifest = read_manifest(&dir.join("prebuilt.json")).with_context(|| {
        format!(
            "no cache entry for {} {name} {}: build it first (a build, or `release build --no-prebuilt`)",
            family.name(),
            key.short()
        )
    })?;
    verify_entry(&dir, &manifest, family, name, key, &expected_refs)?;

    let staging = tempfile::Builder::new().prefix("prebuilt-publish-").tempdir()?;
    let archive_file = staging.path().join(archive_name(family, name));
    make_archive(&dir, &expected, &archive_file)?;
    let (archive_sha, archive_bytes) = sha256_file(&archive_file)?;
    manifest.archive = Some(ArchiveEntry {
        name: archive_name(family, name),
        sha256: archive_sha.clone(),
        bytes: archive_bytes,
    });
    manifest.ci_run = match (
        std::env::var("GITHUB_SERVER_URL"),
        std::env::var("GITHUB_REPOSITORY"),
        std::env::var("GITHUB_RUN_ID"),
    ) {
        (Ok(server), Ok(repository), Ok(run)) => Some(format!("{server}/{repository}/actions/runs/{run}")),
        _ => None,
    };
    let manifest_file = staging.path().join("prebuilt.json");
    fs::write(&manifest_file, serde_json::to_vec_pretty(&manifest)?)?;

    let listing = Cmd::new("gh")
        .args(["release", "view", &tag, "-R", &slug, "--json", "assets", "-q", ".assets[].name"])
        .run();
    let existing: Vec<String> = match listing {
        Ok(output) => output.stdout.lines().map(str::to_string).collect(),
        Err(error) if format!("{error:#}").contains("release not found") => {
            Cmd::new("gh")
                .args(["release", "create", &tag, "-R", &slug, "--latest=false"])
                .args(["--title", &format!("{} {name} {}", family.name(), key.short())])
                .args(["--notes", &format!("Prebuilt {} {name}, key {}", family.name(), key.hex)])
                .error_msg(format!("creating release {tag} in {slug}"))
                .run()?;
            crate::tprintln!("[prebuilt] created release {tag} in {slug}");
            Vec::new()
        }
        Err(error) => return Err(error.context(format!("looking up release {tag} in {slug}"))),
    };

    // Decide everything before uploading anything.
    let mut upload = Vec::new();
    for (asset, local) in [(archive_name(family, name), &archive_file), ("prebuilt.json".to_string(), &manifest_file)] {
        if !existing.contains(&asset) {
            upload.push((asset, local.clone()));
            continue;
        }
        let fetched = tempfile::Builder::new().prefix("prebuilt-existing-").tempdir()?;
        Cmd::new("gh")
            .args(["release", "download", &tag, "-R", &slug, "-p", &asset, "-D"])
            .arg(fetched.path())
            .error_msg(format!("downloading the existing {asset} of {tag}"))
            .run()?;
        let remote = fetched.path().join(&asset);
        if sha256_file(&remote)?.0 == sha256_file(local)?.0 {
            crate::tprintln!("[prebuilt] {asset} is already published with these bytes");
        } else if asset == "prebuilt.json" && same_prebuilt(&remote, key, &archive_sha) {
            crate::tprintln!("[prebuilt] {asset} is already published (same key and archive, other details); keeping it");
        } else {
            bail!("refusing to replace {asset} of release {tag} in {slug}: it holds different bytes");
        }
    }
    for (asset, local) in &upload {
        Cmd::new("gh")
            .args(["release", "upload", &tag, "-R", &slug])
            .arg(local)
            .error_msg(format!("uploading {asset} to release {tag} in {slug}"))
            .run()?;
        crate::tprintln!("[prebuilt] uploaded {asset} to {slug} release {tag}");
    }
    crate::tprintln!("[prebuilt] {} {name}: published {} in {slug}", family.name(), key.short());
    Ok(())
}

/// Whether the published `prebuilt.json` at `path` is for this key and archive.
fn same_prebuilt(path: &Path, key: &Key, archive_sha: &str) -> bool {
    read_manifest(path).is_ok_and(|m| m.key == key.hex && m.archive.is_some_and(|a| a.sha256 == archive_sha))
}

/// The same bytes for the same files: names sorted, owners and times zeroed, no
/// gzip name or time.
fn make_archive(dir: &Path, files: &[String], output: &Path) -> Result<()> {
    use std::process::{Command, Stdio};
    let mut names = files.to_vec();
    names.sort();
    let mut tar = Command::new("tar")
        .args(["--sort=name", "--mtime=@0", "--owner=0", "--group=0", "--numeric-owner", "--format=gnu"])
        .args(["-cf", "-", "-C"])
        .arg(dir)
        .arg("--")
        .args(&names)
        .stdout(Stdio::piped())
        .spawn()
        .context("running tar")?;
    let gzip = Command::new("gzip")
        .args(["-n", "-9", "-c"])
        .stdin(tar.stdout.take().context("tar has no output")?)
        .stdout(fs::File::create(output)?)
        .status()
        .context("running gzip")?;
    let tar = tar.wait()?;
    if !tar.success() || !gzip.success() {
        bail!("packing '{}' failed (tar {tar}, gzip {gzip})", dir.display());
    }
    Ok(())
}
