//! The catalogues live in `stagecraft_core::catalog` (load sums need them there); this
//! re-exports them for views.

pub use stagecraft_core::catalog::{
    PlatformSystem, SupportPreset, TrussProfile, platform_system, platform_systems, support, supports, truss_profile,
    truss_profile_or_fallback, truss_profiles,
};
