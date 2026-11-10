//! extends: locked-preseed.rhai
// The locked Alpine APKs and apk-tools-static, which the rootfs recipe
// imports as `apks`.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ target: "directory?" }, outputs: #{ apks: "directory" }, network: ["dl-cdn.alpinelinux.org:443"], memory_mib: 4096 };
import "sources/acorn-apk-lock.rhai" as lock;
let ctx = #{
    name: "alpine-preseed-source-assets",
    description: "Locked Alpine " + lock::release() + " APK set",
    output: "apks",
    files: lock::packages() + [lock::apk_tools()],
};
