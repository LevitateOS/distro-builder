//! extends: locked-preseed.rhai
const RECIPE_API = 1;
// The locked Fedora RPM set, which the rootfs recipe imports as `rpms`.
const NEEDS = #{
    inputs: #{ store: "directory?" },
    outputs: #{ rpms: "directory" },
    network: ["dl.fedoraproject.org:443", "kojipkgs.fedoraproject.org:443"],
    memory_mib: 8192,
    runtime: "minimal",
};
const PACKAGE = #{ name: "fedora-rpm-preseed", description: "Locked Fedora 45 RPM set" };
import "sources/levitate-rpm-lock.rhai" as lock;
fn rows() { lock::packages() }
