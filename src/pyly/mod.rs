//!
//! Utilities for getting the type information for [::pyly_lib].
//!
//! 
pub mod comptime;

use std::collections::{HashMap, HashSet};

use rustc_data_structures::intern::Interned;
use rustc_hir::def_id::{CrateNum, DefId};
use rustc_middle::ty::{self, AdtDef, AdtDefData, Ty, TyCtxt, TyKind, TypingEnv};
use rustc_type_ir::fast_reject::SimplifiedType;

#[allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
mod traits {
    pub const Language: &str = "pyly_lib::Language";

    pub const Exposed: &str = "pyly_lib::Exposed";

    pub mod Exposed {
        pub const AS: &str = "AS";
    }
}

#[allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
mod types {
    pub const Python: &str = "pyly_lib::Python";

    pub mod python {
        pub const InBuilt: &str = "pyly_lib::python::InBuilt";
        pub const Type: &str = "pyly_lib::python::Type";
        pub const Typing: &str = "pyly_lib::python::Typing";
    }
}

type PathDefMap = HashMap<String, DefId>;

/// Common traits and types in [::pyly_lib].
#[derive(Debug)]
pub struct PyLy {
    pub krate: CrateNum,
    pub traits: PyLyTraits,

    pub py: PyLyPy,
}

impl PyLy {
    /// Try to load the library's traits and types.
    pub fn new(tcx: TyCtxt<'_>) -> Option<Self> {
        // Fetch pyly_lib's crate number, if it is used used at all.
        let krate = tcx
            .used_crates(())
            .iter()
            .find(|&&num| tcx.crate_name(num).to_ident_string() == "pyly_lib")
            .copied()?;

        // Collect all relevant traits defined in the library.
        let traits = tcx
            .traits(krate)
            .iter()
            .map(|&did| (tcx.def_path_str(did), did))
            .collect::<PathDefMap>();

        // Populate our cache of PyLy types, because we can't access them directly...
        let mut visitor = Visitor::new(krate);
        traits.values().for_each(|&tr| visitor.visit_trait(tcx, tr));
        let types = &mut visitor.finish(tcx);

        let traits = PyLyTraits::from_map(traits)?;

        let py = PyLyPy::new(types)?;

        Some(Self { krate, traits, py })
    }
}

#[derive(Debug)]
pub struct PyLyTraits {
    /// [pyly_lib::Language]
    pub language: DefId,
    /// [pyly_lib::Exposed]
    pub exposed: DefId,
}

impl PyLyTraits {
    pub fn from_map(mut map: PathDefMap) -> Option<Self> {
        Some(Self {
            language: map.remove(traits::Language)?,
            exposed: map.remove(traits::Exposed)?,
        })
    }
}

/// Contents of [::pyly_lib::python].
#[derive(Debug)]
pub struct PyLyPy {
    /// [pyly_lib::Python]
    pub python: DefId,
    /// [pyly_lib::python::Type]
    pub type_: DefId,
    /// [pyly_lib::python::Typing]
    pub typing: DefId,
    /// [pyly_lib::python::InBuilt]
    pub in_built: DefId,
}

impl PyLyPy {
    pub fn new(types: &mut PathDefMap) -> Option<Self> {
        Some(Self {
            python: types.remove(types::Python)?,
            type_: types.remove(types::python::Type)?,
            typing: types.remove(types::python::Typing)?,
            in_built: types.remove(types::python::InBuilt)?,
        })
    }
}

#[derive(Debug)]
struct Visitor {
    krate: CrateNum,
    adts: HashSet<DefId>,
}

fn simplify_ty<'tcx>(
    tcx: TyCtxt<'tcx>,
    parent: impl Into<Option<DefId>>,
    ty: ty::EarlyBinder<'tcx, Ty<'tcx>>,
) -> Ty<'tcx> {
    let ty_env = match parent.into() {
        Some(parent) => TypingEnv::post_analysis(tcx, parent),
        None => TypingEnv::fully_monomorphized(),
    };

    tcx.normalize_erasing_regions(ty_env, ty.instantiate_identity())
}

impl Visitor {
    fn new(krate: CrateNum) -> Self {
        Self {
            krate,
            adts: Default::default(),
        }
    }

    fn visit_ty<'tcx>(
        &mut self,
        tcx: TyCtxt<'tcx>,
        parent: impl Into<Option<DefId>>,
        ty: Ty<'tcx>,
    ) {
        let parent = parent.into();
        if let TyKind::Adt(adt @ AdtDef(Interned(AdtDefData { did, .. }, ..)), gen) = ty.kind() {
            if self.adts.contains(did) {
                return;
            }

            // Inspect within this type...
            adt.variants()
                .iter()
                .flat_map(|var| var.fields.iter())
                .for_each(|f| {
                    let ty = f.ty(tcx, gen);
                    self.visit_ty(tcx, parent, ty);
                });

            if did.krate == self.krate {
                self.adts.insert(*did);
            }
        }
    }

    fn visit_trait_impl(&mut self, tcx: TyCtxt<'_>, impl_: DefId) {
        let assoc_items = tcx.associated_items(impl_);
        assoc_items
            .in_definition_order()
            .filter_map(|impl_assoc| match impl_assoc.kind {
                rustc_middle::ty::AssocKind::Fn => None,
                rustc_middle::ty::AssocKind::Const => Some(tcx.type_of(impl_assoc.def_id)),
                rustc_middle::ty::AssocKind::Type => Some(tcx.type_of(impl_assoc.def_id)),
            })
            .map(|ty| {
                tcx.normalize_erasing_regions(
                    TypingEnv::post_analysis(tcx, impl_),
                    ty.instantiate_identity(),
                )
            })
            .for_each(|ty| self.visit_ty(tcx, impl_, ty));
    }

    fn visit_trait(&mut self, tcx: TyCtxt<'_>, tr_did: DefId) {
        let tr = tcx.trait_impls_of(tr_did);

        tr.non_blanket_impls().iter().for_each(|(ty, impls)| {
            if let SimplifiedType::Adt(def) = ty {
                let ty = tcx.type_of(def);
                let ty = simplify_ty(tcx, tr_did, ty);
                self.visit_ty(tcx, None, ty);
            }

            impls
                .iter()
                .for_each(|&impl_| self.visit_trait_impl(tcx, impl_));

            // self.visit_ty(tcx, ty.);
        });

        tr.blanket_impls().iter().for_each(|&impl_| {
            self.visit_trait_impl(tcx, impl_);
        });
    }

    fn finish(self, tcx: TyCtxt<'_>) -> PathDefMap {
        self.adts
            .into_iter()
            .map(|did| (tcx.def_path_str(did), did))
            .collect()
    }
}
