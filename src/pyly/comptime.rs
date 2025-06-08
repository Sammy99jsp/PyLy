//!
//! Utilities for evaluating the `const` value of [pyly_lib::Exposed::AS].
//!

pub mod exposed {
    use std::{
        marker::DiscriminantKind,
        mem::{self, MaybeUninit},
        ptr::{self, NonNull},
    };

    use pyly_lib::python;
    use rustc_const_eval::{
        const_eval::{mk_eval_cx_for_const_val, CompileTimeMachine},
        interpret::{InterpCx, InterpResult, OpTy, Projectable},
    };
    use rustc_hir::{def::Namespace, def_id::DefId};
    use rustc_middle::ty::{TyCtxt, TyKind, TypingEnv};
    use rustc_span::Ident;

    use crate::pyly::PyLy;

    type Icx<'tcx> = InterpCx<'tcx, CompileTimeMachine<'tcx>>;

    fn read_disc_then_first_field<'a, D: DiscriminantKind>(
        icx: &Icx<'a>,
        op: OpTy<'a>,
    ) -> (D::Discriminant, Option<OpTy<'a>>)
    where
        D::Discriminant: TryFrom<usize>,
        <D::Discriminant as TryFrom<usize>>::Error: std::fmt::Debug,
    {
        let variant = icx.read_discriminant(&op).unwrap();
        let down = icx.project_downcast(&op, variant).unwrap();
        let variant: D::Discriminant = TryFrom::try_from(variant.as_usize()).unwrap();

        if down.layout.fields.count() == 0 {
            return (variant, None);
        }

        let offset = down.layout.fields.offset(0);
        let layout = down.layout.field(icx, 0);
        let field = op.offset(offset, layout, icx).unwrap();
        (variant, Some(field))
    }

    fn read_field<'a>(icx: &Icx<'a>, op: &OpTy<'a>, i: usize) -> InterpResult<'a, OpTy<'a>> {
        let offset = op.layout.fields.offset(i);
        let layout = op.layout.field(icx, i);
        op.offset(offset, layout, icx)
    }

    #[derive(Debug, PartialEq, Eq)]
    pub enum StoredType {
        Single(NonNull<python::Type<'static>>),
        Dual(NonNull<[python::Type<'static>; 2]>),
        Multiple(NonNull<[python::Type<'static>]>),
    }

    impl StoredType {
        fn extract<Ty: FromStoredType + ?Sized>(this: &StoredType) -> &'static Ty {
            Ty::from_stored_type(this)
        }
    }

    trait IntoStoredType {
        const FN: fn(NonNull<Self>) -> StoredType;

        fn into_stored_type(this: Box<Self>) -> StoredType {
            Self::FN(Box::into_non_null(this))
        }
    }

    impl IntoStoredType for python::Type<'static> {
        const FN: fn(NonNull<Self>) -> StoredType = StoredType::Single;
    }

    impl IntoStoredType for [python::Type<'static>; 2] {
        const FN: fn(NonNull<Self>) -> StoredType = StoredType::Dual;
    }

    impl IntoStoredType for [python::Type<'static>] {
        const FN: fn(NonNull<Self>) -> StoredType = StoredType::Multiple;
    }

    trait FromStoredType: IntoStoredType {
        fn from_stored_type(ty: &StoredType) -> &'static Self;
    }

    macro_rules! impl_from_stored_type {
        ($ty: ty, $var: ident) => {
            impl FromStoredType for $ty {
                fn from_stored_type(ty: &StoredType) -> &'static Self {
                    match ty {
                        StoredType::$var(ptr) => unsafe { ptr.as_ptr().as_ref().unwrap() },
                        _ => unimplemented!(),
                    }
                }
            }
        };
    }

    impl_from_stored_type!(python::Type<'static>, Single);
    impl_from_stored_type!([python::Type<'static>; 2], Dual);
    impl_from_stored_type!([python::Type<'static>], Multiple);

    trait PossibleRef {
        type Inner<T>: Sized
        where
            T: 'static;

        fn intern<T: FromStoredType + 'static>(ctx: &mut PyLyCtx, type_: T) -> Self::Inner<T>;
    }

    struct ByRef;

    impl PossibleRef for ByRef {
        type Inner<T>
            = &'static T
        where
            T: 'static;

        fn intern<T: FromStoredType + 'static>(ctx: &mut PyLyCtx, type_: T) -> Self::Inner<T> {
            ctx.intern_type(Box::new(type_))
        }
    }

    struct ByValue;

    impl PossibleRef for ByValue {
        type Inner<T>
            = T
        where
            T: 'static;

        fn intern<T: FromStoredType + 'static>(_: &mut PyLyCtx, type_: T) -> Self::Inner<T> {
            type_
        }
    }

    impl Drop for StoredType {
        fn drop(&mut self) {
            unsafe {
                match self {
                    StoredType::Single(ptr) => drop(Box::from_non_null(*ptr)),
                    StoredType::Dual(ptr) => drop(Box::from_non_null(*ptr)),
                    StoredType::Multiple(ptr) => drop(Box::from_non_null(*ptr)),
                };
            }
        }
    }

    pub struct PyLyCtx {
        stored_types: Vec<StoredType>,
    }

    impl Default for PyLyCtx {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PyLyCtx {
        pub const fn new() -> Self {
            Self {
                stored_types: Vec::new(),
            }
        }
    }

    impl PyLyCtx {
        fn intern_type<Ty: FromStoredType + ?Sized>(&mut self, ty: Box<Ty>) -> &'static Ty {
            let desired = Ty::into_stored_type(ty);
            if let Some(ty) = self.stored_types.iter().find(|&ty| &desired == ty) {
                return StoredType::extract(ty);
            }

            self.stored_types.push(desired);
            Ty::from_stored_type(self.stored_types.last().unwrap())
        }

        fn read_py_in_built<'tcx>(
            &mut self,
            tcx: TyCtxt<'tcx>,
            icx: &Icx<'tcx>,
            pyly: &PyLy,
            op: OpTy<'tcx>,
        ) -> python::InBuilt<'static> {
            let mut uninit = MaybeUninit::<python::InBuilt<'static>>::uninit();
            let (var, op) = read_disc_then_first_field::<python::InBuilt<'static>>(icx, op);

            // TODO: Refactor this such that only 0..=8 use
            //       the discriminant trick.

            // 1. Write the discriminant.
            // SAFETY: We are aligned to a usize
            //         by the layout of InBuilt<'_>.
            unsafe {
                (uninit.as_mut_ptr() as *mut usize).write(var as usize);
            }

            match (var, op) {
                // None, Ellipses, Int, Float, Complex, Bool, Str, Bytes, ByteArray
                (0..=8, None) => (), // Do nothing...

                // Tuple
                (9, Some(ref op)) => {
                    // Get the individual layout of one.
                    let typing_env = TypingEnv::fully_monomorphized();
                    let layout =
                        if let rustc_type_ir::TyKind::Ref(_, ty, _) = op.layout.ty.kind() {
                            if let rustc_type_ir::TyKind::Slice(ty) = ty.kind() {
                                tcx.layout_of(typing_env.as_query_input(*ty))
                            } else {
                                unreachable!()
                            }
                        } else {
                            unreachable!("Should only be a reference type.")
                        }
                        .unwrap();

                    let ptr = read_field(icx, op, 0)
                        .and_then(|ref op| icx.read_pointer(op))
                        .unwrap();

                    let len = read_field(icx, op, 1)
                        .and_then(|ref op| icx.read_target_usize(op))
                        .unwrap();

                    let tys = (0..len)
                        .map(|i| {
                            let offset = layout.size.checked_mul(i, &tcx).unwrap();
                            let op = icx
                                .ptr_to_mplace(ptr.wrapping_offset(offset, &tcx), layout)
                                .to_op(icx)
                                .unwrap();

                            self.read_py_type(tcx, icx, pyly, op, ByValue)
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();

                    let inner = self.intern_type(tys);
                    let ptr = unsafe {
                        let ptr = uninit
                            .as_mut_ptr()
                            .byte_add(mem::offset_of!(python::InBuilt, Tuple.0));

                        ptr::from_raw_parts_mut::<&[python::Type<'static>]>(ptr, ())
                    };
                    unsafe { ptr.write(inner) };
                }

                // List, Set, Dict
                (10..=12, Some(op)) => {
                    let ptr = icx.read_pointer(&op).unwrap();

                    let typing_env = TypingEnv::fully_monomorphized();
                    let layout = match op.layout.ty.kind() {
                        rustc_type_ir::TyKind::Ref(_, ty, _) => {
                            tcx.layout_of(typing_env.as_query_input(*ty))
                        }
                        _ => unreachable!("Should only be a reference type."),
                    }
                    .unwrap();

                    // Read the pointer:
                    // &Type<'_> | &[Type<'_>; 2]
                    let op = icx.ptr_to_mplace(ptr, layout).to_op(icx).unwrap();
                    match op.layout.ty.kind() {
                        // Dict => [Type<'_>; 2]
                        TyKind::Array(ty, len) => {
                            let len = len.try_to_target_usize(tcx).unwrap();
                            let layout = tcx.layout_of(typing_env.as_query_input(*ty)).unwrap();

                            assert_eq!(len, 2);
                            assert_eq!(ty.ty_adt_def().map(|a| a.did()), Some(pyly.py.type_));

                            let inner: [_; 2] = (0..len)
                                .map(|i| {
                                    let offset = layout.size.checked_mul(i, &tcx).unwrap();
                                    let op = icx
                                        .ptr_to_mplace(ptr.wrapping_offset(offset, &tcx), layout)
                                        .to_op(icx)
                                        .unwrap();

                                    self.read_py_type(tcx, icx, pyly, op, ByValue)
                                })
                                .collect::<Vec<_>>()
                                .try_into()
                                .unwrap();

                            let inner = self.intern_type(Box::new(inner));
                            let ptr = unsafe {
                                uninit
                                    .as_mut_ptr()
                                    .byte_add(mem::offset_of!(python::InBuilt, Dict.0))
                                    as *mut &'static [python::Type<'static>; 2]
                            };

                            unsafe {
                                ptr.write(inner);
                            }
                        }
                        TyKind::Adt(adt, _) => {
                            assert_eq!(adt.did(), pyly.py.type_);

                            let inner = self.read_py_type(tcx, icx, pyly, op, ByValue);
                            let inner = self.intern_type(Box::new(inner));

                            let ptr = unsafe {
                                // Should be same offset with List, Set...
                                assert_eq!(
                                    mem::offset_of!(python::InBuilt, List.0),
                                    mem::offset_of!(python::InBuilt, Set.0)
                                );

                                uninit
                                    .as_mut_ptr()
                                    .byte_add(mem::offset_of!(python::InBuilt, List.0))
                                    as *mut &'static python::Type<'static>
                            };

                            unsafe {
                                ptr.write(inner);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!("No other variants!"),
            }

            unsafe { uninit.assume_init() }
        }

        #[allow(unused)]
        fn read_py_typing<'tcx>(
            &mut self,
            tcx: TyCtxt<'tcx>,
            icx: &Icx<'tcx>,
            pyly: &PyLy,
            op: OpTy<'tcx>,
        ) -> python::Typing<'static> {
            todo!("{op:#?}")
        }

        fn read_py_type<'tcx, P: PossibleRef>(
            &mut self,
            tcx: TyCtxt<'tcx>,
            icx: &Icx<'tcx>,
            pyly: &PyLy,
            op: OpTy<'tcx>,
            _: P,
        ) -> P::Inner<python::Type<'static>> {
            // 1. Read discriminant.
            let (var, field) = read_disc_then_first_field::<python::Type>(icx, op);

            // Variant in source-order, so this is fine:
            let type_ = match (var, field) {
                // BuiltIn
                (0, Some(op)) => {
                    let in_built = self.read_py_in_built(tcx, icx, pyly, op);
                    python::Type::InBuilt(in_built)
                }
                // Typing
                (1, Some(op)) => {
                    let typing = self.read_py_typing(tcx, icx, pyly, op);
                    python::Type::Typing(typing)
                }
                // Custom
                (2, None) => python::Type::Custom,

                // No other possibilities.
                _ => unreachable!("Only 3 variants (for now!)"),
            };

            P::intern(self, type_)
        }
    }

    #[allow(non_snake_case)]
    pub fn AS<'a>(
        tcx: TyCtxt,
        pyly: &PyLy,
        ctx: &'a mut PyLyCtx,
        impl_: DefId,
    ) -> &'a python::Type<'a> {
        let assoc_items = tcx.associated_items(impl_);
        let as_const = assoc_items
            .find_by_name_and_namespace(
                tcx,
                Ident::from_str(crate::pyly::traits::Exposed::AS),
                Namespace::ValueNS,
                impl_,
            )
            .expect("find <_ as pyly_lib::Expose<_>>::AS");

        let ty_env = TypingEnv::post_analysis(tcx, impl_);

        let as_ty = tcx
            .normalize_erasing_regions(ty_env, tcx.type_of(as_const.def_id).instantiate_identity());

        let as_val = tcx
            .const_eval_poly(as_const.def_id)
            .expect("Be able to evaluate AS constant!");

        let (icx, op) =
            mk_eval_cx_for_const_val(tcx.at(tcx.def_span(as_const.def_id)), ty_env, as_val, as_ty)
                .unwrap();

        ctx.read_py_type(tcx, &icx, pyly, op, ByRef)
    }
}
