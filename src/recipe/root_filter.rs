//! Making an OS root tar acceptable to the image producer.
//!
//! `levitate_recipe::image::Validator` refuses three kinds of member that an
//! assembled OS root can legitimately hold: `etc/hosts` (the lifecycle always
//! provides its own hosts file), device nodes, FIFOs and other special entries,
//! and any name with a path component starting `.wh.` (Ginux's expander reads it
//! as an OCI whiteout). [`filter`] writes the tar again without exactly those
//! members, in one streaming pass that never holds a file in memory, and says
//! on the build log how many of each it left out. Every other member keeps its
//! header (mode, owner, mtime, link target) and its place in the order.
//!
//! The validator itself is not weakened: the filtered tar is still checked by
//! `write_layout`.

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use levitate_recipe::image::{member_path, HOSTS};

/// Why a member is left out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Refused {
    /// A path component starts `.wh.`.
    Whiteout,
    /// The member is `etc/hosts`.
    Hosts,
    /// Not a file, directory, symlink or hard link (device, FIFO, ...).
    Special,
}

/// What the root tar loses, as counted.
#[derive(Default)]
struct Dropped {
    whiteouts: usize,
    hosts: usize,
    special: usize,
}

impl Dropped {
    fn count(&mut self, why: Refused) {
        match why {
            Refused::Whiteout => self.whiteouts += 1,
            Refused::Hosts => self.hosts += 1,
            Refused::Special => self.special += 1,
        }
    }

    fn total(&self) -> usize {
        self.whiteouts + self.hosts + self.special
    }
}

fn refusal(path: &Path, kind: tar::EntryType) -> Option<Refused> {
    if path
        .components()
        .any(|part| part.as_os_str().as_encoded_bytes().starts_with(b".wh."))
    {
        Some(Refused::Whiteout)
    } else if path == Path::new(HOSTS) {
        Some(Refused::Hosts)
    } else if !(kind.is_file() || kind.is_dir() || kind.is_symlink() || kind.is_hard_link()) {
        Some(Refused::Special)
    } else {
        None
    }
}

/// The root tar `layer` without the members the image rules refuse. `None`
/// when it has none (use `layer` as it is); else the filtered tar, written to
/// `out`. A hard link whose target is a left-out member stops the build with
/// the link's name: the root would break, and nothing here converts it.
pub(super) fn filter(layer: &Path, out: &Path) -> Result<Option<PathBuf>> {
    let scanned = scan(layer)?;
    if scanned.total() == 0 {
        return Ok(None);
    }
    rewrite(layer, out)?;
    crate::teprintln!(
        "root '{}': left out of the image layer: {} etc/hosts, {} special entries (device nodes, FIFOs), {} names with a `.wh.` component",
        layer.display(),
        scanned.hosts,
        scanned.special,
        scanned.whiteouts
    );
    Ok(Some(out.to_owned()))
}

/// Counts what would be left out, and refuses a kept hard link to a left-out member.
fn scan(layer: &Path) -> Result<Dropped> {
    let mut file = File::open(layer).with_context(|| format!("opening '{}'", layer.display()))?;
    let mut archive = tar::Archive::new(&mut file);
    let mut dropped = Dropped::default();
    let mut gone: HashSet<PathBuf> = HashSet::new();
    for entry in archive
        .entries_with_seek()
        .with_context(|| format!("reading the root '{}'", layer.display()))?
    {
        let entry = entry.with_context(|| format!("reading the root '{}'", layer.display()))?;
        let path = member_path(&entry.path()?)?;
        if path.as_os_str().is_empty() {
            continue;
        }
        let kind = entry.header().entry_type();
        if let Some(why) = refusal(&path, kind) {
            dropped.count(why);
            gone.insert(path);
            continue;
        }
        if kind.is_hard_link() {
            if let Some(target) = entry.link_name()? {
                let target = member_path(&target)?;
                if gone.contains(&target) {
                    bail!(
                        "the root '{}' has the hard link '{}' to '{}', which the image leaves out; \
                         the link would break, so the root is not converted: make the link a copy \
                         or point it at a member the image keeps",
                        layer.display(),
                        path.display(),
                        target.display()
                    );
                }
            }
        }
    }
    Ok(dropped)
}

/// Streams `layer` into `out` without the refused members.
fn rewrite(layer: &Path, out: &Path) -> Result<()> {
    let input = File::open(layer).with_context(|| format!("opening '{}'", layer.display()))?;
    let mut archive = tar::Archive::new(BufReader::with_capacity(1 << 20, input));
    let output = File::create(out).with_context(|| format!("creating '{}'", out.display()))?;
    let mut builder = tar::Builder::new(BufWriter::with_capacity(1 << 20, output));
    for entry in archive
        .entries()
        .with_context(|| format!("reading the root '{}'", layer.display()))?
    {
        let mut entry = entry.with_context(|| format!("reading the root '{}'", layer.display()))?;
        let raw = entry.path()?.into_owned();
        let path = member_path(&raw)?;
        let kind = entry.header().entry_type();
        if !path.as_os_str().is_empty() {
            if refusal(&path, kind).is_some() {
                // Its data is skipped when the next entry is read.
                continue;
            }
        }
        let mut header = entry.header().clone();
        if entry.size() != header.size()? {
            bail!(
                "the root '{}' member '{}' carries its size in a PAX record, which is not supported",
                layer.display(),
                raw.display()
            );
        }
        // The path and link name are written again below; clear the old bytes
        // so the header holds nothing but them.
        header.as_old_mut().name = [0; 100];
        header.as_old_mut().linkname = [0; 100];
        if let Some(ustar) = header.as_ustar_mut() {
            ustar.prefix = [0; 155];
        }
        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()?
                .with_context(|| format!("the link '{}' has no target", raw.display()))?
                .into_owned();
            builder.append_link(&mut header, &raw, &target)
        } else {
            builder.append_data(&mut header, &raw, &mut entry)
        }
        .with_context(|| format!("writing '{}' to '{}'", raw.display(), out.display()))?;
    }
    let mut writer = builder
        .into_inner()
        .context("finishing the filtered root")?;
    writer
        .flush()
        .with_context(|| format!("writing '{}'", out.display()))?;
    Ok(())
}
