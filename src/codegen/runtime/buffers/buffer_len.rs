//! Purpose:
//! Emits the `__rt_buffer_len`, `__rt_buffer_use_after_free` runtime helper assembly for buffer length reads.
//! Keeps compiler buffer extension checks and fatal paths aligned with generated pointer operations.
//!
//! Called from:
//! - `crate::codegen::runtime::emitters::emit_runtime()` via `crate::codegen::runtime::buffers`.
//!
//! Key details:
//! - Buffer helpers enforce extension ownership rules, including live headers, bounds checks, and fatal paths before unsafe access.

use crate::codegen::emit::Emitter;
use crate::codegen::platform::Arch;

pub fn emit_buffer_len(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_buffer_len_linux_x86_64(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: buffer_len ---");
    emitter.label_global("__rt_buffer_len");
    emitter.instruction("cbz x0, __rt_buffer_use_after_free");                  // abort deterministically when buffer_len() is called after buffer_free()
    emitter.instruction("ldr x0, [x0]");                                        // load the logical element count from the buffer header
    emitter.instruction("ret");                                                 // return length in x0
}

fn emit_buffer_len_linux_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: buffer_len ---");
    emitter.label_global("__rt_buffer_len");
    emitter.instruction("test rax, rax");                                       // abort deterministically when buffer_len() is called after buffer_free() nulled the local slot
    emitter.instruction("jz __rt_buffer_use_after_free");                       // jump to the shared fatal helper when the buffer header pointer is null
    emitter.instruction("mov rax, QWORD PTR [rax]");                            // load the logical element count from the buffer header
    emitter.instruction("ret");                                                 // return the logical length in rax
}
