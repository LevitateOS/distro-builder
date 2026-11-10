//! extends: locked-preseed.rhai
// The locked Alpine APKs that compiling the desktop apps (gui/) needs beyond
// the base (the apps-build lock): the compiler, CMake and Qt's development
// files. The apps-build root recipe imports them as `apps_build_apks`. None
// of them goes into an image. apk-tools-static comes with the base preseed.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ target: "directory?" }, outputs: #{ apps_build_apks: "directory" }, network: ["dl-cdn.alpinelinux.org:443"], memory_mib: 4096 };
import "sources/acorn-apk-apps-build-lock.rhai" as lock;
let ctx = #{
    name: "alpine-apps-build-preseed",
    description: "Locked Alpine " + lock::release() + " APK set for compiling the desktop apps",
    output: "apps_build_apks",
    files: lock::packages(),
};
