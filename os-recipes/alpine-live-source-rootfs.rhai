//! extends: rootfs-producer.rhai
// The APK backend of the system recipe: the OpenRC system configured over
// the package tree of the locked Alpine APK set (the `system` input, which
// `alpine-apk-package-effects.rhai` makes: the installed APKs with what their
// scripts would have done). On the desktop system (REC_FLAVOUR=desktop in the
// `os_release` input) that tree holds the desktop lock over the base, so the
// installed-system policy runs over base and desktop packages alike.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ system: "file", capabilities: "file", system_setup: "file", os_release: "file", admin_rule_file: "file", admin_rule: "file", display_manager: "file?", look: "file?", wallpaper_light: "file?", wallpaper_dark: "file?", logo_ink: "file?", logo_paper: "file?", logo_micro_ink: "file?", logo_micro_paper: "file?" }, outputs: #{ rootfs: "directory" }, memory_mib: 8192, storage_max_objects: 262144, timeout_secs: 14400 };
let ctx = #{
    name: "alpine-live-source-rootfs",
    description: "The configured system rootfs from the locked Alpine APK set",
    init: "openrc",
};
