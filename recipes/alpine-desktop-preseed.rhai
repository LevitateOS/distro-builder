//! extends: locked-preseed.rhai
// The locked Alpine APKs the desktop adds to the base (the desktop lock),
// which the rootfs recipe imports as `desktop_apks` when it builds the
// desktop system. apk-tools-static comes with the base preseed.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ target: "directory?" }, outputs: #{ desktop_apks: "directory" }, network: ["dl-cdn.alpinelinux.org:443"], memory_mib: 4096 };
import "sources/acorn-apk-desktop-lock.rhai" as lock;
let ctx = #{
    name: "alpine-desktop-preseed",
    description: "Locked Alpine " + lock::release() + " desktop APK set",
    output: "desktop_apks",
    files: lock::packages(),
};
