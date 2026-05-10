mod enums;
mod fiber;
mod interface;
mod method;
mod prep;
mod static_call;
mod vtable;

pub(crate) use interface::emit_dispatch_interface_method;
pub(crate) use vtable::emit_dispatch_instance_method;
pub(super) use method::{
    emit_method_call, emit_method_call_with_pushed_args,
    emit_method_call_with_saved_receiver_below_args, emit_pushed_method_args,
};
pub(super) use static_call::{
    emit_forwarded_called_class_id, emit_immediate_class_id, emit_static_method_call,
};
