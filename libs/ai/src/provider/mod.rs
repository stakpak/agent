//! Provider trait and dispatcher

mod dispatcher;
mod trait_def;

pub use dispatcher::{ProviderDispatcher, ProviderKind};
pub use trait_def::Provider;
