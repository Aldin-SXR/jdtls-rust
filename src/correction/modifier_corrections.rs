//! Port of `ModifierCorrectionSubProcessor(Core)`.
mod change;
mod invalid;
mod methods;
mod override_annotation;
mod permitted;
mod sealed;
mod split;
mod visibility;

pub use invalid::remove_invalid_modifiers;
pub use methods::{abstract_method, abstract_type, native_method, requires_body};
pub use override_annotation::{overriding_deprecated_method, remove_override_annotation};
pub use permitted::permitted_types;
pub use sealed::{sealed_as_direct_super_type, sealed_missing_modifier, type_as_permitted_sub_type};
pub use visibility::{add_method_modifier, change_overridden_modifier, make_final, non_accessible_reference};
pub(crate) use split::rewrite_field_modifiers;
pub(crate) use visibility::Units;
pub(crate) use change::find_declaring_node;

pub const TO_STATIC: i32 = 1;
pub const TO_VISIBLE: i32 = 2;
pub const TO_NON_PRIVATE: i32 = 3;
pub const TO_NON_STATIC: i32 = 4;
pub const TO_NON_FINAL: i32 = 5;
