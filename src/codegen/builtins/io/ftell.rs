//! Purpose:
//! Emits PHP `ftell` stream builtin calls over runtime file handles.
//! Uses shared stream unboxing before invoking file descriptor runtime helpers.
//!
//! Called from:
//! - `crate::codegen::builtins::io::emit()`.
//!
//! Key details:
//! - Stream resources must be validated and failure results must follow PHP false/null conventions.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::platform::Arch;
use crate::parser::ast::Expr;
use crate::types::PhpType;

use super::stream_arg::emit_stream_fd_arg;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("ftell()");
    emit_stream_fd_arg("ftell", &args[0], emitter, ctx, data);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x1, #0");                                  // offset = 0 for the AArch64 ftell() lseek syscall
            emitter.instruction("mov x2, #1");                                  // whence = SEEK_CUR for the AArch64 ftell() lseek syscall
            emitter.syscall(199);                                               // ask the kernel for the current file position through lseek()
        }
        Arch::X86_64 => {
            emitter.instruction("mov rdi, rax");                                // move the file descriptor into the first SysV lseek() argument register
            emitter.instruction("xor esi, esi");                                // offset = 0 for the linux-x86_64 ftell() lseek() call
            emitter.instruction("mov edx, 1");                                  // whence = SEEK_CUR for the linux-x86_64 ftell() lseek() call
            emitter.instruction("call lseek");                                  // ask libc lseek() for the current file position on linux-x86_64
        }
    }
    Some(PhpType::Int)
}
