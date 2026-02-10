let ctx = #{
    name: "mock-build-stdout-test",
};

fn is_installed(ctx) { throw "always run"; }
fn is_acquired(ctx) { ctx }

fn install(ctx) {
    let lines = [
        "  CC      kernel/fork.o",
        "  CC      kernel/exec_domain.o",
        "  CC      kernel/panic.o",
        "  CC      kernel/cpu.o",
        "  CC      kernel/exit.o",
        "  CC      kernel/softirq.o",
        "  CC      kernel/resource.o",
        "  CC      kernel/sysctl.o",
        "  CC      kernel/sched/core.o",
        "  CC      kernel/sched/fair.o",
        "  LD      kernel/sched/built-in.a",
        "  CC      mm/filemap.o",
        "  CC      mm/mempool.o",
        "  CC      mm/oom_kill.o",
        "  CC      mm/page_alloc.o",
        "  CC      mm/swap.o",
        "  CC      mm/vmscan.o",
        "  LD      mm/built-in.a",
        "  CC      fs/open.o",
        "  CC      fs/read_write.o",
        "  CC      fs/namei.o",
        "  CC      fs/dcache.o",
        "  CC      fs/inode.o",
        "  LD      fs/built-in.a",
        "  CC      net/socket.o",
        "  CC      net/core/sock.o",
        "  CC      net/core/skbuff.o",
        "  LD      net/built-in.a",
        "  CC      drivers/base/core.o",
        "  CC      drivers/base/bus.o",
        "  LD      drivers/built-in.a",
        "  AR      built-in.a",
        "  LD      vmlinux.o",
        "  MODPOST vmlinux.symvers",
        "  LD      .tmp_vmlinux.kallsyms1",
        "  KSYMS   .tmp_vmlinux.kallsyms1.S",
        "  AS      .tmp_vmlinux.kallsyms1.S",
        "  LD      .tmp_vmlinux.kallsyms2",
        "  KSYMS   .tmp_vmlinux.kallsyms2.S",
        "  AS      .tmp_vmlinux.kallsyms2.S",
        "  LD      vmlinux",
        "  BTFIDS  vmlinux",
        "  SORTTAB vmlinux",
        "  SYSMAP  System.map",
        "  OBJCOPY arch/x86/boot/compressed/vmlinux.bin",
        "  GZIP    arch/x86/boot/compressed/vmlinux.bin.gz",
        "  MKPIGGY arch/x86/boot/compressed/piggy.S",
        "  CC      arch/x86/boot/compressed/misc.o",
        "  LD      arch/x86/boot/compressed/vmlinux",
        "  OBJCOPY arch/x86/boot/bzImage",
        "Kernel: arch/x86/boot/bzImage is ready",
    ];

    for line in lines {
        shell("echo '" + line + "' && sleep 0.1");
    }

    ctx
}
