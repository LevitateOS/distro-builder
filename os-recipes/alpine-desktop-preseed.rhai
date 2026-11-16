//! extends: locked-preseed.rhai
const RECIPE_API = 1;
// The locked Alpine APKs the group recipes in recipes/ pin beyond the base
// (the desktop lock). No image is made of them: the download store they
// land in fills the release medium's package cache (src/default_set.rs).
// apk-tools-static comes with the base preseed.
const NEEDS = #{
    inputs: #{ store: "directory?" },
    outputs: #{ desktop_apks: "directory" },
    network: ["dl-cdn.alpinelinux.org:443"],
    memory_mib: 4096,
};
import "sources/acorn-apk-desktop-lock.rhai" as lock;
const PACKAGE = #{ name: "alpine-desktop-preseed", description: "Locked Alpine v3.24 desktop APK set" };
fn rows() { lock::packages() }
