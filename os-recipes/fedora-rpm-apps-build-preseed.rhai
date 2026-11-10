//! extends: locked-preseed.rhai
// The locked Fedora RPMs that compiling the desktop apps (gui/) needs beyond
// the base (the apps-build lock): the compiler, CMake and Qt's development
// files. The apps-build root recipe imports them as `apps_build_rpms`. None
// of them goes into an image.
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ target: "directory?" }, outputs: #{ apps_build_rpms: "directory" }, network: ["dl.fedoraproject.org:443"], memory_mib: 8192 };
import "sources/levitate-rpm-apps-build-lock.rhai" as lock;
let ctx = #{
    name: "fedora-rpm-apps-build-preseed",
    description: "Locked Fedora " + lock::release() + " RPM set for compiling the desktop apps",
    output: "apps_build_rpms",
    files: lock::packages(),
};
