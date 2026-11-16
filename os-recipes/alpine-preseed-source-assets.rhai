//! extends: locked-preseed.rhai
const RECIPE_API = 1;
// The locked Alpine APKs and apk-tools-static, which the rootfs recipe
// imports as `apks`.
const NEEDS = #{
    inputs: #{ store: "directory?" },
    outputs: #{ apks: "directory" },
    network: ["dl-cdn.alpinelinux.org:443"],
    memory_mib: 4096,
};
import "sources/acorn-apk-lock.rhai" as lock;
const PACKAGE = #{ name: "alpine-preseed-source-assets", description: "Locked Alpine v3.24 APK set" };
fn rows() { lock::packages() + [lock::apk_tools()] }
