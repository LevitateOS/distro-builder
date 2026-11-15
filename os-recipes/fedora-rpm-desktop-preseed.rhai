//! extends: locked-preseed.rhai
// The locked Fedora RPMs the group recipes in recipes/ pin beyond the base
// (the desktop lock). No image is made of them: the download store they
// land in fills the release medium's package cache (src/default_set.rs).
const RECIPE_API = 2;
const NEEDS = #{ inputs: #{ target: "directory?", store: "directory?" }, outputs: #{ desktop_rpms: "directory" }, network: ["dl.fedoraproject.org:443"], memory_mib: 8192 };
import "sources/levitate-rpm-desktop-lock.rhai" as lock;
let ctx = #{
    name: "fedora-rpm-desktop-preseed",
    description: "Locked Fedora " + lock::release() + " desktop RPM set",
    output: "desktop_rpms",
    files: lock::packages(),
};
