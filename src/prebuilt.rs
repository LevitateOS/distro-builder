//! Prebuilt programs of our own tools, the TUI binary, the desktop apps (Qt) and the
//! display server compiled from source (XLibre on AcornOS).
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
//! `distro-builder prebuilt key|publish` compute a key and upload a cache entry;
//! `prebuilt list|build` name every part and compile one into the cache, which is
//! what CI runs before it publishes.
//!
//! The desktop apps family is a whole tree (programs, menu entries, polkit actions and
//! Recipe's target metadata), one entry per distro, `desktop-apps/<key>/{prebuilt.json,tree/}`.
//! Its entries record every path with kind, exact mode, size and SHA-256, and verification
//! compares the whole tree both ways (no missing and no extra entries). See
//! [`desktop_apps_key`], [`lookup_desktop_apps`] and [`store_desktop_apps`]. The display
//! server family is such a tree too, one entry per distro that names a
//! `desktop.display_server_recipe_script` ([`display_server_key`], [`lookup_display_server`],
//! [`store_display_server`]).
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
    /// The Qt desktop apps tree (`gui/`), one entry per distro: its `name` is the distro id.
    DesktopApps,
    /// The display server tree compiled from source (`desktop.display_server_recipe_script`),
    /// one entry per distro that has one: its `name` is the distro id.
    DisplayServer,
}

impl Family {
    pub const NAMES: &'static str = "tools, tui, desktop-apps, display-server";

    pub fn name(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Tui => "tui",
            Self::DesktopApps => "desktop-apps",
            Self::DisplayServer => "display-server",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "tools" => Some(Self::Tools),
            "tui" => Some(Self::Tui),
            "desktop-apps" => Some(Self::DesktopApps),
            "display-server" => Some(Self::DisplayServer),
            _ => None,
        }
    }

    /// Whether the family's name is a distro id (one tree per distro).
    pub fn per_distro(self) -> bool {
        matches!(self, Self::DesktopApps | Self::DisplayServer)
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

/// One file of a prebuilt. Flat families list regular files only (`kind` `file`, no mode);
/// a tree lists every entry: `file`, `dir` or `symlink`, with the exact permission bits of
/// files and directories and, for a link, its target (its SHA-256 is the target's).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    #[serde(default = "file_kind", skip_serializing_if = "is_file_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

fn file_kind() -> String {
    "file".into()
}

fn is_file_kind(kind: &String) -> bool {
    kind == "file"
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

/// What a family's entry holds.
enum Payload {
    /// Regular files directly in the payload directory, by name.
    Files(Vec<String>),
    /// A whole tree, verified entry by entry with exact modes.
    Tree,
}

impl Payload {
    fn of(family: Family, name: &str) -> Result<Self> {
        match family {
            Family::Tools => Ok(Self::Files(crate::recipe::tools::tool_binaries(name)?)),
            Family::Tui => Ok(Self::Files(vec!["program".into()])),
            Family::DesktopApps | Family::DisplayServer => Ok(Self::Tree),
        }
    }
}

/// The directory of a cache entry that holds the payload: the entry itself for files,
/// `tree/` for a tree (beside `prebuilt.json`).
fn payload_dir(entry: &Path, payload: &Payload) -> PathBuf {
    match payload {
        Payload::Files(_) => entry.to_path_buf(),
        Payload::Tree => entry.join("tree"),
    }
}

/// Checks `manifest` against what is wanted and every file of `dir` (the payload
/// directory) against it.
fn verify_entry(dir: &Path, manifest: &Manifest, family: Family, name: &str, key: &Key, payload: &Payload) -> Result<()> {
    if manifest.format != FORMAT {
        bail!("prebuilt.json has format {}, expected {FORMAT}", manifest.format);
    }
    if manifest.key != key.hex || manifest.family != family.name() || manifest.name != name {
        bail!("prebuilt.json is for another key, family or name");
    }
    let expected = match payload {
        Payload::Tree => return verify_tree(dir, manifest),
        Payload::Files(expected) => expected,
    };
    let listed: BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    let wanted: BTreeSet<&str> = expected.iter().map(String::as_str).collect();
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

/// Every entry of the tree at `root` below it, sorted by path: kind, exact mode (files and
/// directories), size and SHA-256 (a link's: of its target). Only files, directories and
/// symlinks may exist.
fn scan_tree(root: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1).sort_by_file_name() {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?.to_str().context("a non-Unicode path in a prebuilt tree")?.to_string();
        let metadata = fs::symlink_metadata(path)?;
        let file_type = metadata.file_type();
        let mode = metadata.permissions().mode() & 0o7777;
        entries.push(if file_type.is_symlink() {
            let target = fs::read_link(path)?.to_str().context("a non-Unicode link target in a prebuilt tree")?.to_string();
            FileEntry {
                path: relative,
                sha256: hex(&Sha256::digest(target.as_bytes())),
                bytes: target.len() as u64,
                kind: "symlink".into(),
                mode: None,
                target: Some(target),
            }
        } else if file_type.is_dir() {
            FileEntry {
                path: relative,
                sha256: hex(&Sha256::digest(b"")),
                bytes: 0,
                kind: "dir".into(),
                mode: Some(mode),
                target: None,
            }
        } else if file_type.is_file() {
            let (sha256, bytes) = sha256_file(path)?;
            FileEntry { path: relative, sha256, bytes, kind: "file".into(), mode: Some(mode), target: None }
        } else {
            bail!("unsupported entry '{}' in a prebuilt tree (only files, directories and links)", path.display());
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Compares the tree at `dir` with `manifest.files`, both ways: every listed entry exists
/// with its recorded kind, mode, size, SHA-256 and target, and nothing else exists.
fn verify_tree(dir: &Path, manifest: &Manifest) -> Result<()> {
    let mut listed = BTreeMap::new();
    for entry in &manifest.files {
        if !plain_name(&entry.path) {
            bail!("prebuilt.json names an unsafe path '{}'", entry.path);
        }
        if listed.insert(entry.path.as_str(), entry).is_some() {
            bail!("prebuilt.json lists '{}' twice", entry.path);
        }
    }
    if !fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        bail!("'{}' is not a directory", dir.display());
    }
    let actual = scan_tree(dir)?;
    for found in &actual {
        match listed.remove(found.path.as_str()) {
            None => bail!("'{}' is in the tree but not in prebuilt.json (an extra entry)", found.path),
            Some(want) if want != found => bail!(
                "'{}' does not match prebuilt.json: has {} mode {:?} {} bytes sha256 {}, recorded {} mode {:?} {} bytes sha256 {}",
                found.path, found.kind, found.mode.map(|m| format!("{m:o}")), found.bytes, found.sha256,
                want.kind, want.mode.map(|m| format!("{m:o}")), want.bytes, want.sha256
            ),
            Some(_) => {}
        }
    }
    if let Some(missing) = listed.keys().next() {
        bail!("'{missing}' is in prebuilt.json but missing from the tree");
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("reading '{}'", path.display()))?)
        .with_context(|| format!("parsing '{}'", path.display()))
}

/// A verified cache entry for `key`, or `None` (also when it fails verification).
/// (The entry directory; for a tree its payload is `tree/` inside it.)
fn lookup(repo: &Path, family: Family, name: &str, key: &Key, payload: &Payload) -> Option<PathBuf> {
    let dir = entry_dir(repo, family, key);
    let json = dir.join("prebuilt.json");
    if !json.is_file() {
        return None;
    }
    let verified = read_manifest(&json).and_then(|m| verify_entry(&payload_dir(&dir, payload), &m, family, name, key, payload));
    match verified {
        Ok(()) => Some(dir),
        Err(error) => {
            crate::teprintln!("[prebuilt] {name}: ignoring the local entry for {}: {error:#}", key.short());
            None
        }
    }
}

/// What goes into a cache entry.
enum Content<'a> {
    /// Files (name, source path), made executable.
    Files(&'a [(String, PathBuf)]),
    /// A tree, copied with exact modes and links.
    Tree(&'a Path),
}

/// Copies the tree `source` to `dest` (which must not exist) with every mode exact and
/// symlinks kept as links; hard links become separate files.
fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir(dest)?;
    let mut directories = vec![(dest.to_path_buf(), fs::symlink_metadata(source)?.permissions())];
    for entry in walkdir::WalkDir::new(source).follow_links(false).min_depth(1).sort_by_file_name() {
        let entry = entry?;
        let target = dest.join(entry.path().strip_prefix(source)?);
        let metadata = fs::symlink_metadata(entry.path())?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else if file_type.is_dir() {
            fs::create_dir(&target)?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
            directories.push((target, metadata.permissions()));
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).with_context(|| format!("copying '{}'", entry.path().display()))?;
            fs::set_permissions(&target, metadata.permissions())?;
        } else {
            bail!("unsupported entry '{}' in a prebuilt tree (only files, directories and links)", entry.path().display());
        }
    }
    // Directories last, deepest first, so a read-only one does not stop its children.
    for (path, permissions) in directories.into_iter().rev() {
        fs::set_permissions(&path, permissions)?;
    }
    Ok(())
}

/// Puts `content` into the cache entry for `key`, atomically: it is built beside the entry
/// and renamed into place, replacing an older one.
#[allow(clippy::too_many_arguments)]
fn store(
    repo: &Path,
    family: Family,
    name: &str,
    key: &Key,
    sources: BTreeMap<String, String>,
    content: Content<'_>,
    origin: &str,
    archive: Option<ArchiveEntry>,
) -> Result<PathBuf> {
    let dest = entry_dir(repo, family, key);
    let parent = dest.parent().context("a cache entry has a parent")?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".tmp-").tempdir_in(parent)?;
    let mut entries = Vec::new();
    match content {
        Content::Files(files) => {
            for (file, source) in files {
                if !plain_name(file) {
                    bail!("unsafe prebuilt file name '{file}'");
                }
                let target = staging.path().join(file);
                fs::copy(source, &target).with_context(|| format!("copying '{}'", source.display()))?;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
                let (sha256, bytes) = sha256_file(&target)?;
                entries.push(FileEntry { path: file.clone(), sha256, bytes, kind: file_kind(), mode: None, target: None });
            }
            entries.sort_by(|a, b| a.path.cmp(&b.path));
        }
        Content::Tree(source) => {
            let tree = staging.path().join("tree");
            copy_tree(source, &tree).with_context(|| format!("copying the tree '{}'", source.display()))?;
            entries = scan_tree(&tree)?;
        }
    }
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
fn fetch(repo: &Path, family: Family, name: &str, key: &Key, payload: &Payload) -> Result<PathBuf> {
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
        verify_entry(&files, &manifest, family, name, key, payload)?;
        let archive_entry = manifest.archive.clone().context("prebuilt.json names no archive")?;
        let sources = manifest.sources.clone();
        match payload {
            Payload::Files(_) => {
                let list: Vec<(String, PathBuf)> =
                    manifest.files.iter().map(|f| (f.path.clone(), files.join(&f.path))).collect();
                store(repo, family, name, key, sources, Content::Files(&list), "downloaded", Some(archive_entry))
            }
            Payload::Tree => store(repo, family, name, key, sources, Content::Tree(&files), "downloaded", Some(archive_entry)),
        }
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
    let payload = Payload::Files(expected.iter().map(|file| file.to_string()).collect());
    let from_dir = |dir: PathBuf| Obtained {
        files: expected.iter().map(|file| (file.to_string(), dir.join(file))).collect(),
    };
    if NO_PREBUILT.load(Ordering::SeqCst) {
        crate::tprintln!("[prebuilt] {name}: compiling {short} (--no-prebuilt)");
    } else {
        if let Some(dir) = lookup(repo, family, name, &key, &payload) {
            crate::tprintln!("[prebuilt] {name}: local {short}");
            return Ok(from_dir(dir));
        }
        match fetch(repo, family, name, &key, &payload) {
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
    let dir = store(repo, family, name, &key, sources(), Content::Files(&compiled), "local", None)
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
        Family::DesktopApps => desktop_apps_key(repo, &distro_contract::load(repo, name)?),
        Family::DisplayServer => display_server_key(repo, &distro_contract::load(repo, name)?),
    }
}

/// One part CI publishes: a family, a name, and for a TUI binary the distro whose libc
/// it is built for.
pub struct Part {
    pub family: Family,
    pub name: String,
    pub distro: Option<String>,
}

/// Every prebuilt part: each tool package, the desktop apps of every distro, the display
/// server of every distro that compiles one, and each TUI binary once per libc (the first
/// distro with that libc stands for the others: the key depends on the libc alone).
pub fn list(repo: &Path) -> Result<Vec<Part>> {
    let mut parts: Vec<Part> = crate::recipe::tools::tool_packages()
        .into_iter()
        .map(|name| Part { family: Family::Tools, name: name.to_string(), distro: None })
        .collect();
    let mut libcs = BTreeSet::new();
    for id in distro_contract::distro_ids(repo) {
        parts.push(Part { family: Family::DesktopApps, name: id.clone(), distro: None });
        let variant = distro_contract::load(repo, &id)?;
        if variant.contract.desktop.display_server_recipe_script.is_some() {
            parts.push(Part { family: Family::DisplayServer, name: id.clone(), distro: None });
        }
        if !libcs.insert(variant.contract.profile.libc.bun_target()) {
            continue;
        }
        for (name, _) in tui_binaries() {
            parts.push(Part { family: Family::Tui, name: name.to_string(), distro: Some(id.clone()) });
        }
    }
    Ok(parts)
}

/// The TUI binaries: file name and path in an image.
fn tui_binaries() -> [(&'static str, &'static str); 2] {
    let stem = |path: &'static str| Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
    [
        (stem(crate::tui_binary::INSTALL_BINARY), crate::tui_binary::INSTALL_BINARY),
        (stem(crate::tui_binary::SYSTEM_BINARY), crate::tui_binary::SYSTEM_BINARY),
    ]
}

/// Compiles the TUI binary `name` for `libc` through `tui_binary::install` (which fills
/// the cache) into a scratch layer. `install` wants the libraries the binary links on its
/// image; nothing runs here, so empty stand-ins satisfy that check.
fn build_tui(repo: &Path, name: &str, libc: distro_contract::profile::Libc) -> Result<()> {
    let (_, binary_path) = tui_binaries()
        .into_iter()
        .find(|(stem, _)| *stem == name)
        .with_context(|| format!("unknown TUI binary '{name}'; expected install-tui or system-tui"))?;
    let workspace = crate::tui_apps::Workspace::load(repo)?;
    let apps: Vec<&crate::tui_apps::TuiApp> = if binary_path == crate::tui_binary::INSTALL_BINARY {
        crate::install_ux::Screen::resolve(&workspace)?.apps()
    } else {
        distro_contract::SYSTEM_TUI_APPS
            .iter()
            .map(|app| workspace.app(app))
            .collect::<Result<_>>()?
    };
    let scratch = tempfile::tempdir()?;
    let rootfs = scratch.path().join("rootfs");
    fs::create_dir_all(rootfs.join("usr/lib"))?;
    for lib in libc.bun_libs() {
        fs::write(rootfs.join("usr/lib").join(lib), b"")?;
    }
    let layer = scratch.path().join("layer");
    fs::create_dir_all(&layer)?;
    crate::tui_binary::install(repo, &rootfs, &layer, binary_path, libc, &apps)
}

/// Compiles `family`'s `name` into the local cache, through the path an ISO build uses
/// and nothing else (no ISO, no other step, no download), and returns its key and the
/// cache directory. The key is computed from the checkout again afterwards and the
/// entry verified under it, so what CI then publishes is what was compiled.
pub fn build(repo: &Path, family: Family, name: &str, distro: Option<&str>) -> Result<(Key, PathBuf)> {
    if crate::recipe::exact_identity() {
        bail!("an exact (signed) build never fills the prebuilt cache; unset the signing key");
    }
    select_no_prebuilt();
    match family {
        Family::DesktopApps => {
            let variant = distro_contract::load(repo, name)?;
            crate::desktop_apps::build_cache(&variant)?;
        }
        Family::DisplayServer => {
            let variant = distro_contract::load(repo, name)?;
            crate::display_server::build_cache(&variant)?;
        }
        Family::Tools => {
            crate::recipe::tools::compile_crate(repo, name)?;
        }
        Family::Tui => {
            let distro = distro.context("a tui build depends on the distro's libc: pass --distro <id>")?;
            let variant = distro_contract::load(repo, distro)?;
            build_tui(repo, name, variant.contract.profile.libc)?;
        }
    }
    let key = key_for(repo, family, name, distro)?;
    let payload = Payload::of(family, name)?;
    let dir = lookup(repo, family, name, &key, &payload).with_context(|| {
        format!(
            "{} {name} compiled, but the cache holds no verified entry for key {} (the source changed during the build?)",
            family.name(),
            key.short()
        )
    })?;
    Ok((key, dir))
}

/// Uploads the verified cache entry of `key` to the release `<family>-<key16>` of the
/// prebuilt repository with `gh`: creates the release if missing, never replaces an
/// asset with different bytes (it refuses), uploads the archive and then `prebuilt.json`.
pub fn publish(repo: &Path, family: Family, name: &str, key: &Key) -> Result<()> {
    let slug = repository()?;
    let tag = release_tag(family, key);
    let dir = entry_dir(repo, family, key);
    let payload = Payload::of(family, name)?;
    let mut manifest = read_manifest(&dir.join("prebuilt.json")).with_context(|| {
        format!(
            "no cache entry for {} {name} {}: build it first (a build, or `release build --no-prebuilt`)",
            family.name(),
            key.short()
        )
    })?;
    verify_entry(&payload_dir(&dir, &payload), &manifest, family, name, key, &payload)?;

    let staging = tempfile::Builder::new().prefix("prebuilt-publish-").tempdir()?;
    let archive_file = staging.path().join(archive_name(family, name));
    make_archive(&payload_dir(&dir, &payload), &payload, &archive_file)?;
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
/// gzip name or time. A tree's archive holds its contents at the root (`./usr/...`), with
/// exact modes and links, hard links stored as separate files.
fn make_archive(dir: &Path, payload: &Payload, output: &Path) -> Result<()> {
    use std::process::{Command, Stdio};
    let names = match payload {
        Payload::Files(files) => {
            let mut names = files.to_vec();
            names.sort();
            names
        }
        Payload::Tree => vec![".".to_string()],
    };
    let mut tar = Command::new("tar")
        .args(["--sort=name", "--mtime=@0", "--owner=0", "--group=0", "--numeric-owner", "--format=gnu"])
        .args(["--hard-dereference", "-cf", "-", "-C"])
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

// ---------------------------------------------------------------------------
// The desktop apps (Qt) family: one tree per distro.
// ---------------------------------------------------------------------------

/// The recipes whose bytes decide what the desktop apps compile in: `recipe` and everything
/// it names with `import "..."` or a `//! extends:` header, transitively. Names resolve next
/// to the importing recipe, else under `os-recipes`. Keyed by path relative to `repo`.
fn recipe_closure(repo: &Path, recipe: &Path, found: &mut BTreeMap<String, String>) -> Result<()> {
    let label = recipe.strip_prefix(repo).unwrap_or(recipe).display().to_string();
    if found.contains_key(&label) {
        return Ok(());
    }
    let text = fs::read_to_string(recipe).with_context(|| format!("reading '{}'", recipe.display()))?;
    // A lock's `locked_on()` date says when it was re-locked, not what it locks:
    // leave it out, so a re-lock that changes no package changes no key.
    let digest = if label.ends_with("-lock.rhai") {
        let mut hash = Sha256::new();
        for line in text.lines().filter(|line| !line.trim_start().starts_with("fn locked_on()")) {
            hash.update(line.as_bytes());
            hash.update(b"\n");
        }
        hex(&hash.finalize())
    } else {
        file_digest(recipe)?
    };
    found.insert(label, digest);
    let dir = recipe.parent().context("a recipe has a directory")?;
    let os_recipes = repo.join("distro/builder/os-recipes");
    for line in text.lines() {
        let line = line.trim_start();
        let named = if line.starts_with("import ") {
            line.split('"').nth(1)
        } else {
            line.strip_prefix("//! extends:").map(str::trim)
        };
        let Some(named) = named.filter(|name| !name.is_empty()) else { continue };
        let path = [dir.join(named), os_recipes.join(named)]
            .into_iter()
            .find(|path| path.is_file())
            .with_context(|| format!("'{}' names '{named}', which is not a file", recipe.display()))?;
        recipe_closure(repo, &path, found)?;
    }
    Ok(())
}

/// One digest over the closure's `label=digest` lines.
fn closure_digest(files: &BTreeMap<String, String>) -> String {
    let mut hash = Sha256::new();
    for (label, digest) in files {
        hash.update(format!("{label}\0{digest}\n"));
    }
    hex(&hash.finalize())
}

/// A digest of `gui/`: every file git tracks or would add (untracked and not ignored), by
/// path, exact mode and worktree content (a link's target), so local edits change it. A
/// tracked file deleted in the worktree is left out. Everything under `gui/` counts, the
/// `fixtures/` stand-ins too, though the compile's source copy leaves them out: a change
/// there misses once, which is harmless.
fn gui_digest(repo: &Path) -> Result<String> {
    // gui/ is a submodule (LevitateOS/desktop-apps): git lists its files from
    // inside it; they are named as before, under gui/, so the key is the same.
    let listing = Cmd::new("git")
        .arg("-C")
        .arg(repo.join("gui"))
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .error_msg("listing the files of gui/ with git")
        .run()?;
    let paths: BTreeSet<String> = listing
        .stdout
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| format!("gui/{path}"))
        .collect();
    if paths.is_empty() {
        bail!("git lists no files under gui/ in '{}'", repo.display());
    }
    let mut hash = Sha256::new();
    for path in paths {
        let full = repo.join(&path);
        let metadata = match fs::symlink_metadata(&full) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("reading '{}'", full.display())),
        };
        let mode = metadata.permissions().mode() & 0o7777;
        let (kind, content) = if metadata.file_type().is_symlink() {
            ("l", hex(fs::read_link(&full)?.as_os_str().as_encoded_bytes()))
        } else if metadata.is_dir() {
            ("d", String::new())
        } else if metadata.is_file() {
            ("f", file_digest(&full)?)
        } else {
            bail!("unsupported entry '{}' under gui/", full.display());
        };
        hash.update(format!("{kind}\0{path}\0{mode:o}\0{content}\n"));
    }
    Ok(hex(&hash.finalize()))
}

/// The lock files (`sources/*-lock.rhai`) a recipe closure contains, as one digest.
fn lock_digest(files: &BTreeMap<String, String>) -> String {
    closure_digest(&files.iter().filter(|(label, _)| label.ends_with("-lock.rhai")).map(|(l, d)| (l.clone(), d.clone())).collect())
}

/// What of Recipe's declared identity a prebuilt key carries: only the two
/// behaviour versions, which are bumped exactly when a Recipe or Ginux change can
/// alter what a compile produces. The rest of `identity::declared()` (Ginux's
/// contract digests, transport and catalog numbers) changes with what the
/// sandbox permits, not with what a compile writes, so it must not force a
/// rebuild of an identical program.
pub(crate) fn behaviour() -> String {
    format!(
        "engine={} ginux={}",
        levitate_recipe::identity::ENGINE_BEHAVIOUR,
        levitate_recipe::identity::GINUX_BEHAVIOUR
    )
}

/// The key of the desktop apps tree for `variant`: the `gui/` tree, the base and apps-build
/// locks (the root the apps compile in is made from both), the apps-build root recipe, both
/// preseed recipes and `desktop-apps.rhai` with everything they import, the compiler image,
/// Recipe's declared behaviour versions, and the distro and its libc.
///
/// Not covered: the builder's own Rust that composes the root (`compose.rs`) and runs the
/// compile (`desktop_apps.rs`), which the other prebuilts leave out too.
pub fn desktop_apps_key(repo: &Path, variant: &distro_contract::Variant) -> Result<Key> {
    let mut fields = vec![
        ("family", "desktop-apps".into()),
        ("distro", variant.id.clone()),
        ("libc", variant.contract.profile.libc.as_str().into()),
        ("gui_tree", gui_digest(repo)?),
    ];
    fields.extend(build_root_fields(repo, variant)?);
    fields.extend([
        ("desktop_apps_recipe", recipe_digest(repo, &repo.join("distro/builder/os-recipes/desktop-apps.rhai"))?),
        ("compiler_image", crate::recipe::compiler_image_reference(repo)?),
        ("behaviour", behaviour()),
    ]);
    Ok(Key::new(&fields))
}

/// The digest of `recipe` and everything it imports ([`recipe_closure`]).
fn recipe_digest(repo: &Path, recipe: &Path) -> Result<String> {
    let mut found = BTreeMap::new();
    recipe_closure(repo, recipe, &mut found)?;
    Ok(closure_digest(&found))
}

/// The key fields of the apps-build root a tree is compiled in: the base and apps-build locks
/// (the root is made from both), both preseed recipes and the root recipe, each with what it
/// imports.
fn build_root_fields(repo: &Path, variant: &distro_contract::Variant) -> Result<Vec<(&'static str, String)>> {
    use crate::pipeline::source::{apps_build_preseed_recipe, apps_build_root_recipe, preseed_recipe};
    let closure = |recipe: &Path| -> Result<BTreeMap<String, String>> {
        let mut found = BTreeMap::new();
        recipe_closure(repo, recipe, &mut found)?;
        Ok(found)
    };
    let base_preseed = closure(&preseed_recipe(variant, distro_contract::profile::Flavour::Base))?;
    let build_preseed = closure(&apps_build_preseed_recipe(variant))?;
    let root = closure(&apps_build_root_recipe(variant))?;
    Ok(vec![
        ("base_lock", lock_digest(&base_preseed)),
        ("apps_build_lock", lock_digest(&build_preseed)),
        ("base_preseed_recipe", closure_digest(&base_preseed)),
        ("apps_build_preseed_recipe", closure_digest(&build_preseed)),
        ("apps_build_root_recipe", closure_digest(&root)),
    ])
}

/// The key of the display server tree for `variant` (`desktop.display_server_recipe_script`):
/// that recipe with everything it imports (it holds the source pins), the apps-build root it
/// compiles in ([`build_root_fields`]: both locks and the root's recipes), the compiler
/// image, Recipe's declared behaviour versions, and the distro and its libc. Fails for a
/// distro that names no display server recipe.
pub fn display_server_key(repo: &Path, variant: &distro_contract::Variant) -> Result<Key> {
    let recipe = crate::display_server::recipe(variant)
        .with_context(|| format!("{} compiles no display server (no desktop.display_server_recipe_script)", variant.id))?;
    let mut fields = vec![
        ("family", "display-server".into()),
        ("distro", variant.id.clone()),
        ("libc", variant.contract.profile.libc.as_str().into()),
        ("display_server_recipe", recipe_digest(repo, &recipe)?),
    ];
    fields.extend(build_root_fields(repo, variant)?);
    fields.extend([
        ("compiler_image", crate::recipe::compiler_image_reference(repo)?),
        ("behaviour", behaviour()),
    ]);
    Ok(Key::new(&fields))
}

/// The verified desktop apps tree for `variant`, from the local cache, else downloaded; `None`
/// on a miss (or any failure), without compiling. Never looks in exact mode or with
/// `--no-prebuilt`. The path is a directory holding the tree itself, ready to merge.
pub fn lookup_desktop_apps(repo: &Path, variant: &distro_contract::Variant) -> Result<Option<PathBuf>> {
    lookup_tree(repo, Family::DesktopApps, variant, || desktop_apps_key(repo, variant))
}

/// Puts a freshly compiled desktop apps `tree` into the local cache (origin `local`) under the
/// key the checkout has now and returns the cached tree. Exact mode stores nothing and returns
/// `tree` itself. `--no-prebuilt` still stores.
pub fn store_desktop_apps(repo: &Path, variant: &distro_contract::Variant, tree: &Path) -> Result<PathBuf> {
    store_tree(repo, Family::DesktopApps, variant, tree, || desktop_apps_key(repo, variant), &[PathBuf::from("gui")])
}

/// [`lookup_desktop_apps`] for the display server tree.
pub fn lookup_display_server(repo: &Path, variant: &distro_contract::Variant) -> Result<Option<PathBuf>> {
    lookup_tree(repo, Family::DisplayServer, variant, || display_server_key(repo, variant))
}

/// [`store_desktop_apps`] for the display server tree.
pub fn store_display_server(repo: &Path, variant: &distro_contract::Variant, tree: &Path) -> Result<PathBuf> {
    let recipe = crate::display_server::recipe(variant).context("no display server recipe")?;
    let dir = recipe.parent().map(Path::to_path_buf).unwrap_or_default();
    let relative = dir.strip_prefix(repo).map(Path::to_path_buf).unwrap_or(dir);
    store_tree(repo, Family::DisplayServer, variant, tree, || display_server_key(repo, variant), &[relative])
}

fn lookup_tree(
    repo: &Path,
    family: Family,
    variant: &distro_contract::Variant,
    key: impl FnOnce() -> Result<Key>,
) -> Result<Option<PathBuf>> {
    let (label, name) = (family.name(), variant.id.as_str());
    if crate::recipe::exact_identity() {
        crate::tprintln!("[prebuilt] {label} {name}: compiling (exact build: nothing prebuilt is used)");
        return Ok(None);
    }
    let key = key()?;
    let short = key.short().to_string();
    if NO_PREBUILT.load(Ordering::SeqCst) {
        crate::tprintln!("[prebuilt] {label} {name}: compiling {short} (--no-prebuilt)");
        return Ok(None);
    }
    if let Some(dir) = lookup(repo, family, name, &key, &Payload::Tree) {
        crate::tprintln!("[prebuilt] {label} {name}: local {short}");
        return Ok(Some(payload_dir(&dir, &Payload::Tree)));
    }
    match fetch(repo, family, name, &key, &Payload::Tree) {
        Ok(dir) => {
            crate::tprintln!("[prebuilt] {label} {name}: downloaded {short}");
            Ok(Some(payload_dir(&dir, &Payload::Tree)))
        }
        Err(error) => {
            crate::tprintln!("[prebuilt] {label} {name}: no prebuilt for {short}, compiling");
            let reason = format!("{error:#}");
            crate::teprintln!("  ({})", reason.lines().next().unwrap_or("no reason given"));
            Ok(None)
        }
    }
}

fn store_tree(
    repo: &Path,
    family: Family,
    variant: &distro_contract::Variant,
    tree: &Path,
    key: impl FnOnce() -> Result<Key>,
    source_dirs: &[PathBuf],
) -> Result<PathBuf> {
    let (label, name) = (family.name(), variant.id.as_str());
    if crate::recipe::exact_identity() {
        return Ok(tree.to_path_buf());
    }
    let key = key()?;
    let sources = source_commits(repo, source_dirs.iter().cloned());
    let dir = store(repo, family, name, &key, sources, Content::Tree(tree), "local", None)
        .with_context(|| format!("caching the compiled {label} of {name}"))?;
    crate::tprintln!("[prebuilt] {label} {name}: compiled {}", key.short());
    Ok(payload_dir(&dir, &Payload::Tree))
}
