use std::mem::ManuallyDrop;

use crate::component::{Component, ComponentId, Components};

/// A group of components inserted together. Implemented for tuples of up to 12 components.
/// For a single component use a 1-tuple `(c,)` or [`World::insert_one`](crate::World::insert_one).
pub trait Bundle: Send + Sync + 'static {
    /// Registers component types and appends their ids in bundle order.
    fn register(components: &mut Components, out: &mut Vec<ComponentId>);
    /// Hands each component (in bundle order) to `f` as a pointer; `f` takes ownership by bitwise copy.
    fn take(self, f: &mut dyn FnMut(usize, *mut u8));
}

impl Bundle for () {
    fn register(_: &mut Components, _: &mut Vec<ComponentId>) {}
    fn take(self, _: &mut dyn FnMut(usize, *mut u8)) {}
}

macro_rules! impl_bundle {
    ($($n:ident $i:tt),+) => {
        impl<$($n: Component),+> Bundle for ($($n,)+) {
            fn register(components: &mut Components, out: &mut Vec<ComponentId>) {
                $( out.push(components.register::<$n>()); )+
            }
            #[allow(non_snake_case)]
            fn take(self, f: &mut dyn FnMut(usize, *mut u8)) {
                let ($(mut $n,)+) = ($(ManuallyDrop::new(self.$i),)+);
                $( f($i, (&mut *$n) as *mut $n as *mut u8); )+
            }
        }
    };
}

impl_bundle!(A 0);
impl_bundle!(A 0, B 1);
impl_bundle!(A 0, B 1, C 2);
impl_bundle!(A 0, B 1, C 2, D 3);
impl_bundle!(A 0, B 1, C 2, D 3, E 4);
impl_bundle!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_bundle!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_bundle!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
impl_bundle!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
impl_bundle!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
impl_bundle!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
impl_bundle!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);
