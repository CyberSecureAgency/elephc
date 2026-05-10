mod collect;
mod dynamic_instanceof;

pub(in crate::codegen) use collect::collect_required_class_names;
pub(in crate::codegen) use dynamic_instanceof::program_has_dynamic_instanceof;
