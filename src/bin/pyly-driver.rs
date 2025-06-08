#![feature(rustc_private)]

fn main() {
    rustc_plugin::driver_main(::pyly::SveltePlugin);
}
