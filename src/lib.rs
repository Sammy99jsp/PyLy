//! A Rustc plugin that evaluates the `const` items of [pyly_lib::Exposed::AS].

#![feature(
    rustc_private,
    box_patterns,
    slice_as_array,
    once_cell_try,
    try_blocks,
    closure_lifetime_binder,
    maybe_uninit_as_bytes,
    discriminant_kind,
    box_vec_non_null,
    associated_type_defaults,
    ptr_metadata,
    offset_of_enum
)]

pub mod pyly;

extern crate either;
extern crate rustc_abi;
extern crate rustc_const_eval;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_smir;
extern crate rustc_span;
extern crate rustc_type_ir;
extern crate stable_mir;

use std::{borrow::Cow, env, process::Command};

use clap::Parser;

use rustc_middle::ty::TyCtxt;
use rustc_plugin::{CrateFilter, RustcPlugin, RustcPluginArgs, Utf8Path};
use serde::{Deserialize, Serialize};

use crate::pyly::{
    comptime::{self, exposed::PyLyCtx},
    PyLy,
};

// This struct is the plugin provided to the rustc_plugin framework,
// and it must be exported for use by the CLI/driver binaries.
pub struct SveltePlugin;

// To parse CLI arguments, we use Clap for this example. But that
// detail is up to you.
#[derive(Parser, Serialize, Deserialize)]
pub struct PyLyPluginArgs {
    #[arg(short, long)]
    allcaps: bool,

    #[clap(last = true)]
    cargo_args: Vec<String>,
}

impl RustcPlugin for SveltePlugin {
    type Args = PyLyPluginArgs;

    fn version(&self) -> Cow<'static, str> {
        env!("CARGO_PKG_VERSION").into()
    }

    fn driver_name(&self) -> Cow<'static, str> {
        "pyly-driver".into()
    }

    // In the CLI, we ask Clap to parse arguments and also specify a CrateFilter.
    // If one of the CLI arguments was a specific file to analyze, then you
    // could provide a different filter.
    fn args(&self, _target_dir: &Utf8Path) -> RustcPluginArgs<Self::Args> {
        let args = PyLyPluginArgs::parse_from(env::args().skip(1));
        let filter = CrateFilter::AllCrates;
        RustcPluginArgs { args, filter }
    }

    // Pass Cargo arguments (like --feature) from the top-level CLI to Cargo.
    fn modify_cargo(&self, cargo: &mut Command, args: &Self::Args) {
        cargo.args(&args.cargo_args);
    }

    // In the driver, we use the Rustc API to start a compiler session
    // for the arguments given to us by rustc_plugin.
    fn run(
        self,
        compiler_args: Vec<String>,
        _plugin_args: Self::Args,
    ) -> rustc_interface::interface::Result<()> {
        let mut callbacks = PyLyCallback {};
        rustc_driver::run_compiler(&compiler_args, &mut callbacks);
        Ok(())
    }
}

struct PyLyCallback {}

impl rustc_driver::Callbacks for PyLyCallback {
    // At the top-level, the Rustc API uses an event-based interface for
    // accessing the compiler at different stages of compilation. In this callback,
    // all the type-checking has completed.
    fn after_analysis(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'_>,
    ) -> rustc_driver::Compilation {
        let pyly = PyLy::new(tcx).expect("PyLy library present");

        let py_ctx = &mut PyLyCtx::new();
        tcx.all_impls(pyly.traits.exposed)
            .filter(|did| did.krate != pyly.krate)
            .for_each(|impl_| {
                let ty = comptime::exposed::AS(tcx, &pyly, py_ctx, impl_);
                println!("{}", ty.as_str());
            });

        // let impls_exposed =
        // exposed_trait.skip_binder();
        //     tcx.def_path_table().def_key(index).enumerated_keys_and_path_hashes().filter(|(a, b, c)| {
        //         b.
        //     })
        // let hir = tcx.def_path_str(def_kind, def_id)

        rustc_driver::Compilation::Continue
    }
}
