//! extends: locked-preseed.rhai
const RECIPE_API = 1;
// The locked Fedora RPMs the group recipes in recipes/ pin beyond the base
// (the desktop lock). No image is made of them: the download store they
// land in fills the release medium's package cache (src/default_set.rs).
const NEEDS = #{
    inputs: #{ store: "directory?" },
    outputs: #{ desktop_rpms: "directory" },
    network: ["dl.fedoraproject.org:443", "kojipkgs.fedoraproject.org:443"],
    memory_mib: 8192,
    runtime: "minimal",
};
import "sources/levitate-rpm-desktop-lock.rhai" as lock;
const PACKAGE = #{ name: "fedora-rpm-desktop-preseed", description: "Locked Fedora 45 desktop RPM set" };
fn rows() { lock::packages() }
