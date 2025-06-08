#![allow(internal_features)]
#![feature(core_intrinsics, const_eval_select, const_copy_from_slice, const_heap)]

#[cfg(feature = "macros")]
pub use pyly_macros::expose;

#[doc(hidden)]
mod __private {
    #[doc(hidden)]
    pub trait _Private {}
}

pub trait Language: __private::_Private {
    type Type: Default;
}

pub trait Exposed<L: Language> {
    const AS: L::Type;
}

macro_rules! impl_lang_for {
    ($lang: ty, [$($target: ty => $as: expr),*]) => {
        $(
            impl $crate::Exposed<$lang> for $target {
                const AS: <$lang as $crate::Language>::Type = $as;
            }
        )*
    };
    ($lang: ty, [$($target: ty),*] => $as: expr) => {
        $(
            impl $crate::Exposed<$lang> for $target {
                const AS: <$lang as $crate::Language>::Type = $as;
            }
        )*
    };
}

pub struct Python;
impl __private::_Private for Python {}
impl Language for Python {
    type Type = python::Type<'static>;
}
#[allow(non_camel_case_types)]
pub mod python {
    macro_rules! impl_tuple {
        ($($ti: ident),*) => {
            #[doc = "This trait is implemented for tuples up to twelve items long."]
            impl<$($ti : crate::Exposed<Py>),*,> crate::Exposed<Py> for ($($ti),*, ) {
                const AS: <Py as $crate::Language>::Type = InBuilt(Tuple(&[$($ti ::AS),*]));
            }
        }
    }

    #[repr(C, u8)]
    #[derive(Debug, PartialEq, Eq)]
    pub enum InBuilt<'a> {
        None,
        Ellipses,
        Int,
        Float,
        Complex,
        Bool,
        Str,
        Bytes,
        ByteArray,
        Tuple(&'a [Type<'a>]),
        List(&'a Type<'a>),
        Set(&'a Type<'a>),
        Dict(&'a [Type<'a>; 2]),
    }

    /// THIS LEAKS MEMORY!
    const fn generic_format(start: &str, items: &[Type<'_>]) -> &'static str {
        const unsafe fn slice_mut<T>(slice: &mut [T], start: usize, end: usize) -> &mut [T] {
            if start > slice.len() || end > slice.len() {
                panic!("Out of bounds!");
            }

            let ptr = slice.as_mut_ptr();
            let ptr = unsafe { ptr.add(start) };
            unsafe { core::slice::from_raw_parts_mut(ptr, end - start) }
        }

        const unsafe fn copy_into_slice_at<T: Copy>(
            dest: &mut [T],
            start: usize,
            src: &[T],
        ) -> usize {
            let end = start + src.len();
            let subslice = unsafe { slice_mut(dest, start, end) };
            subslice.copy_from_slice(src);
            end
        }

        const fn compiletime(start: &str, items: &[Type<'_>]) -> &'static str {
            let mut sum_type_lengths = 0;
            {
                let mut i = 0;
                loop {
                    if i >= items.len() {
                        break;
                    }

                    sum_type_lengths += items[i].as_str().len();
                    i += 1;
                }
            }

            let length = start.len()
                + "[".len()
                + sum_type_lengths
                + (", ".len() * (items.len().saturating_sub(1)))
                + "]".len();

            let output = unsafe {
                core::slice::from_raw_parts_mut(std::intrinsics::const_allocate(length, 1), length)
            };

            let mut output_i = 0;
            output_i = unsafe { copy_into_slice_at(output, output_i, start.as_bytes()) };
            output_i = unsafe { copy_into_slice_at(output, output_i, b"[") };

            let mut item_i = 0;
            loop {
                if item_i >= items.len() {
                    break;
                }

                output_i = unsafe {
                    copy_into_slice_at(output, output_i, items[item_i].as_str().as_bytes())
                };

                // Not last -- add the nice comma!
                if item_i < (items.len() - 1) {
                    output_i = unsafe { copy_into_slice_at(output, output_i, b", ") };
                }

                item_i += 1;
            }

            output_i = unsafe { copy_into_slice_at(output, output_i, b"]") };

            assert!(output.len() == output_i);
            unsafe { str::from_utf8_unchecked_mut(output) }
        }

        fn runtime(start: &str, items: &[Type<'_>]) -> &'static str {
            let mut s = String::new();
            write!(s, "{start}[").unwrap();

            for (i, item) in items.iter().enumerate() {
                write!(s, "{}", item.as_str()).unwrap();

                if i < (items.len() - 1) {
                    write!(s, ", ").unwrap();
                }
            }

            write!(s, "]").unwrap();

            // BUG: FIX THIS INTENTIONAL MEMORY LEAK AT RUNTIME!
            Box::leak(s.into_boxed_str())
        }

        std::intrinsics::const_eval_select((start, items), compiletime, runtime)
    }

    impl InBuilt<'_> {
        pub const fn as_str(&self) -> &'static str {
            match self {
                Ellipses => "...",
                None => "None",
                Int => "int",
                Float => "float",
                Complex => "complex",
                Bool => "bool",
                Str => "str",
                Bytes => "bytes",
                ByteArray => "bytearray",
                Tuple(items) => generic_format("tuple", items),
                List(t) => generic_format("list", core::slice::from_ref(t)),
                Set(t) => generic_format("set", core::slice::from_ref(t)),
                Dict(kv) => generic_format("dict", kv.as_slice()),
            }
        }
    }

    #[repr(C, u8)]
    #[derive(Debug, PartialEq, Eq)]
    pub enum Typing<'a> {
        Iterator(&'a Type<'a>),
    }

    impl Typing<'_> {
        pub const fn as_str(&self) -> &'static str {
            match self {
                Typing::Iterator(t) => generic_format("typing.Iterator", core::slice::from_ref(t)),
            }
        }
    }

    #[repr(C, u8)]
    #[derive(Debug, Default, PartialEq, Eq)]
    pub enum Type<'a> {
        InBuilt(InBuilt<'a>),
        Typing(Typing<'a>),

        #[default]
        Custom,
    }

    impl Type<'_> {
        pub const fn as_str(&self) -> &'static str {
            match self {
                Type::InBuilt(in_built) => in_built.as_str(),
                Type::Typing(typing) => typing.as_str(),
                Custom => "typing.Any",
            }
        }
    }

    use std::{
        collections::{BTreeMap, BTreeSet, HashMap, HashSet},
        fmt::Write,
    };

    use crate::Exposed;

    use self::{InBuilt::*, Type::*};
    use super::Python as Py;

    impl_lang_for!(Py, [u8, u16, u32, u64, u128, usize] => InBuilt(Int));
    impl_lang_for!(Py, [i8, i16, i32, i64, i128, isize] => InBuilt(Int));

    impl_lang_for!(Py, [f32, f64] => InBuilt(Float));

    impl_lang_for!(Py, [bool] => InBuilt(Bool));

    impl_lang_for!(Py, [char, &str, String, std::borrow::Cow<'_, str>, std::rc::Rc<str>, std::sync::Arc<str>, Box<str>] => InBuilt(Str));

    // Tuple-likes
    impl_lang_for!(Py, [()] => InBuilt(None));
    impl_tuple!(T1);
    impl_tuple!(T1, T2);
    impl_tuple!(T1, T2, T3);
    impl_tuple!(T1, T2, T3, T4);
    impl_tuple!(T1, T2, T3, T4, T5);
    impl_tuple!(T1, T2, T3, T4, T5, T6);
    impl_tuple!(T1, T2, T3, T4, T5, T6, T7);
    impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8);
    impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
    impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
    impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
    impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

    // List-likes
    impl<T: Exposed<Py>> Exposed<Py> for [T] {
        const AS: <Py as crate::Language>::Type = InBuilt(List(&T::AS));
    }

    impl<T: Exposed<Py>> Exposed<Py> for Box<[T]> {
        const AS: <Py as crate::Language>::Type = InBuilt(List(&T::AS));
    }

    impl<T: Exposed<Py>> Exposed<Py> for Vec<T> {
        const AS: <Py as crate::Language>::Type = InBuilt(List(&T::AS));
    }

    // Set-likes
    impl<T: Exposed<Py>> Exposed<Py> for HashSet<T> {
        const AS: <Py as crate::Language>::Type = InBuilt(Set(&T::AS));
    }
    impl<T: Exposed<Py>> Exposed<Py> for BTreeSet<T> {
        const AS: <Py as crate::Language>::Type = InBuilt(Set(&T::AS));
    }

    // Dictionary-Likes
    impl<K: Exposed<Py>, V: Exposed<Py>> Exposed<Py> for HashMap<K, V> {
        const AS: <Py as crate::Language>::Type = InBuilt(Dict(&[K::AS, V::AS]));
    }
    impl<K: Exposed<Py>, V: Exposed<Py>> Exposed<Py> for BTreeMap<K, V> {
        const AS: <Py as crate::Language>::Type = InBuilt(Dict(&[K::AS, V::AS]));
    }

    impl<I: Exposed<Py>> Exposed<Py> for Box<dyn Iterator<Item = I>> {
        const AS: <Py as crate::Language>::Type = Typing(Typing::Iterator(&I::AS));
    }

    #[cfg(test)]
    mod tests {
        use std::{
            collections::{BTreeMap, HashMap, HashSet},
            mem,
        };

        use crate::{
            Exposed, Python,
            python::{self, Type},
        };

        #[test]
        fn compound_types() {
            const A: &str = {
                <Box<
                    dyn Iterator<
                        Item = (
                            u8,
                            u16,
                            f64,
                            (
                                String,
                                &'static str,
                                HashMap<(HashSet<usize>, String), BTreeMap<String, usize>>,
                            ),
                        ),
                    >,
                > as Exposed<Python>>::AS
                    .as_str()
            };

            println!("{A}");
        }

        #[test]
        fn mem_layout() {
            println!("{:?}", unsafe {
                mem::transmute::<Type, [[u8; 8]; 4]>(python::Custom)
            });
        }
    }
}
