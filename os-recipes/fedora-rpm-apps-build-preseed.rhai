//! extends: locked-preseed.rhai
const RECIPE_API = 1;
// The locked Fedora RPMs that compiling the desktop apps (tools/gui/) needs beyond
// the base (the apps-build lock): the compiler, CMake and Qt's development
// files. The apps-build root recipe imports them as `apps_build_rpms`. None
// of them goes into an image.
const NEEDS = #{
    inputs: #{ store: "directory?" },
    outputs: #{ apps_build_rpms: "directory" },
    network: ["dl.fedoraproject.org:443"],
    memory_mib: 8192,
    runtime: "minimal",
};
import "sources/levitate-rpm-apps-build-lock.rhai" as lock;
const PACKAGE = #{ name: "fedora-rpm-apps-build-preseed", description: "Locked Fedora 45 RPM set for compiling the desktop apps" };
fn rows() { lock::packages() }
