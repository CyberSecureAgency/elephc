//! Purpose:
//! Lowers `fopen("compress.bzip2://path", ...)` calls. Opens the underlying
//! file read-only, slurps the bzip2-compressed payload, decompresses it via
//! libbz2's one-shot `BZ2_bzBuffToBuffDecompress`, writes the plain bytes
//! to an anonymous temp file, then `dup2`s that fd onto the original
//! descriptor so subsequent fread/fseek/feof see the decompressed bytes
//! transparently.
//!
//! Called from:
//! - `crate::codegen::builtins::io::fopen::emit()` when the path literal
//!   begins with `compress.bzip2://`.
//!
//! Key details:
//! - The URL must be a string literal; the prefix is stripped at compile
//!   time and the underlying path is opened with mode "r".
//! - Slurp cap = 64 KiB (`_stream_filter_buf`), output cap = 256x the
//!   compressed size (min 64 KiB) — matches the `zlib.inflate` budget.
//! - libbz2 is referenced only from this builtin's USER asm, so programs
//!   that don't use `compress.bzip2://` neither link against nor reference
//!   libbz2. The checker emits `require_builtin_library("bz2")` for
//!   programs that do.
//! - On decompress failure (non-zero return) we skip the dup2 and let the
//!   source fd stay positioned at end-of-file — fread returns empty bytes,
//!   matching how the `zlib.inflate` filter degrades on broken input.

use crate::codegen::abi;
use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::platform::Arch;
use crate::parser::ast::{Expr, ExprKind};
use crate::types::PhpType;

const FILTER_BUF_SIZE: i64 = 65536;

/// Emits a `fopen("compress.bzip2://...", ...)` call. The path is known to
/// be a string literal beginning with `compress.bzip2://`.
pub fn emit(
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("fopen() compress.bzip2:// stream");
    let underlying = match &args[0].kind {
        ExprKind::StringLiteral(path) => path.strip_prefix("compress.bzip2://").map(str::to_string),
        _ => None,
    };
    let underlying = match underlying {
        Some(p) if !p.is_empty() => p,
        _ => {
            match emitter.target.arch {
                Arch::AArch64 => emitter.instruction("mov x0, #-1"),
                Arch::X86_64 => emitter.instruction("mov rax, -1"),
            }
            super::fopen::box_fopen_result(emitter, ctx);
            return Some(PhpType::Mixed);
        }
    };

    let (path_sym, path_len) = data.add_string(underlying.as_bytes());
    let (mode_sym, mode_len) = data.add_string(b"r");
    match emitter.target.arch {
        Arch::AArch64 => {
            abi::emit_symbol_address(emitter, "x1", &path_sym);
            emitter.instruction(&format!("mov x2, #{}", path_len));
            abi::emit_symbol_address(emitter, "x3", &mode_sym);
            emitter.instruction(&format!("mov x4, #{}", mode_len));
            abi::emit_call_label(emitter, "__rt_fopen");
        }
        Arch::X86_64 => {
            abi::emit_symbol_address(emitter, "rax", &path_sym);
            emitter.instruction(&format!("mov rdx, {}", path_len));
            abi::emit_symbol_address(emitter, "rdi", &mode_sym);
            emitter.instruction(&format!("mov rsi, {}", mode_len));
            abi::emit_call_label(emitter, "__rt_fopen");
        }
    }

    let false_label = ctx.next_label("cbz2_false");
    let done_label = ctx.next_label("cbz2_done");
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("cmp x0, #0");
            emitter.instruction(&format!("b.lt {}", false_label));
        }
        Arch::X86_64 => {
            emitter.instruction("test rax, rax");
            emitter.instruction(&format!("js {}", false_label));
        }
    }

    match emitter.target.arch {
        Arch::AArch64 => emit_arm64(emitter, ctx),
        Arch::X86_64 => emit_x86_64(emitter, ctx),
    }
    match emitter.target.arch {
        Arch::AArch64 => emitter.instruction(&format!("b {}", done_label)),
        Arch::X86_64 => emitter.instruction(&format!("jmp {}", done_label)),
    }
    emitter.label(&false_label);
    super::fopen::box_fopen_result(emitter, ctx);
    emitter.label(&done_label);
    Some(PhpType::Mixed)
}

/// ARM64: 96-byte stack frame.
/// Layout:
///   [sp,  0..8)   source fd
///   [sp,  8..16)  slurp offset / compressed length
///   [sp, 16..24)  decompressed buffer pointer
///   [sp, 24..32)  decompressed length (after BZ2 call)
///   [sp, 32..40)  temp fd
///   [sp, 40..48)  write offset
///   [sp, 48..56)  destLen u32 spill (in: capacity, out: bytes written)
///   [sp, 56..64)  padding
///   [sp, 64..72)  saved x29
///   [sp, 72..80)  saved x30
pub(super) fn emit_arm64(emitter: &mut Emitter, ctx: &mut Context) {
    let slurp = ctx.next_label("bz2_slurp");
    let slurp_done = ctx.next_label("bz2_slurped");
    let write = ctx.next_label("bz2_write");
    let write_done = ctx.next_label("bz2_written");
    let decompress_fail = ctx.next_label("bz2_decompress_fail");
    let common_done = ctx.next_label("bz2_done_arm");

    emitter.instruction("sub sp, sp, #96");
    emitter.instruction("stp x29, x30, [sp, #64]");
    emitter.instruction("add x29, sp, #64");
    emitter.instruction("str x0, [sp, #0]");                                    // save source fd

    // Slurp every compressed byte from the descriptor into _stream_filter_buf.
    emitter.instruction("str xzr, [sp, #8]");                                   // slurp offset = 0
    emitter.label(&slurp);
    emitter.instruction("ldr x0, [sp, #0]");                                    // fd to read from
    abi::emit_symbol_address(emitter, "x1", "_stream_filter_buf");
    emitter.instruction("ldr x9, [sp, #8]");
    emitter.instruction("add x1, x1, x9");                                      // write ptr = buf + offset
    emitter.instruction(&format!("mov x2, #{}", FILTER_BUF_SIZE));
    emitter.instruction("sub x2, x2, x9");                                      // remaining capacity
    emitter.syscall(3);                                                         // read
    emitter.instruction("cmp x0, #0");
    emitter.instruction(&format!("b.le {}", slurp_done));                       // EOF or error
    emitter.instruction("ldr x9, [sp, #8]");
    emitter.instruction("add x9, x9, x0");
    emitter.instruction("str x9, [sp, #8]");
    emitter.instruction(&format!("mov x10, #{}", FILTER_BUF_SIZE));
    emitter.instruction("cmp x9, x10");
    emitter.instruction(&format!("b.lt {}", slurp));
    emitter.label(&slurp_done);

    // Size + allocate output buffer (256x input, min 64 KiB).
    emitter.instruction("ldr x9, [sp, #8]");
    emitter.instruction("lsl x9, x9, #8");                                      // 256x compressed
    emitter.instruction(&format!("mov x10, #{}", FILTER_BUF_SIZE));
    emitter.instruction("cmp x9, x10");
    emitter.instruction("csel x9, x9, x10, gt");                                // max(256x, 64KiB)
    emitter.instruction("str w9, [sp, #48]");                                   // destLen = capacity (u32)
    emitter.instruction("mov x0, x9");
    emitter.instruction("bl __rt_heap_alloc");                                  // allocate output buffer
    emitter.instruction("mov x9, #1");                                          // heap kind 1 = persisted string
    emitter.instruction("str x9, [x0, #-8]");
    emitter.instruction("str x0, [sp, #16]");                                   // save output buffer ptr

    // BZ2_bzBuffToBuffDecompress(dest, &destLen, source, sourceLen, 0, 0).
    emitter.instruction("ldr x0, [sp, #16]");                                   // dest
    emitter.instruction("add x1, sp, #48");                                     // &destLen
    abi::emit_symbol_address(emitter, "x2", "_stream_filter_buf");              // source
    emitter.instruction("ldr x3, [sp, #8]");                                    // sourceLen (passed as w3 below)
    emitter.instruction("mov w4, #0");                                          // small = 0
    emitter.instruction("mov w5, #0");                                          // verbosity = 0
    emitter.bl_c("BZ2_bzBuffToBuffDecompress");                                 // libbz2 one-shot decompress
    emitter.instruction("cmp w0, #0");
    emitter.instruction(&format!("b.ne {}", decompress_fail));                  // non-zero = error → skip dup2

    emitter.instruction("ldr w9, [sp, #48]");                                   // destLen now holds bytes written
    emitter.instruction("str x9, [sp, #24]");                                   // save decompressed length

    // Back the descriptor with an anonymous temp file of the plain bytes.
    emitter.instruction("bl __rt_tmpfile");                                     // x0 = temp fd
    emitter.instruction("str x0, [sp, #32]");                                   // save temp fd

    // Write loop.
    emitter.instruction("str xzr, [sp, #40]");                                  // write offset = 0
    emitter.label(&write);
    emitter.instruction("ldr x10, [sp, #24]");                                  // total decompressed length
    emitter.instruction("ldr x9, [sp, #40]");                                   // write offset
    emitter.instruction("cmp x9, x10");
    emitter.instruction(&format!("b.ge {}", write_done));
    emitter.instruction("ldr x0, [sp, #32]");                                   // temp fd
    emitter.instruction("ldr x1, [sp, #16]");
    emitter.instruction("add x1, x1, x9");                                      // src = buf + offset
    emitter.instruction("sub x2, x10, x9");                                     // remaining bytes
    emitter.syscall(4);                                                         // write
    emitter.instruction("cmp x0, #0");
    emitter.instruction(&format!("b.le {}", write_done));                       // bail on error or short write
    emitter.instruction("ldr x9, [sp, #40]");
    emitter.instruction("add x9, x9, x0");
    emitter.instruction("str x9, [sp, #40]");
    emitter.instruction(&format!("b {}", write));
    emitter.label(&write_done);

    // lseek(temp_fd, 0, SEEK_SET) — rewind so reads start at byte 0.
    emitter.instruction("ldr x0, [sp, #32]");
    emitter.instruction("mov x1, #0");                                          // offset
    emitter.instruction("mov x2, #0");                                          // whence = SEEK_SET
    emitter.syscall(199);                                                       // lseek

    // dup2(temp_fd, source_fd) so subsequent reads see decompressed bytes.
    emitter.instruction("ldr x0, [sp, #32]");                                   // oldfd = temp fd
    emitter.instruction("ldr x1, [sp, #0]");                                    // newfd = source fd
    emitter.bl_c("dup2");                                                       // libc dup2
    emitter.instruction("ldr x0, [sp, #32]");                                   // close temp fd
    emitter.syscall(6);                                                         // close

    emitter.label(&decompress_fail);
    emitter.label(&common_done);
    emitter.instruction("ldr x0, [sp, #0]");                                    // return source fd
    emitter.instruction("ldp x29, x30, [sp, #64]");
    emitter.instruction("add sp, sp, #96");
    emitter.instruction("mov x1, x0");                                          // resource payload = fd
    emitter.instruction("mov x2, #0");
    emitter.instruction("mov x0, #9");                                          // tag 9 = resource
    abi::emit_call_label(emitter, "__rt_mixed_from_value");
}

/// x86_64: same shape as ARM64. Frame is rbp-relative (-88 bytes).
pub(super) fn emit_x86_64(emitter: &mut Emitter, ctx: &mut Context) {
    let slurp = ctx.next_label("bz2_slurp_x");
    let slurp_done = ctx.next_label("bz2_slurped_x");
    let write = ctx.next_label("bz2_write_x");
    let write_done = ctx.next_label("bz2_written_x");
    let decompress_fail = ctx.next_label("bz2_decompress_fail_x");
    let common_done = ctx.next_label("bz2_done_x");

    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("sub rsp, 88");                                         // reserve frame; 88≡8 mod 16 so rsp is 16-aligned at libc calls (push rbp made it 8)
    emitter.instruction("mov QWORD PTR [rbp - 8], rax");                        // save source fd

    // Slurp.
    emitter.instruction("mov QWORD PTR [rbp - 16], 0");                         // slurp offset
    emitter.label(&slurp);
    emitter.instruction("mov rdi, QWORD PTR [rbp - 8]");                        // fd
    emitter.instruction("lea rsi, [rip + _stream_filter_buf]");
    emitter.instruction("add rsi, QWORD PTR [rbp - 16]");                       // ptr = buf + offset
    emitter.instruction(&format!("mov rdx, {}", FILTER_BUF_SIZE));
    emitter.instruction("sub rdx, QWORD PTR [rbp - 16]");                       // remaining
    emitter.instruction("call read");                                           // read
    emitter.instruction("cmp rax, 0");
    emitter.instruction(&format!("jle {}", slurp_done));
    emitter.instruction("add QWORD PTR [rbp - 16], rax");                       // bump offset
    emitter.instruction(&format!("cmp QWORD PTR [rbp - 16], {}", FILTER_BUF_SIZE));
    emitter.instruction(&format!("jl {}", slurp));
    emitter.label(&slurp_done);

    // Size + allocate output buffer.
    emitter.instruction("mov rax, QWORD PTR [rbp - 16]");                       // compressed len
    emitter.instruction("shl rax, 8");                                          // 256x
    emitter.instruction(&format!("mov rcx, {}", FILTER_BUF_SIZE));
    emitter.instruction("cmp rax, rcx");
    emitter.instruction("cmovl rax, rcx");                                      // max(256x, 64KiB)
    emitter.instruction("mov DWORD PTR [rbp - 48], eax");                       // destLen u32 = capacity
    emitter.instruction("mov rdi, rax");
    emitter.instruction("call __rt_heap_alloc");
    emitter.instruction("mov QWORD PTR [rax - 8], 1");                          // heap kind = string
    emitter.instruction("mov QWORD PTR [rbp - 24], rax");                       // save output buffer ptr

    // BZ2_bzBuffToBuffDecompress(dest, &destLen, source, sourceLen, 0, 0).
    emitter.instruction("mov rdi, QWORD PTR [rbp - 24]");                       // dest
    emitter.instruction("lea rsi, [rbp - 48]");                                 // &destLen
    emitter.instruction("lea rdx, [rip + _stream_filter_buf]");                 // source
    emitter.instruction("mov ecx, DWORD PTR [rbp - 16]");                       // sourceLen u32 (compressed len)
    emitter.instruction("xor r8d, r8d");                                        // small = 0
    emitter.instruction("xor r9d, r9d");                                        // verbosity = 0
    emitter.bl_c("BZ2_bzBuffToBuffDecompress");                                 // libbz2 one-shot decompress
    emitter.instruction("test eax, eax");
    emitter.instruction(&format!("jnz {}", decompress_fail));                   // non-zero = error

    emitter.instruction("mov eax, DWORD PTR [rbp - 48]");                       // destLen now holds decompressed length
    emitter.instruction("mov QWORD PTR [rbp - 32], rax");                       // save decompressed length

    // Temp file backing.
    emitter.instruction("call __rt_tmpfile");                                   // rax = temp fd
    emitter.instruction("mov QWORD PTR [rbp - 40], rax");

    // Write loop.
    emitter.instruction("mov QWORD PTR [rbp - 56], 0");                         // write offset
    emitter.label(&write);
    emitter.instruction("mov rcx, QWORD PTR [rbp - 32]");                       // total
    emitter.instruction("mov rax, QWORD PTR [rbp - 56]");                       // offset
    emitter.instruction("cmp rax, rcx");
    emitter.instruction(&format!("jge {}", write_done));
    emitter.instruction("mov rdi, QWORD PTR [rbp - 40]");                       // temp fd
    emitter.instruction("mov rsi, QWORD PTR [rbp - 24]");
    emitter.instruction("add rsi, rax");                                        // src = buf + offset
    emitter.instruction("mov rdx, rcx");
    emitter.instruction("sub rdx, rax");                                        // remaining bytes
    emitter.instruction("call write");
    emitter.instruction("cmp rax, 0");
    emitter.instruction(&format!("jle {}", write_done));
    emitter.instruction("add QWORD PTR [rbp - 56], rax");
    emitter.instruction(&format!("jmp {}", write));
    emitter.label(&write_done);

    // lseek + dup2 + close.
    emitter.instruction("mov rdi, QWORD PTR [rbp - 40]");
    emitter.instruction("xor esi, esi");                                        // offset = 0
    emitter.instruction("xor edx, edx");                                        // whence = SEEK_SET
    emitter.instruction("call lseek");                                          // libc lseek
    emitter.instruction("mov rdi, QWORD PTR [rbp - 40]");                       // oldfd = temp fd
    emitter.instruction("mov rsi, QWORD PTR [rbp - 8]");                        // newfd = source fd
    emitter.instruction("call dup2");
    emitter.instruction("mov rdi, QWORD PTR [rbp - 40]");
    emitter.instruction("call close");

    emitter.label(&decompress_fail);
    emitter.label(&common_done);
    emitter.instruction("mov rax, QWORD PTR [rbp - 8]");                        // return source fd
    emitter.instruction("add rsp, 88");                                         // release the 88-byte frame (matches the aligned sub rsp, 88)
    emitter.instruction("pop rbp");
    emitter.instruction("mov rdi, rax");                                        // resource payload = fd
    emitter.instruction("xor esi, esi");
    emitter.instruction("mov eax, 9");                                          // tag 9 = resource
    abi::emit_call_label(emitter, "__rt_mixed_from_value");
}
