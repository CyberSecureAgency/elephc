//! Purpose:
//! Walks EIR basic blocks in function order and delegates instruction/terminator lowering.
//! Owns function setup for the initial Phase 04 backend path.
//!
//! Called from:
//! - `crate::codegen_ir::generate_user_asm_from_ir()`.
//!
//! Key details:
//! - This first backend increment supports straight-line main blocks and reports
//!   explicit unsupported-feature errors for control flow not lowered yet.

use crate::codegen::abi;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::UNINITIALIZED_TYPED_PROPERTY_SENTINEL;
use crate::ir::{BasicBlock, Function, Module};
use crate::names::{function_epilogue_symbol, static_property_symbol};

use super::context::FunctionContext;
use super::frame;
use super::function_variants;
use super::lower_inst;
use super::lower_term;
use super::{CodegenIrError, Result};

/// Emits all supported EIR functions and then the process-entry main function.
pub(super) fn emit_module(
    module: &Module,
    emitter: &mut Emitter,
    data: &mut DataSection,
) -> Result<()> {
    function_variants::emit_dispatchers(module, emitter, data);
    for function in module.functions.iter().filter(|function| !is_main(function)) {
        emit_user_function(module, function, emitter, data)?;
    }
    let main = module
        .functions
        .iter()
        .find(|function| is_main(function))
        .ok_or_else(|| CodegenIrError::invalid_module("EIR module has no main function"))?;
    emit_main_function(module, main, emitter, data)
}

/// Emits a non-main EIR function as a direct-call target.
fn emit_user_function(
    module: &Module,
    function: &Function,
    emitter: &mut Emitter,
    data: &mut DataSection,
) -> Result<()> {
    let layout = frame::layout_for_function(function);
    let epilogue_label = function_epilogue_symbol(&function.name);
    let mut ctx = FunctionContext::new(
        module,
        function,
        emitter,
        data,
        layout,
        false,
        Some(epilogue_label),
    );
    frame::emit_function_prologue(&mut ctx)?;
    emit_blocks(&mut ctx)?;
    frame::emit_function_epilogue(&mut ctx);
    Ok(())
}

/// Emits the EIR main function as the process entry point.
fn emit_main_function(
    module: &Module,
    function: &Function,
    emitter: &mut Emitter,
    data: &mut DataSection,
) -> Result<()> {
    let layout = frame::layout_for_function(function);
    let mut ctx = FunctionContext::new(module, function, emitter, data, layout, true, None);
    frame::emit_main_prologue(&mut ctx);
    emit_static_property_sentinels(&mut ctx);
    emit_blocks(&mut ctx)?;
    if !ctx.epilogue_emitted {
        frame::emit_main_epilogue(&mut ctx);
    }
    Ok(())
}

/// Returns true when a function is the process entry function.
fn is_main(function: &Function) -> bool {
    function.flags.is_main || function.name == "main"
}

/// Marks typed static properties without defaults as uninitialized before user code runs.
fn emit_static_property_sentinels(ctx: &mut FunctionContext<'_>) {
    let mut class_names = super::runtime_referenced_class_names(ctx.module)
        .into_iter()
        .collect::<Vec<_>>();
    class_names.sort();
    for class_name in class_names {
        let Some(class_info) = ctx.module.class_infos.get(&class_name) else {
            continue;
        };
        for (index, (property, _)) in class_info.static_properties.iter().enumerate() {
            let declaring_class = class_info
                .static_property_declaring_classes
                .get(property)
                .map(String::as_str)
                .unwrap_or(class_name.as_str());
            if declaring_class != class_name
                || !class_info.declared_static_properties.contains(property)
                || class_info
                    .static_defaults
                    .get(index)
                    .is_some_and(Option::is_some)
            {
                continue;
            }
            ctx.emitter.comment(&format!(
                "mark static property {}::${} uninitialized",
                class_name, property
            ));
            let marker_reg = abi::int_result_reg(ctx.emitter);
            abi::emit_load_int_immediate(
                ctx.emitter,
                marker_reg,
                UNINITIALIZED_TYPED_PROPERTY_SENTINEL,
            );
            let symbol = static_property_symbol(&class_name, property);
            abi::emit_store_reg_to_symbol(ctx.emitter, marker_reg, &symbol, 8);
        }
    }
}

/// Emits every block in table order.
fn emit_blocks(ctx: &mut FunctionContext<'_>) -> Result<()> {
    let blocks = ctx.function.blocks.clone();
    for block in blocks {
        emit_block(ctx, &block)?;
    }
    Ok(())
}

/// Emits one EIR basic block.
fn emit_block(ctx: &mut FunctionContext<'_>, block: &BasicBlock) -> Result<()> {
    ctx.emitter.label(&ctx.block_label(&block.name, block.id.as_raw()));
    for inst_id in &block.instructions {
        lower_inst::lower_instruction(ctx, *inst_id)?;
    }
    let terminator = block
        .terminator
        .as_ref()
        .ok_or_else(|| CodegenIrError::invalid_module(format!("block '{}' has no terminator", block.name)))?;
    lower_term::lower_terminator(ctx, terminator)
}
