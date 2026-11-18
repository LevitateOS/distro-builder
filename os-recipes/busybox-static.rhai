const RECIPE_API = 1;
// The static busybox the live and the installed initramfs run (recinit): the
// prebuilt musl binary busybox.net publishes, pinned by its sha256.
const NEEDS = #{ outputs: #{ busybox: "file" }, network: ["busybox.net:443"], runtime: "minimal" };
const PACKAGE = #{ name: "busybox-static", version: "1.35.0", description: "Static busybox for the initramfs" };
const SOURCES = #{
    busybox: #{
        url: "https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox",
        sha256: "6e123e7f3202a8c1e9b1f94d8941580a25135382b99e8d3e34fb858bba311348",
    },
};
// The downloaded file becomes the output file, executable.
const FILES = [["busybox", "", 0o755]];
