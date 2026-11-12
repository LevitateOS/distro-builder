//! The configured store: the system tree after the package effects and the
//! system configuration, at `.artifacts/store/configured/<key>/tree` with
//! `configured.json` beside it. These two Recipe steps take most of a system
//! build (an hour or more on a desktop tree), and Recipe throws their saved
//! state away whenever its declared identity changes, which every Ginux
//! catalog change does. So the builder keeps their result under a key of what
//! they read: the package locks, the recipes (with their imports), the texts
//! and branding they are given, the unpack and compose formats, Recipe's and
//! Ginux's behaviour versions and the root they run in. Recipe's and Ginux's
//! sources and Ginux's catalog digests are not in it: what the system ships of
//! them is added after this tree, in the system part. Exact mode adds the
//! full declared identity and the bytes of the Recipe CLI and the launcher,
//! so a release reuses only what byte-identical tools made.
//!
//! An entry is made in a sibling temporary directory and renamed into place
//! only when complete. An `flock` on `.locks/<key>.lock` lets one builder
//! make a key while another waits for it and then reuses it.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::compose;

/// Bump whenever what an entry holds or how it is laid out changes in a way
/// no field shows.
const FORMAT: u32 = 1;
const RECORD: &str = "configured.json";
const TREE: &str = "tree";

pub(crate) struct Entry {
    key: String,
    dir: PathBuf,
    fields: Vec<(String, String)>,
    lock: Option<fs::File>,
}

impl Entry {
    pub(crate) fn new(repo: &Path, fields: Vec<(String, String)>) -> Result<Self> {
        let mut all = vec![
            ("store".to_string(), "configured".to_string()),
            ("format".to_string(), FORMAT.to_string()),
        ];
        all.extend(fields);
        let named: Vec<(&str, &str)> = all.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        let key = compose::key(&named);
        let parent = compose::store(repo).join("configured");
        fs::create_dir_all(&parent).with_context(|| format!("creating {}", parent.display()))?;
        Ok(Self { dir: parent.join(&key), key, fields: all, lock: None })
    }

    pub(crate) fn short(&self) -> &str {
        &self.key[..12]
    }

    fn tree(&self) -> PathBuf {
        self.dir.join(TREE)
    }

    fn is_complete(&self) -> bool {
        self.dir.join(RECORD).is_file() && self.tree().is_dir()
    }

    fn parent(&self) -> &Path {
        self.dir.parent().expect("a store entry has a parent")
    }

    /// The stored tree when the key is complete. Otherwise takes the key's
    /// lock (waiting while another builder holds it, then looking again),
    /// clears what a dead builder left, and returns `None`: the caller makes
    /// the tree and gives it to [`Entry::store`].
    pub(crate) fn reuse_or_lock(&mut self) -> Result<Option<PathBuf>> {
        if self.is_complete() {
            return Ok(Some(self.tree()));
        }
        let locks = self.parent().join(".locks");
        fs::create_dir_all(&locks).with_context(|| format!("creating {}", locks.display()))?;
        let path = locks.join(format!("{}.lock", self.key));
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        if !compose::flock(&file, libc::LOCK_EX | libc::LOCK_NB)? {
            crate::tprintln!("  waiting for another build making the configured tree {}", self.short());
            compose::flock(&file, libc::LOCK_EX)?;
        }
        self.lock = Some(file);
        if self.is_complete() {
            return Ok(Some(self.tree()));
        }
        compose::remove_unfinished(self.parent(), &self.key)?;
        Ok(None)
    }

    /// Copies `tree` (a Recipe output tree with its target metadata) into
    /// the store under the key and returns the stored tree.
    pub(crate) fn store(&self, tree: &Path) -> Result<PathBuf> {
        let step = crate::timeline::Step::begin("step", &format!("store the configured tree {}", self.short()));
        let temp = self.parent().join(format!(".tmp-{}-{}", self.key, std::process::id()));
        let result = (|| -> Result<()> {
            levitate_recipe::target::merge_tree(tree, &temp.join(TREE))
                .with_context(|| format!("copying {}", tree.display()))?;
            let record = serde_json::json!({
                "format": FORMAT,
                "key": self.key,
                "fields": self.fields.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(),
                "configured_at": distro_contract::runs::now_rfc3339(),
            });
            fs::write(temp.join(RECORD), serde_json::to_vec_pretty(&record)?)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&temp);
            return Err(error.context(format!("storing the configured tree {}", self.short())));
        }
        fs::rename(&temp, &self.dir).with_context(|| format!("moving the configured tree into {}", self.dir.display()))?;
        fs::File::open(self.parent())?.sync_all()?;
        step.finish();
        Ok(self.tree())
    }
}
