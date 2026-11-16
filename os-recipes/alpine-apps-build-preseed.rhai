//! extends: locked-preseed.rhai
const RECIPE_API = 1;
// The locked Alpine APKs that compiling the desktop apps (tools/gui/) needs beyond
// the base (the apps-build lock): the compiler, CMake and Qt's development
// files. The apps-build root recipe imports them as `apps_build_apks`. None
// of them goes into an image. apk-tools-static comes with the base preseed.
const NEEDS = #{
    inputs: #{ store: "directory?" },
    outputs: #{ apps_build_apks: "directory" },
    network: ["dl-cdn.alpinelinux.org:443"],
    memory_mib: 4096,
};
import "sources/acorn-apk-apps-build-lock.rhai" as lock;
const PACKAGE = #{ name: "alpine-apps-build-preseed", description: "Locked Alpine v3.24 APK set for compiling the desktop apps" };
fn rows() { lock::packages() }
