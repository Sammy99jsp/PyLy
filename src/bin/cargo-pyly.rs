#![feature(rustc_private)]

fn main() {
  rustc_plugin::cli_main(::pyly::SveltePlugin);
}