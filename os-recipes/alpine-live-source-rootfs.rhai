//! extends: rootfs-producer.rhai
const RECIPE_API = 1;
// The APK backend of the system recipe: the OpenRC system configured over
// the package tree of the locked Alpine APK set (the `system` input, which
// `alpine-apk-package-effects.rhai` makes: the installed APKs with what their
// scripts would have done): the base system. Nothing above the base is in it;
// the default software set is installed through Recipe during the
// installation.
const NEEDS = #{ inputs: #{ system: "file", capabilities: "file", system_setup: "file", os_release: "file", licenses: "file", admin_rule_file: "file", admin_rule: "file", root_effects: "file", wallpaper_light: "file", wallpaper_dark: "file", logo_ink: "file", logo_paper: "file", logo_micro_ink: "file", logo_micro_paper: "file" }, outputs: #{ rootfs: "directory" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 14400, runtime: "minimal" };
const PACKAGE = #{ name: "alpine-live-source-rootfs", description: "The configured system rootfs from the locked Alpine APK set" };
const INIT_SYSTEM = "openrc";
