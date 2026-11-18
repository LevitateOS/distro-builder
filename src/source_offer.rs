//! The source offer of a release: `SOURCES.txt`, written next to the ISO by
//! the release build, and the `LICENSES.txt` the system recipe puts into the
//! image (`/usr/share/doc/levitate/LICENSES.txt`). Both carry the same written
//! offer; the list names the exact upstream source of every package and
//! component of the release (docs/release-engineering.md).
//!
//! The list covers, sorted and without any time or host in it:
//!
//! - the kernel: the pinned kernel.org tarball (`kernel/metadata.rhai`);
//! - the components built from a pinned source by a recipe (`SOURCES` of the
//!   default set's recipes and their dependencies, of the initramfs's busybox
//!   and of the display server), except
//!   LevitateOS's own prebuilt programs, whose source is this repository;
//! - one entry per source package of the base lock and the desktop lock: for
//!   Fedora the source RPM the locked binary RPM's own header names, for
//!   Alpine the aports commit the package's `.PKGINFO` names.
//!
//! The locks pin binary packages and carry neither source hashes nor licences,
//! so those come from the packages' own headers in the download store; a fact
//! that is in neither place is written `unknown`, never guessed.

use anyhow::{ensure, Context, Result};
use distro_contract::profile::{Libc, PackageBackend};
use distro_contract::Variant;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::pipeline::source::preseed_recipe;
use crate::recipe::rootfs_source::{lock_path, lock_rows, store_dir, LockRow};
use distro_contract::profile::Flavour;

/// The list's file name, next to the ISO and as an asset of the release.
pub(crate) const FILE: &str = "SOURCES.txt";

/// The written offer, word for word.
pub(crate) const OFFER: &str = "For at least three years after the release, on request to the project's issue \
tracker (https://github.com/LevitateOS/LevitateOS/issues) we provide the complete corresponding source code of \
every GPL- and LGPL-licensed program in the release on a medium customarily used for software interchange, for \
no more than the cost of physically performing the distribution. The exact upstream source packages and their \
hashes are listed in SOURCES.txt.";

const UNKNOWN: &str = "unknown";
const KOJI: &str = "https://kojipkgs.fedoraproject.org/packages";
const APORTS: &str = "https://gitlab.alpinelinux.org/alpine/aports/-/tree";
const KERNEL_LICENSE: &str = "GPL-2.0-only WITH Linux-syscall-note";
/// A source hosted under this owner is the project's own: its source is the
/// repository, not an upstream package.
const OWN: &str = "https://github.com/LevitateOS/";

/// One line of the list.
struct Entry {
    name: String,
    version: String,
    licenses: BTreeSet<String>,
    url: String,
    sha256: String,
    /// The binary packages (or the kernel's own output) built from it.
    packages: BTreeSet<String>,
}

impl Entry {
    fn line(&self) -> String {
        let licenses = if self.licenses.is_empty() {
            UNKNOWN.to_string()
        } else {
            self.licenses.iter().cloned().collect::<Vec<_>>().join("; ")
        };
        let packages = self.packages.iter().cloned().collect::<Vec<_>>().join(",");
        format!("{}\t{}\t{licenses}\t{}\t{}\t{packages}\n", self.name, self.version, self.url, self.sha256)
    }
}

/// The packages' own world, as the lock names it: "Fedora 45" or "Alpine 3.24".
fn package_world(variant: &Variant) -> Result<String> {
    let base = lock_path(&preseed_recipe(variant, Flavour::Base))?;
    let text = fs::read_to_string(&base).with_context(|| format!("reading {}", base.display()))?;
    let release = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("fn release() {")?.strip_suffix('}'))
        .map(|value| value.trim().trim_matches('"').trim_start_matches('v').to_string())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{} has no release()", base.display()))?;
    Ok(match variant.contract.profile.packages {
        PackageBackend::Rpm => format!("Fedora {release} RPM"),
        PackageBackend::Apk => format!("Alpine {release} APK"),
    })
}

/// The web page of the licences of `variant`'s system.
fn licenses_url(variant: &Variant) -> &'static str {
    match variant.contract.identity.os_id.as_str() {
        "acornos" => "https://levitateos.org/acorn/licenses",
        _ => "https://levitateos.org/licenses",
    }
}

/// The release asset `SOURCES.txt` is published as.
fn sources_url(variant: &Variant) -> String {
    let identity = &variant.contract.identity;
    format!(
        "https://github.com/LevitateOS/releases/releases/download/{}-{}/{FILE}",
        identity.os_id, identity.release_serial
    )
}

/// The text of `/usr/share/doc/levitate/LICENSES.txt`: what the image says
/// about the licences of what it contains. The system recipe writes it.
pub(crate) fn licenses_text(variant: &Variant) -> Result<String> {
    let identity = &variant.contract.identity;
    let name = &identity.os_name;
    let world = package_world(variant)?;
    let texts = match variant.contract.profile.packages {
        PackageBackend::Rpm => {
            "The licence texts of the packages are in /usr/share/licenses, one folder for each package."
        }
        PackageBackend::Apk => {
            "Alpine's packages carry the name of their licence, not its text. The texts this system has are in \
             /usr/share/licenses, one folder for each licence identifier the packages declare (for example \
             GPL-2.0-or-later/LICENSE); /usr/share/licenses/MISSING.txt lists the identifiers whose text it \
             does not have, each with the packages that declare it. A package that ships its own licence \
             files installs them with the package."
        }
    };
    Ok(format!(
        "{name} {version}: licences and source code\n\
         \n\
         {name} is an independent operating system. It is built from {world} packages, from its own \
         programs and from a Linux kernel it builds from pinned kernel.org source.\n\
         \n\
         The system contains third-party software, each program under its own licence. {texts}\n\
         The licences of the project and of the software in the release: {licenses}\n\
         The project's own programs are in https://github.com/LevitateOS/LevitateOS.\n\
         \n\
         The exact source package of everything in the release, with its licence and hash, is listed in \
         SOURCES.txt, an asset of this release:\n{sources}\n\
         \n\
         Written offer\n\
         \n\
         {OFFER}\n",
        version = identity.os_version,
        licenses = licenses_url(variant),
        sources = sources_url(variant),
    ))
}

/// The licence texts the system recipe installs under `/usr/share/licenses`,
/// as the `license_texts` input: records `@@@ <path>` followed by the file's
/// text, each record ending in a newline and joined by one more.
///
/// AcornOS: the identifiers the locked packages declare in their `.PKGINFO`
/// (and the kernel's), each with the text the project holds for it in
/// `LICENSES/<identifier>.txt` (a `-only`, `-or-later` or `+` identifier takes
/// the text of its family: `GPL-2.0.txt` serves `GPL-2.0-or-later`) as
/// `<identifier>/LICENSE`; every identifier without a text is listed in
/// `MISSING.txt` with the packages that declare it, never given an invented
/// text. A text added to `LICENSES/` is picked up by its name alone.
/// LevitateOS: empty, Fedora's packages carry their own texts.
pub(crate) fn license_texts(variant: &Variant) -> Result<String> {
    if variant.contract.profile.packages == PackageBackend::Rpm {
        return Ok(String::new());
    }
    // The desktop lock's APKs are in the store only once its preseed ran;
    // every package a lock names has to be read, or the list is incomplete.
    let desktop = preseed_recipe(variant, Flavour::Desktop);
    crate::recipe::rootfs_source::preseed_rootfs_source(&variant.repo_root, &variant.id, &desktop, Flavour::Desktop, false)
        .context("acquiring the desktop lock to read its packages' licences")?;
    let mut declared: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut unread = Vec::new();
    for row in lock_packages(variant)? {
        let info = stored(variant, &row).map(|path| apk_info(&path)).transpose();
        match info {
            Ok(Some(info)) => {
                let ids = info.get("license").map(|text| license_ids(text)).unwrap_or_default();
                if ids.is_empty() {
                    declared.entry(UNKNOWN.into()).or_default().insert(row.name.clone());
                }
                for id in ids {
                    declared.entry(id).or_default().insert(row.name.clone());
                }
            }
            Ok(None) => unread.push(format!("{} (not in the download store)", row.id)),
            Err(error) => unread.push(format!("{}: {error:#}", row.id)),
        }
    }
    ensure!(
        unread.is_empty(),
        "the licences of these locked packages could not be read from their .PKGINFO, so the licence list \
         would be incomplete: {}. Run the desktop and base preseeds (`cargo xtask build` does) and rebuild.",
        unread.join("; ")
    );
    for id in license_ids(KERNEL_LICENSE) {
        declared.entry(id).or_default().insert("linux".into());
    }

    let dir = variant.repo_root.join("LICENSES");
    let mut records = Vec::new();
    let mut missing = String::new();
    for (id, packages) in &declared {
        match license_file(&dir, id) {
            Some(file) => {
                let text = fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
                records.push((format!("{id}/LICENSE"), text));
            }
            None => {
                let packages = packages.iter().cloned().collect::<Vec<_>>().join(",");
                missing.push_str(&format!("{id}\t{packages}\n"));
            }
        }
    }
    records.push((
        "MISSING.txt".into(),
        format!(
            "# Licence identifiers declared by the packages of this system (the Alpine package locks and the \
             kernel)\n# whose text this image does not carry. One per line: the identifier, a tab, the packages \
             that declare it.\n# A package that ships its own licence files installs them with the package.\n{missing}"
        ),
    ));
    let mut text = String::new();
    for (index, (path, content)) in records.iter().enumerate() {
        ensure!(
            content.ends_with('\n') && !content.lines().any(|line| line.starts_with("@@@ ")),
            "the licence text for {path} would break the record format of the license_texts input"
        );
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&format!("@@@ {path}\n{content}"));
    }
    Ok(text)
}

/// The licence identifiers of a package's `license` field: an SPDX expression
/// or, as Alpine writes some, a plain list. The operators and parentheses go;
/// `Public Domain` is one identifier.
fn license_ids(expression: &str) -> BTreeSet<String> {
    let flat = expression.replace(['(', ')', ','], " ").replace("Public Domain", "Public-Domain");
    flat.split_whitespace()
        .filter(|token| !matches!(*token, "AND" | "OR" | "WITH"))
        .map(str::to_string)
        .collect()
}

/// The text the project holds for licence `id`: `LICENSES/<id>.txt`, else that
/// of the id's family without `+`, `-only` or `-or-later`.
fn license_file(dir: &Path, id: &str) -> Option<PathBuf> {
    let safe = |name: &str| {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b'_'))
    };
    let family = id.trim_end_matches('+');
    let family = family.strip_suffix("-or-later").or_else(|| family.strip_suffix("-only")).unwrap_or(family);
    [id, family]
        .into_iter()
        .filter(|name| safe(name))
        .map(|name| dir.join(format!("{name}.txt")))
        .find(|path| path.is_file())
}

/// Writes [`FILE`] into the release's run directory `dir`.
pub(crate) fn write(variant: &Variant, dir: &Path) -> Result<()> {
    let identity = &variant.contract.identity;
    let world = package_world(variant)?;
    let mut groups: Vec<(&str, &str, Vec<Entry>)> = Vec::new();

    let (packages_title, packages_note, packages) = match variant.contract.profile.packages {
        PackageBackend::Rpm => (
            "fedora-source-packages",
            "One entry for each source RPM of the base lock and the desktop lock \
             (distro/builder/os-recipes/sources). The source RPM's name, version and release are read from the \
             SOURCERPM tag of the locked binary RPM's own header, and its licence from the LICENSE tag \
             (the locks pin binary RPMs and carry neither). The URL is where Fedora's Koji keeps that source \
             RPM, derived from those three. The SHA-256 of a source RPM is unknown to the locks, so it is \
             written unknown; the SHA-256 of every binary RPM is in the locks.",
            fedora_entries(variant)?,
        ),
        PackageBackend::Apk => (
            "alpine-source-packages",
            "One entry for each origin (source package) of the base lock and the desktop lock \
             (distro/builder/os-recipes/sources). The origin, version, licence and the aports commit are read from \
             the .PKGINFO of the locked APK (the locks pin APKs and carry none of them). The URL is the origin's \
             folder in Alpine's aports repository at that commit: its APKBUILD names the upstream source \
             archives and their hashes (SHA-512), which this list does not repeat, so the SHA-256 column is \
             unknown; the SHA-256 of every APK is in the locks.",
            alpine_entries(variant)?,
        ),
    };
    groups.push((
        "kernel",
        "The kernel, built by this project from the pinned kernel.org tarball \
         (distro/builder/os-recipes/kernel/metadata.rhai).",
        vec![kernel_entry(variant)?],
    ));
    groups.push((
        "components-built-from-pinned-sources",
        "Components built or taken from a pinned source by a recipe (SOURCES of the recipe). A prebuilt \
         program pinned by its SHA-256 (busybox, GPL-2.0-only) is listed by the pinned file's URL; its \
         source is the upstream project's release of the same version. LevitateOS's own programs are not listed: their \
         source is https://github.com/LevitateOS/LevitateOS and the repositories it names.",
        recipe_entries(variant)?,
    ));
    groups.push((packages_title, packages_note, packages));

    let mut text = format!(
        "SOURCES.txt: the source of {} {} (release serial {})\n\n{OFFER}\n\n",
        identity.os_name, identity.os_version, identity.release_serial
    );
    text.push_str(&format!(
        "{} is built from {world} packages, its own programs and a kernel built from pinned source. \
         This list is written by the release build from the pins the build used, sorted, and carries \
         `unknown` wherever a fact is in no pin or package header.\n\
         Columns, separated by a tab: name, version, licence (as the package states it; several \
         binary packages of one source may state several), source URL, SHA-256 of that source, \
         the binary packages built from it.\n",
        identity.os_name
    ));
    for (title, note, mut entries) in groups {
        entries.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        text.push_str(&format!("\n[{title}] {} entries\n# {note}\n", entries.len()));
        for entry in &entries {
            text.push_str(&entry.line());
        }
    }
    let path = dir.join(FILE);
    distro_contract::runs::write_atomic(&path, text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    crate::tprintln!("[release] wrote {FILE} ({} bytes)", text.len());
    Ok(())
}

/// The base lock's and the desktop lock's rows (the packages the system and
/// the default set are made of), each package once.
fn lock_packages(variant: &Variant) -> Result<Vec<LockRow>> {
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for flavour in [Flavour::Base, Flavour::Desktop] {
        for row in lock_rows(&preseed_recipe(variant, flavour))? {
            if seen.insert(row.id.clone()) {
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

/// A locked package's file in the download store, when the store has it.
fn stored(variant: &Variant, row: &LockRow) -> Option<PathBuf> {
    Some(store_dir(&variant.repo_root).join(&row.sha256)).filter(|path| path.is_file())
}

fn fedora_entries(variant: &Variant) -> Result<Vec<Entry>> {
    let mut sources: BTreeMap<String, Entry> = BTreeMap::new();
    for row in lock_packages(variant)? {
        let header = stored(variant, &row).and_then(|path| match rpm_header(&path) {
            Ok(header) => Some(header),
            Err(error) => {
                crate::tprintln!("[release] warning: {}: {error:#}", row.id);
                None
            }
        });
        let (srpm, license) = match header {
            Some((srpm, license)) => (Some(srpm), license),
            None => (koji_srpm(&row.url), None),
        };
        let parsed = srpm.as_deref().and_then(split_srpm);
        let entry = match (&srpm, &parsed) {
            (Some(srpm), Some((name, version, release))) => sources.entry(srpm.clone()).or_insert_with(|| Entry {
                name: name.clone(),
                version: format!("{version}-{release}"),
                licenses: BTreeSet::new(),
                url: format!("{KOJI}/{name}/{version}/{release}/src/{srpm}"),
                sha256: UNKNOWN.into(),
                packages: BTreeSet::new(),
            }),
            _ => {
                // No store file and no koji URL: the binary package is all the lock says.
                crate::tprintln!("[release] warning: {} has no source RPM name in the store or its URL", row.id);
                sources.entry(format!("binary:{}", row.id)).or_insert_with(|| Entry {
                    name: row.name.clone(),
                    version: UNKNOWN.into(),
                    licenses: BTreeSet::new(),
                    url: UNKNOWN.into(),
                    sha256: UNKNOWN.into(),
                    packages: BTreeSet::new(),
                })
            }
        };
        entry.licenses.extend(license);
        entry.packages.insert(row.name);
    }
    Ok(sources.into_values().collect())
}

/// `name-version-release.src.rpm` of a koji URL
/// (`packages/<source name>/<version>/<release>/...`), which names the source
/// RPM exactly.
fn koji_srpm(url: &str) -> Option<String> {
    let rest = url.strip_prefix(KOJI)?.strip_prefix('/')?;
    let mut parts = rest.split('/');
    let (name, version, release) = (parts.next()?, parts.next()?, parts.next()?);
    Some(format!("{name}-{version}-{release}.src.rpm"))
}

/// `(name, version, release)` of `name-version-release.src.rpm`.
fn split_srpm(srpm: &str) -> Option<(String, String, String)> {
    let stem = srpm.strip_suffix(".src.rpm")?;
    let mut parts = stem.rsplitn(3, '-');
    let (release, version, name) = (parts.next()?, parts.next()?, parts.next()?);
    Some((name.into(), version.into(), release.into()))
}

/// The source RPM and the licence an RPM's own header states. Reads the lead,
/// the signature header and the main header only.
fn rpm_header(path: &Path) -> Result<(String, Option<String>)> {
    const SOURCERPM: u32 = 1044;
    const LICENSE: u32 = 1014;
    let mut file = BufReader::new(fs::File::open(path).with_context(|| format!("opening {}", path.display()))?);
    let mut lead = [0u8; 96];
    file.read_exact(&mut lead).context("reading the RPM lead")?;
    ensure!(lead[..4] == [0xed, 0xab, 0xee, 0xdb], "{} is not an RPM", path.display());
    read_section(&mut file, true).context("reading the signature header")?;
    let (index, data) = read_section(&mut file, false).context("reading the main header")?;
    let string = |wanted: u32| -> Option<String> {
        let (_, _, offset, _) = index.iter().find(|entry| entry.0 == wanted)?;
        let rest = data.get(*offset as usize..)?;
        let end = rest.iter().position(|b| *b == 0)?;
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    };
    let srpm = string(SOURCERPM).filter(|s| s.ends_with(".src.rpm")).context("the header has no SOURCERPM")?;
    Ok((srpm, string(LICENSE)))
}

/// One RPM header section: its index entries (tag, type, offset, count) and
/// data; `pad` skips the padding to 8 bytes the signature header has.
fn read_section(file: &mut impl Read, pad: bool) -> Result<(Vec<(u32, u32, u32, u32)>, Vec<u8>)> {
    let mut intro = [0u8; 16];
    file.read_exact(&mut intro)?;
    ensure!(intro[..3] == [0x8e, 0xad, 0xe8], "not an RPM header");
    let count = u32::from_be_bytes(intro[8..12].try_into()?) as usize;
    let size = u32::from_be_bytes(intro[12..16].try_into()?) as usize;
    ensure!(count <= 1 << 16 && size <= 64 << 20, "implausible RPM header");
    let mut index = vec![0u8; count * 16];
    file.read_exact(&mut index)?;
    let mut data = vec![0u8; size];
    file.read_exact(&mut data)?;
    if pad {
        let mut skip = vec![0u8; (8 - (count * 16 + size) % 8) % 8];
        file.read_exact(&mut skip)?;
    }
    let word = |bytes: &[u8]| u32::from_be_bytes(bytes.try_into().expect("four bytes"));
    let entries = index
        .chunks_exact(16)
        .map(|e| (word(&e[0..4]), word(&e[4..8]), word(&e[8..12]), word(&e[12..16])))
        .collect();
    Ok((entries, data))
}

fn alpine_entries(variant: &Variant) -> Result<Vec<Entry>> {
    let mut sources: BTreeMap<(String, String, String), Entry> = BTreeMap::new();
    for row in lock_packages(variant)? {
        let info = stored(variant, &row).and_then(|path| match apk_info(&path) {
            Ok(info) => Some(info),
            Err(error) => {
                crate::tprintln!("[release] warning: {}: {error:#}", row.id);
                None
            }
        });
        let field = |key: &str| info.as_ref().and_then(|info| info.get(key)).cloned();
        let origin = field("origin").or_else(|| field("pkgname")).unwrap_or_else(|| row.name.clone());
        let version = field("pkgver").unwrap_or_else(|| UNKNOWN.into());
        let commit = field("commit").unwrap_or_else(|| UNKNOWN.into());
        // The repository folder (main, community) is in the lock's URL:
        // .../<release>/<repository>/<arch>/<file>.
        let repository = row.url.rsplit('/').nth(2).unwrap_or(UNKNOWN).to_string();
        let url = if commit == UNKNOWN || repository == UNKNOWN {
            UNKNOWN.to_string()
        } else {
            format!("{APORTS}/{commit}/{repository}/{origin}")
        };
        let entry = sources.entry((origin.clone(), version.clone(), commit)).or_insert_with(|| Entry {
            name: origin,
            version,
            licenses: BTreeSet::new(),
            url,
            sha256: UNKNOWN.into(),
            packages: BTreeSet::new(),
        });
        entry.licenses.extend(field("license"));
        entry.packages.insert(row.name);
    }
    Ok(sources.into_values().collect())
}

/// The `key = value` lines of an APK's `.PKGINFO` (the first of a repeated
/// key). Read with the host's `tar`, which the build already needs.
fn apk_info(path: &Path) -> Result<BTreeMap<String, String>> {
    let output = Command::new("tar")
        .arg("-xzOf")
        .arg(path)
        .arg("--occurrence=1")
        .arg(".PKGINFO")
        .stderr(Stdio::null())
        .output()
        .context("running tar")?;
    ensure!(output.status.success(), "{} has no readable .PKGINFO", path.display());
    let mut info = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((key, value)) = line.split_once(" = ") {
            info.entry(key.trim().to_string()).or_insert_with(|| value.trim().to_string());
        }
    }
    Ok(info)
}

fn kernel_entry(variant: &Variant) -> Result<Entry> {
    let pin = crate::pipeline::kernel::pin(variant)?;
    Ok(Entry {
        name: "linux".into(),
        version: pin.version.clone(),
        licenses: BTreeSet::from([KERNEL_LICENSE.to_string()]),
        url: crate::pipeline::kernel::source_tarball_url(variant)?,
        sha256: pin.sha256,
        packages: BTreeSet::from([format!("linux-{}", pin.release)]),
    })
}

/// The pinned sources of the recipes that make up the release.
fn recipe_entries(variant: &Variant) -> Result<Vec<Entry>> {
    let repo = &variant.repo_root;
    let dir = repo.join("recipes");
    let mut files: Vec<PathBuf> = crate::default_set::closure(repo, &variant.contract.desktop.recipes)?
        .iter()
        .map(|name| dir.join(format!("{name}.rhai")))
        .collect();
    files.push(repo.join(crate::release::BUSYBOX_RECIPE));
    files.extend(crate::display_server::recipe(variant));
    let libc = match variant.contract.profile.libc {
        Libc::Glibc => "glibc",
        Libc::Musl => "musl",
    };
    let mut entries = Vec::new();
    for file in files {
        let folder = file.parent().context("a recipe has a folder")?;
        let declaration = levitate_recipe::needs::declaration(&file, Some(folder))
            .with_context(|| format!("reading {}", file.display()))?;
        let declared = &declaration.declared;
        let license = declared.about.as_ref().and_then(|about| about["license"].as_str()).map(str::to_string);
        let sources: Vec<_> = declared
            .sources
            .iter()
            .filter(|(name, _)| name.as_str() != levitate_recipe::declarations::PACKAGES_SOURCE)
            .filter(|(_, source)| source.libc.is_none_or(|l| l.as_str() == libc))
            .collect();
        for (key, source) in &sources {
            use levitate_recipe::declarations::Origin;
            let (url, sha256) = match &source.origin {
                Origin::Url { url, sha256 } => (url.clone(), sha256.clone()),
                Origin::Git { git, commit, .. } => (format!("git+{git}@{commit}"), UNKNOWN.to_string()),
            };
            if url.trim_start_matches("git+").starts_with(OWN) {
                continue;
            }
            let recipe = declaration.name().to_string();
            entries.push(Entry {
                name: if sources.len() == 1 { recipe.clone() } else { format!("{recipe}/{key}") },
                version: declaration.version().unwrap_or(UNKNOWN).to_string(),
                licenses: license.iter().cloned().collect(),
                url,
                sha256,
                packages: BTreeSet::from([format!("recipe {recipe}")]),
            });
        }
    }
    Ok(entries)
}
