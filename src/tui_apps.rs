//! The terminal apps under `tui/apps/`, as their own `package.json` files describe them.
//!
//! Each app is a member of the root JavaScript workspace and declares itself once:
//! its `bin` entry gives its one name (also the directory name and the package name
//! `@levitate/<name>`) and its entry file, and an optional `levitate` block lists the
//! binaries it runs from PATH, the flags it takes the distro's facts by and how
//! `cargo xtask inspect` renders it. The builder (the install screen) and xtask (the
//! TUI harness) read apps from here instead of spelling out paths or flags.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The root workspace, read once: its TUI packages and its apps.
pub struct Workspace {
    /// Every TUI package (apps and the kit), absolute, in workspace order.
    pub packages: Vec<PathBuf>,
    /// Every app, in workspace order.
    pub apps: Vec<TuiApp>,
}

#[derive(Debug, Clone)]
pub struct TuiApp {
    /// The app's one name: `bin` key, directory name, `@levitate/<name>`.
    pub name: String,
    /// One line on what the app is for: `description` in its package.json.
    pub description: String,
    /// Absolute app directory.
    pub dir: PathBuf,
    /// Entry file, relative to `dir`.
    pub entry: PathBuf,
    /// Binaries the app runs from PATH (workspace tools, by binary name).
    pub tools: Vec<String>,
    /// The flags the app is told the distro's facts by.
    pub distro_args: DistroArgs,
    /// How `cargo xtask inspect` renders the app headlessly.
    pub inspect: Inspect,
}

/// The flags an app takes the distro's facts by: its launcher (the install screen,
/// or xtask) passes them, so the app reads nothing from the running system.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DistroArgs {
    /// The flag that names the distro (its `os_name`).
    #[serde(default)]
    pub os_name: Option<String>,
    /// The flag that names the install shell.
    #[serde(default)]
    pub shell: Option<String>,
}

impl TuiApp {
    /// The arguments that tell this app it runs on `os_name` with `shell`, as its
    /// manifest declares them (none for an app that declares no flags).
    pub fn distro_argv(&self, os_name: &str, shell: &str) -> Vec<String> {
        let flags = &self.distro_args;
        [(&flags.os_name, os_name), (&flags.shell, shell)]
            .into_iter()
            .filter_map(|(flag, value)| Some([flag.clone()?, value.to_string()]))
            .flatten()
            .collect()
    }

    /// The story the manifest names `story`, or numbers it (from 1, as
    /// `cargo xtask tui list <app>` does).
    pub fn story(&self, story: &str) -> Result<&Story> {
        let stories = &self.inspect.stories;
        let by_number = story
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| stories.get(i));
        by_number
            .or_else(|| stories.iter().find(|s| s.name == story))
            .with_context(|| {
                format!(
                    "{} has no story '{story}'; `cargo xtask tui list {}` lists them",
                    self.name, self.name
                )
            })
    }

    /// The flag that opens a page (`inspect.pages.arg`).
    pub fn page_arg(&self) -> Result<&str> {
        self.inspect
            .pages
            .as_ref()
            .map(|pages| pages.arg.as_str())
            .with_context(|| format!("{} declares no levitate.inspect.pages", self.name))
    }
}

#[derive(Deserialize)]
struct RootManifest {
    workspaces: Vec<String>,
}

#[derive(Deserialize)]
struct AppManifest {
    name: String,
    /// One line on what the app is for (its `--help` summary too).
    description: String,
    #[serde(default)]
    bin: BTreeMap<String, String>,
    #[serde(default)]
    levitate: Levitate,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Levitate {
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    distro_args: DistroArgs,
    #[serde(default)]
    inspect: Inspect,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    /// How to render every page: run the app with `list` to print one page per line,
    /// then render each with `arg <page>`.
    #[serde(default)]
    pub pages: Option<Pages>,
    /// Patterns (regular expressions) for screen text that depends on data the app shows
    /// rather than on the app, masked before snapshots are compared.
    #[serde(default)]
    pub mask: Vec<String>,
    /// App-relative directories put first on PATH in headless runs. Each must hold a
    /// stand-in for every binary in `tools`, so a headless run never reaches a real one.
    #[serde(default)]
    pub path: Vec<String>,
    /// Named states of the app: how to launch it and the keys that reach the state.
    /// `cargo xtask check tui` runs each and checks the text its steps expect;
    /// `cargo xtask inspect --story` renders one.
    #[serde(default)]
    pub stories: Vec<Story>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Story {
    pub name: String,
    /// What case it shows, in a sentence (`cargo xtask tui list <app>` prints it).
    #[serde(default)]
    pub about: Option<String>,
    /// Arguments for the app.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment (e.g. making a stand-in tool fail).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub steps: Vec<StoryStep>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoryStep {
    /// Keys to type (printf escapes), empty for the screen as it is.
    #[serde(default)]
    pub keys: String,
    /// Text the screen must show afterwards.
    pub expect: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pages {
    /// Arguments that print every page, one per line (the distro's flags come first).
    pub list: Vec<String>,
    /// The flag that opens a page. The live install screen passes it too, to open the
    /// docs at their page.
    pub arg: String,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading '{}'", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing '{}'", path.display()))
}

impl Workspace {
    /// Reads the root `package.json` and every app's: TUI packages are the members
    /// under `tui/`, apps the ones that declare a `bin`.
    pub fn load(repo_root: &Path) -> Result<Self> {
        let root: RootManifest = read_json(&repo_root.join("package.json"))?;
        let packages: Vec<PathBuf> = root
            .workspaces
            .iter()
            .filter(|member| member.starts_with("tui/"))
            .map(|member| repo_root.join(member))
            .collect();
        let mut apps = Vec::new();
        for dir in &packages {
            let path = dir.join("package.json");
            let manifest: AppManifest = read_json(&path)?;
            if manifest.bin.is_empty() {
                continue;
            }
            apps.push(load_app(dir, &path, manifest)?);
        }
        Ok(Self { packages, apps })
    }

    /// The app called `name`.
    pub fn app(&self, name: &str) -> Result<&TuiApp> {
        self.apps.iter().find(|app| app.name == name).with_context(|| {
            let names: Vec<&str> = self.apps.iter().map(|app| app.name.as_str()).collect();
            format!(
                "no TUI app named '{name}' (apps in the root workspace: {})",
                names.join(", ")
            )
        })
    }
}

fn load_app(dir: &Path, path: &Path, manifest: AppManifest) -> Result<TuiApp> {
    let mut bins = manifest.bin.into_iter();
    let (name, entry) = match (bins.next(), bins.next()) {
        (Some(bin), None) => bin,
        _ => bail!("'{}' must declare exactly one bin", path.display()),
    };
    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if dir_name != name || manifest.name != format!("@levitate/{name}") {
        bail!(
            "'{}': the bin name, the directory name and the package name '@levitate/<name>' must agree (bin '{name}', directory '{dir_name}', package '{}')",
            path.display(),
            manifest.name
        );
    }
    Ok(TuiApp {
        name,
        description: manifest.description,
        dir: dir.to_path_buf(),
        entry: PathBuf::from(entry),
        tools: manifest.levitate.tools,
        distro_args: manifest.levitate.distro_args,
        inspect: manifest.levitate.inspect,
    })
}
