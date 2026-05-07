//! Fiber stack allocation.
//!
//! Per-fiber stacks are allocated via `mmap` with `MAP_PRIVATE | MAP_ANON` and a
//! 16 KB guard page at the bottom protected with `PROT_NONE`. A stack overflow
//! that touches the guard page faults via SIGSEGV instead of silently
//! corrupting the heap. The full mmap'd region is returned to the OS via
//! `munmap` in the object_free_deep hook when the Fiber object is released.

use crate::codegen::emit::Emitter;
use crate::codegen::platform::{Arch, Platform};

/// Size of the unmapped (PROT_NONE) guard region at the bottom of every fiber
/// stack. Set to 16 KB so the guard fully covers a single page on every
/// supported target — macOS aarch64 uses 16 KB pages, Linux aarch64 typically
/// uses 4 KB but accepts oversized protection ranges silently.
const FIBER_GUARD_PAGE_SIZE: i32 = 16384;

/// Combined `MAP_PRIVATE | MAP_ANON` flag word per platform. macOS uses
/// `MAP_ANON = 0x1000`; Linux uses `MAP_ANONYMOUS = 0x20`.
fn map_anon_private_flags(platform: Platform) -> i32 {
    match platform {
        Platform::MacOS => 0x1002, // MAP_PRIVATE | MAP_ANON
        Platform::Linux => 0x22,   // MAP_PRIVATE | MAP_ANONYMOUS
    }
}

/// __rt_fiber_alloc_stack: reserve a usable stack with a guard page.
/// Input:  x0 = requested usable stack size in bytes
/// Output: x0 = stack_base (low address — the mmap region start, includes the guard page)
///         x1 = stack_top  (high address, initial SP, 16-byte aligned)
///         x2 = total mmap'd length in bytes (usable size + guard page) — needed for munmap
pub fn emit_fiber_alloc_stack(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_x86_64_stub(emitter, "__rt_fiber_alloc_stack");
        return;
    }

    let map_flags = map_anon_private_flags(emitter.target.platform);

    emitter.blank();
    emitter.comment("--- runtime: fiber_alloc_stack (mmap + guard page) ---");
    emitter.label_global("__rt_fiber_alloc_stack");

    // -- prologue: save x19/x20/x21 (callee-saved) since we use them as scratch --
    emitter.instruction("sub sp, sp, #48");                                     // reserve frame plus three saved-callee slots
    emitter.instruction("stp x29, x30, [sp, #32]");                             // save frame pointer and return address
    emitter.instruction("stp x19, x20, [sp]");                                  // preserve caller's x19/x20 — used to remember alloc state across libc calls
    emitter.instruction("str x21, [sp, #16]");                                  // preserve caller's x21 — used to hold the mmap base across mprotect
    emitter.instruction("add x29, sp, #32");                                    // anchor the new frame pointer
    emitter.instruction("mov x19, x0");                                         // x19 = requested usable stack size (callee-saved across mmap/mprotect)

    // -- mmap(NULL, requested_size + GUARD, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANON, -1, 0) --
    emitter.instruction("mov x0, #0");                                          // addr = NULL — let the kernel pick the address
    emitter.instruction(&format!("add x1, x19, #{}", FIBER_GUARD_PAGE_SIZE));   // length = usable size + guard page
    emitter.instruction("mov x20, x1");                                         // x20 = total mapped length (callee-saved across mprotect)
    emitter.instruction("mov x2, #3");                                          // prot = PROT_READ | PROT_WRITE
    emitter.instruction(&format!("mov x3, #{}", map_flags));                    // flags = MAP_PRIVATE | MAP_ANON for this platform
    emitter.instruction("mov x4, #-1");                                         // fd = -1 for anonymous mappings
    emitter.instruction("mov x5, #0");                                          // offset = 0 for anonymous mappings
    emitter.bl_c("mmap");                                                       // x0 = mapping base on success, MAP_FAILED (-1) on failure

    // -- bail out cleanly when mmap returns MAP_FAILED so callers see (0, 0, 0) --
    emitter.instruction("cmn x0, #1");                                          // is x0 == -1 (MAP_FAILED)?
    emitter.instruction("b.eq __rt_fiber_alloc_stack_fail");                    // skip mprotect and return zeros when mmap failed

    // -- mprotect(base, GUARD_PAGE_SIZE, PROT_NONE) installs the guard at the bottom --
    emitter.instruction("mov x21, x0");                                         // x21 = mapping base (callee-saved across mprotect; libc preserves x19-x28)
    emitter.instruction(&format!("mov x1, #{}", FIBER_GUARD_PAGE_SIZE));        // length = one guard page
    emitter.instruction("mov x2, #0");                                          // prot = PROT_NONE — touching the guard faults via SIGSEGV
    emitter.bl_c("mprotect");                                                   // ignore the return value: a failure here would still leave a usable stack

    // -- compute stack_top = mapping base + total length, aligned down to 16 --
    emitter.instruction("add x1, x21, x20");                                    // x1 = end of mapped region (one past the last usable byte)
    emitter.instruction("and x1, x1, #-16");                                    // round stack_top down to a 16-byte boundary for AArch64 SP alignment

    // -- pack outputs: x0 = base, x1 = top, x2 = total length for munmap --
    emitter.instruction("mov x0, x21");                                         // x0 = mapping base (also serves as stack_base for free)
    emitter.instruction("mov x2, x20");                                         // x2 = total mapped length

    // -- epilogue --
    emitter.instruction("ldr x21, [sp, #16]");                                  // restore caller's x21
    emitter.instruction("ldp x19, x20, [sp]");                                  // restore caller's x19/x20
    emitter.instruction("ldp x29, x30, [sp, #32]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #48");                                     // release the allocator scratch frame
    emitter.instruction("ret");                                                 // hand the (base, top, length) triple back to the constructor

    // -- mmap failure path: report a clean (0, 0, 0) so the constructor can detect it --
    emitter.label("__rt_fiber_alloc_stack_fail");
    emitter.instruction("mov x0, #0");                                          // stack_base = 0 indicates the alloc failed
    emitter.instruction("mov x1, #0");                                          // stack_top = 0 mirrors the failure signal
    emitter.instruction("mov x2, #0");                                          // total length = 0 so a defensive free is a no-op
    emitter.instruction("ldr x21, [sp, #16]");                                  // restore caller's x21
    emitter.instruction("ldp x19, x20, [sp]");                                  // restore caller's x19/x20
    emitter.instruction("ldp x29, x30, [sp, #32]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #48");                                     // release the allocator scratch frame
    emitter.instruction("ret");                                                 // bail out with the failure triple
}

/// __rt_fiber_free_stack: return a fiber stack to the kernel via munmap.
/// Input:  x0 = stack_base (mapping base returned by __rt_fiber_alloc_stack)
///         x1 = total mapped length (the third return value of alloc, stored in stack_size)
pub fn emit_fiber_free_stack(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_x86_64_stub(emitter, "__rt_fiber_free_stack");
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_free_stack (munmap) ---");
    emitter.label_global("__rt_fiber_free_stack");

    emitter.instruction("cbz x0, __rt_fiber_free_stack_done");                  // skip NULL bases (alloc failure or already freed)
    emitter.instruction("cbz x1, __rt_fiber_free_stack_done");                  // skip zero-length mappings as a defensive guard against double-free
    emitter.instruction("stp x29, x30, [sp, #-16]!");                           // save the caller's frame pointer and return address before invoking libc
    emitter.bl_c("munmap");                                                     // ignore the return value: the stack is gone either way
    emitter.instruction("ldp x29, x30, [sp], #16");                             // restore the caller's frame pointer and return address
    emitter.label("__rt_fiber_free_stack_done");
    emitter.instruction("ret");                                                 // hand control back to the caller (NULL/zero-length path also lands here)
}

/// Placeholder for x86_64 — fibers are ARM64-only in the MVP.
fn emit_x86_64_stub(emitter: &mut Emitter, name: &str) {
    emitter.blank();
    emitter.comment(&format!("--- runtime: {} (x86_64 stub) ---", name));
    emitter.label_global(name);
    emitter.instruction("ret");                                                 // x86_64 fiber runtime not yet implemented
}
