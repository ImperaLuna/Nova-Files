use std::any::{Any, type_name};
use std::fmt;
use std::sync::Arc;

/// A value one feature defines, such as its own kind of job or the reply
/// to its own question, which the shared part passes along without
/// knowing what it is. Only code that names the type can open it.
#[derive(Clone)]
pub struct Carried(Arc<dyn Held>);

trait Held: Any + Send + Sync {
    fn same(&self, other: &dyn Any) -> bool;
    fn show(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn name(&self) -> &'static str;
}

impl<T: Any + Send + Sync + fmt::Debug + Eq> Held for T {
    fn same(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }

    fn show(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt(f)
    }

    fn name(&self) -> &'static str {
        type_name::<T>()
    }
}

impl Carried {
    pub fn new<T: Any + Send + Sync + fmt::Debug + Eq>(value: T) -> Self {
        Self(Arc::new(value))
    }

    /// The value, when it is a `T`.
    pub fn get<T: Any>(&self) -> Option<&T> {
        let held: &dyn Any = self.0.as_ref();
        held.downcast_ref()
    }

    /// The name of the value's type, for logs.
    pub fn type_name(&self) -> &'static str {
        self.0.name()
    }
}

impl PartialEq for Carried {
    fn eq(&self, other: &Self) -> bool {
        let other: &dyn Any = other.0.as_ref();
        self.0.same(other)
    }
}

impl Eq for Carried {}

impl fmt::Debug for Carried {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.show(f)
    }
}

#[cfg(test)]
mod tests;
