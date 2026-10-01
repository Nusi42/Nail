//! Launch targets: what Windows hands the app when it is started as the
//! handler of a URL scheme or a file type (plan M2b WP44; ADRs 0040, 0041).
//! The parser lives in [`nail_core::launch_args`] (M3a: `nail serve
//! --launch` shares it).

pub use nail_core::launch_args::*;
