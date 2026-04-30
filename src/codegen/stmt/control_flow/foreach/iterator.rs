use crate::codegen::context::{Context, HeapOwnership, LoopLabels};
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::objects::dispatch::emit_dispatch_instance_method;
use crate::codegen::platform::Arch;
use crate::codegen::stmt::emit_stmt;
use crate::parser::ast::Stmt;
use crate::types::PhpType;

/// Foreach over an object implementing the Iterator interface.
///
/// On entry, x0 already holds the iterator object pointer (left there by
/// `emit_expr` on the foreach iterable expression).
///
/// Loop shape:
///
/// ```text
/// rewind()
/// loop_start:
///     valid()  ; if !valid jump loop_end
///     key()    ; if requested -> key_var (Mixed)
///     current(); -> value_var (Mixed)
///     <body>
/// loop_cont:
///     next()
///     b loop_start
/// loop_end:
/// ```
///
/// The receiver pointer is parked in a 16-byte stack slot so it survives the
/// nested method calls without burning a callee-saved register. Each method
/// call reloads `x0` from that slot before dispatching through the vtable.
pub(crate) fn emit_iterator_foreach(
    class_name: &str,
    key_var: &Option<String>,
    value_var: &str,
    body: &[Stmt],
    loop_start: &str,
    loop_end: &str,
    loop_cont: &str,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    if emitter.target.arch != Arch::AArch64 {
        unimplemented!("foreach over Iterator object is only implemented for AArch64 in this slice");
    }

    // Resolve the class whose vtable we should use for the iteration. If
    // `class_name` implements Iterator directly, that's it. Otherwise the
    // class must implement IteratorAggregate; we call its `getIterator()`
    // method to obtain the actual iterator and use the static return type
    // (a concrete Iterator class) for the per-iteration dispatches.
    let (implements_iterator, get_iterator_return) = {
        let info = ctx.classes.get(class_name);
        let direct = info
            .map(|ci| ci.interfaces.iter().any(|name| name == "Iterator"))
            .unwrap_or(false);
        let ret = info
            .and_then(|ci| ci.methods.get("getIterator"))
            .map(|sig| sig.return_type.clone());
        (direct, ret)
    };
    let iter_class: String = if implements_iterator {
        class_name.to_string()
    } else {
        // IteratorAggregate path: dispatch getIterator() on the receiver
        // (x0), replacing x0 with the returned Iterator pointer.
        emit_dispatch_instance_method(class_name, "getIterator", emitter, ctx);
        match get_iterator_return {
            Some(PhpType::Object(name)) => name,
            _ => class_name.to_string(),
        }
    };
    let class_name = iter_class.as_str();

    emitter.instruction("str x0, [sp, #-16]!");                                 // park iterator receiver pointer in a 16-byte stack slot

    emitter.instruction("ldr x0, [sp]");                                        // reload receiver into x0 for rewind() dispatch
    emit_dispatch_instance_method(class_name, "rewind", emitter, ctx);

    emitter.label(loop_start);

    emitter.instruction("ldr x0, [sp]");                                        // reload receiver into x0 for valid() dispatch
    emit_dispatch_instance_method(class_name, "valid", emitter, ctx);
    emitter.instruction("cmp x0, #0");                                          // valid() returned 0 -> end of iteration
    emitter.instruction(&format!("b.eq {}", loop_end));                         // exit foreach when valid() returns false

    if let Some(kv) = key_var {
        emitter.instruction("ldr x0, [sp]");                                    // reload receiver into x0 for key() dispatch
        emit_dispatch_instance_method(class_name, "key", emitter, ctx);
        if let Some(kvar) = ctx.variables.get(kv) {
            let k_offset = kvar.stack_offset;
            crate::codegen::abi::store_at_offset_scratch(emitter, "x0", k_offset, "x10");
            ctx.update_var_type_and_ownership(
                kv,
                PhpType::Mixed,
                HeapOwnership::borrowed_alias_for_type(&PhpType::Mixed),
            );
        } else {
            emitter.comment(&format!("WARNING: undefined foreach key variable ${}", kv));
        }
    }

    emitter.instruction("ldr x0, [sp]");                                        // reload receiver into x0 for current() dispatch
    emit_dispatch_instance_method(class_name, "current", emitter, ctx);
    if let Some(vvar) = ctx.variables.get(value_var) {
        let v_offset = vvar.stack_offset;
        crate::codegen::abi::store_at_offset_scratch(emitter, "x0", v_offset, "x10");
        ctx.update_var_type_and_ownership(
            value_var,
            PhpType::Mixed,
            HeapOwnership::borrowed_alias_for_type(&PhpType::Mixed),
        );
    } else {
        emitter.comment(&format!("WARNING: undefined foreach value variable ${}", value_var));
    }

    ctx.loop_stack.push(LoopLabels {
        continue_label: loop_cont.to_string(),
        break_label: loop_end.to_string(),
        sp_adjust: 16,
    });
    for s in body {
        emit_stmt(s, emitter, ctx, data);
    }
    ctx.loop_stack.pop();

    emitter.label(loop_cont);
    emitter.instruction("ldr x0, [sp]");                                        // reload receiver into x0 for next() dispatch
    emit_dispatch_instance_method(class_name, "next", emitter, ctx);
    emitter.instruction(&format!("b {}", loop_start));                          // continue the iteration

    emitter.label(loop_end);
    emitter.instruction("add sp, sp, #16");                                     // discard the parked receiver slot
}
