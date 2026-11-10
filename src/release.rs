//! Release assembly: a release product's ISO, built in its run directory
//! from the prepared product, the variant's kernel and the boot command line,
//! by calling recinit and reciso. The live medium boots the target rootfs's
//! own systemd-boot.

use anyhow::{bail, Context, Result};
use distro_contract::{ProductId, Variant};
use distro_profile::InitSystem;
use std::fs;
use std::path::{Path, PathBuf};

use crate::kernel::{KERNEL_IMAGE_PATH, KERNEL_MODULES_DIR, KERNEL_RELEASE_PATH};
use crate::recipe::{recipe_work_dir, Invocation};

/// What a live UKI boots into; each role is one UKI and one boot menu entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootRole {
    /// The default entry: the live system.
    Live,
    /// The live init's shell before the rootfs is mounted.
    Emergency,
    /// The live system with the kernel's and the init system's debug output.
    Debug,
}

impl BootRole {
    /// In boot menu order; the first is the default entry.
    pub const ALL: [BootRole; 3] = [Self::Live, Self::Emergency, Self::Debug];

    /// The role's word on the kernel command line.
    fn token(self) -> Option<&'static str> {
        match self {
            Self::Live => None,
            Self::Emergency => Some(recinit::live::CMDLINE_EMERGENCY),
            // The kernel's own parameter (debug console log level), which
            // systemd also reads (debug logging).
            Self::Debug => Some("debug"),
        }
    }

    fn menu_suffix(self) -> &'static str {
        match self {
            Self::Live => "",
            Self::Emergency => " (Emergency)",
            Self::Debug => " (Debug)",
        }
    }
}

/// Tokens on every boot of every variant: both consoles (the last named,
/// tty0, is `/dev/console`), the audit trail from the first process with room
/// for its early records (auditd ships on both), and the display mode.
fn shared_tokens() -> [String; 5] {
    [
        format!("console={}", recinit::live::SERIAL_CONSOLE),
        "console=tty0".into(),
        "audit=1".into(),
        "audit_backlog_limit=8192".into(),
        "video=1920x1080".into(),
    ]
}

/// The quiet boot: only kernel errors on the console, and the init system's
/// own quiet switches.
fn quiet_tokens(init: InitSystem) -> &'static [&'static str] {
    match init {
        InitSystem::Systemd => &["loglevel=3", "systemd.show_status=0", "rd.systemd.show_status=0"],
        InitSystem::Openrc => &["loglevel=3"],
    }
}

/// The kernel command line of a live boot in `role` (reciso puts the medium's
/// `root=LABEL=` before it): the shared tokens, the quiet ones (except for
/// the debug role), then the role's word.
pub fn kernel_cmdline(init: InitSystem, role: BootRole) -> Vec<String> {
    let quiet: &[&str] = if role == BootRole::Debug { &[] } else { quiet_tokens(init) };
    shared_tokens()
        .into_iter()
        .chain(quiet.iter().map(|token| token.to_string()))
        .chain(role.token().map(str::to_string))
        .collect()
}

/// Where the rootfs ships systemd-boot's x86-64 EFI binary (the rootfs
/// producers require it; recab installs it on the installed ESP).
const SYSTEMD_BOOT_EFI: &str = "usr/lib/systemd/boot/efi/systemd-bootx64.efi";

/// The recipe that produces the release's busybox binary.
const BUSYBOX_RECIPE: &str = "distro/builder/os-recipes/busybox-static.rhai";

/// Builds `product`'s release ISO in `dir`, its release run's directory, with
/// the kernel in `kernel` (`runs::kernel_dir`), and returns the ISO's path.
/// Every step's error names the step and the product.
pub fn assemble(variant: &Variant, product: ProductId, dir: &Path, kernel: &Path) -> Result<PathBuf> {
    if !product.is_release() {
        bail!("{product} is prepared, not built into a release ISO");
    }
    let contract = &variant.contract;
    let identity = &contract.identity;
    let names = &contract.artifacts;
    let id = &variant.id;
    let step = |what: &str| {
        println!("[release:{product}:{id}] {what}");
        format!("{what} for the {product} release of {id}")
    };

    let what = step("reading the kernel");
    let release = kernel_release(kernel).context(what)?;
    let modules = kernel.join(KERNEL_MODULES_DIR).join(&release);
    if !modules.is_dir() {
        bail!("the {release} kernel has no modules at {}", modules.display());
    }

    let what = step("preparing the product");
    let prepared = crate::prepare_product(variant, product, dir).context(what)?;
    let rootfs = prepared.rootfs_source(dir);

    let what = step("adding the kernel modules to the rootfs");
    add_modules(&rootfs, &modules, &release).context(what)?;

    let what = step("writing the rootfs image");
    let rootfs_image = dir.join(&names.rootfs);
    crate::build_erofs_default(&rootfs, &rootfs_image).context(what)?;

    let what = step("acquiring busybox");
    let busybox = busybox(variant).context(what)?;

    let what = step("building the live initramfs");
    let initramfs_live = dir.join(&names.initramfs_live);
    recinit::build_tiny_initramfs(
        &recinit::TinyConfig {
            modules_dir: modules.clone(),
            busybox_path: busybox.clone(),
            output: initramfs_live.clone(),
            iso_label: identity.iso_label.clone(),
            boot_banner: format!("{} {} initramfs starting...", identity.os_name, product.title()),
            gzip_level: recinit::DEFAULT_GZIP_LEVEL,
        },
        false,
    )
    .context(what)?;

    // Every product carries the installed system, on every variant, so every
    // ISO carries the initramfs it boots through (recsetup installs it). It
    // is built from the rootfs the ISO installs, so its fsck, blkid and
    // modules match the installed system.
    let what = step("building the installed initramfs");
    let initramfs_installed = dir.join(recinit::live::INITRAMFS_INSTALLED_FILENAME);
    recinit::build_install_initramfs(
        &recinit::InstallConfig {
            rootfs: rootfs.clone(),
            busybox_path: busybox,
            modules_dir: modules,
            output: initramfs_installed.clone(),
            gzip_level: recinit::DEFAULT_GZIP_LEVEL,
        },
        false,
    )
    .context(what)?;

    let what = step("writing the overlay image");
    let overlay_image = dir.join(&names.overlay);
    crate::build_overlayfs_default(&prepared.overlay(dir), &overlay_image).context(what)?;

    let what = step("writing the ISO");
    let iso = dir.join(names.iso(product));
    let menu = format!("{} {}", identity.os_name, product.title());
    let mut config = reciso::IsoConfig::new(
        kernel.join(KERNEL_IMAGE_PATH),
        &initramfs_live,
        &rootfs_image,
        &identity.iso_label,
        &iso,
    );
    config.ukis = BootRole::ALL
        .into_iter()
        .map(|role| reciso::UkiSpec {
            name: format!("{menu}{}", role.menu_suffix()),
            extra_cmdline: kernel_cmdline(contract.profile.init, role).join(" "),
            filename: match role {
                BootRole::Live => names.live_uki.live.clone(),
                BootRole::Emergency => names.live_uki.emergency.clone(),
                BootRole::Debug => names.live_uki.debug.clone(),
            },
        })
        .collect();
    config.overlay_image = Some(overlay_image);
    config.os_release = Some(recuki::OsRelease::new(
        &identity.os_name,
        &identity.os_id,
        &identity.os_version,
    ));
    config.extra_files = vec![(
        initramfs_installed,
        recinit::live::INITRAMFS_INSTALLED_ISO_PATH.to_string(),
    )];
    config.systemd_boot = rootfs.join(SYSTEMD_BOOT_EFI);
    let iso = reciso::create_iso(&config, &|message| println!("  {message}")).context(what)?;

    print_summary(dir)?;
    Ok(iso)
}

/// The kernel's release (`kernel.release`, one line).
fn kernel_release(kernel: &Path) -> Result<String> {
    let path = kernel.join(KERNEL_RELEASE_PATH);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let release = text.strip_suffix('\n').unwrap_or(&text);
    if release.is_empty() || release.contains(char::is_whitespace) {
        bail!("{} does not hold one kernel release", path.display());
    }
    Ok(release.to_string())
}

/// Puts the kernel's modules into the system the ISO runs and installs,
/// beside the kernel it boots; without them no modular driver (WiFi's among
/// them) can load. `lib/modules` is `/usr/lib/modules` where `/lib` links to
/// `usr/lib`, and busybox's modprobe reads `/lib/modules`.
fn add_modules(rootfs: &Path, modules: &Path, release: &str) -> Result<()> {
    let dest = rootfs.join("lib/modules").join(release);
    if fs::symlink_metadata(&dest).is_ok() {
        fs::remove_dir_all(&dest).with_context(|| format!("removing {}", dest.display()))?;
    }
    crate::copy_dir_recursive(modules, &dest)?;
    if !dest.join("modules.dep").is_file() {
        bail!("the kernel's modules {} have no modules.dep", modules.display());
    }
    Ok(())
}

/// The static busybox the initramfs run, from its recipe.
pub fn busybox(variant: &Variant) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    Invocation::new(repo, "install", repo.join(BUSYBOX_RECIPE), recipe_work_dir(repo, &variant.id)?)
        .run()?
        .file("busybox")
}

/// Lists the run directory's files with their sizes.
fn print_summary(dir: &Path) -> Result<()> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            files.push((entry.file_name().to_string_lossy().into_owned(), metadata.len()));
        }
    }
    files.sort();
    let width = files.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    println!("Artifacts in {}:", dir.display());
    for (name, bytes) in files {
        println!("  {name:<width$}  {:>8.1} MiB", bytes as f64 / (1024.0 * 1024.0));
    }
    Ok(())
}
