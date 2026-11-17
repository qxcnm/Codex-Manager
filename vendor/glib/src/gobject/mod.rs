// Take a look at the license at the top of the repository in the LICENSE file.

// rustdoc-stripper-ignore-next
//! GObject bindings

#[allow(unused_imports)]
mod auto;
mod binding;
#[cfg(feature = "v2_72")]
#[cfg_attr(docsrs, doc(cfg(feature = "v2_72")))]
mod binding_group;
mod flags;
#[cfg(feature = "v2_74")]
#[cfg_attr(docsrs, doc(cfg(feature = "v2_74")))]
mod signal_group;

#[cfg(feature = "v2_72")]
#[cfg_attr(docsrs, doc(cfg(feature = "v2_72")))]
pub use binding_group::BindingGroupBuilder;

pub use self::{auto::*, flags::*};
//pub use self::auto::functions::*;
