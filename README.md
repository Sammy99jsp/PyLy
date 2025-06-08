# PyLy: Type Stub Generation for Rust

An experimental Rustc plugin that attempts to generate type stubs for:
- [ ] Python 🚧 *(in progress)*
- [ ] TypeScript

This is different from a traditional proc-macro approach,
which is limited by only being AST-based.

## Try it out!

You'll need a specific nightly compiler toolchain, specified in the `rust-toolchain.toml` &mdash; cargo should automatically fetch this for you. 

```bash
# After cloning...
# Install the pyly binaries
cargo install --path . 

# Run the binaries on an example crate
cd examples/test-crate
cargo pyly
```

You should see the output:

```py
tuple[None, int, float, bool, str, tuple[list[int], set[int], dict[str, int]]]
```

## How ???

This is comprised of three parts:
### 1. Core Library

We make use of two key traits:

* `pyly_lib::Language` is implemented for each target language (e.g. `Python: Language`).

    Each Language `L` has an associated type `L::Type`, which is the Rust type used to encode language `L`'s data types.
<br/>
* `T: Expose<L>` where `L: Language` signifies that type `T` is exposed (should be included)
in the output stubs for language `L`.

  &nbsp;
  As an example, `Expose<Python>` is implemented for many common types:

| Rust types | Python Type |
|-----------|-------------|
| `bool` | `bool` |
| `u8` - `u128`, `usize`, `i8` - `i128`, `isize`| `int` |
| `f32` `f64` | `float` |
|`char` `&str` `String` `Box<str>` `Rc<str>` `Arc<str>`| `str` |
| `()` | `None` |
| `(T1,)` &mdash; tuples up to 12 items | `tuple[T1,]` - &hellip; |
| 🚧 (Planned) `Option<T>` | `typing.Option<T>` |

  Every `T: Expose<L>` has a constant `T::AS: L::Type` which is the type representation for `T` in language `L`. This can be be used at compile-time (as `T::AS` is `const` [^1]).

### 2. Helper Macros

🚧 We do plan to add helper proc-macros (`pyly_macros::expose`) to help users implement the appropriate traits (`pyly_lib::Expose<L>`) themselves.

### 3. Rustc Plugin

The Rustc plugin was made using [rustc_plugin](https://github.com/cognitive-engineering-lab/rustc_plugin/) from Cognitive Engineering, and based on their example code.

Rustc plugins have numerous benefits over proc-macros:
* They have full access to the type system, which makes accessing fields, evaluating `const` values possible.
* We can look at an entire crate at once, collecting data from multiple items (types, traits).

This can allow us to eventually add advanced features, such as generics, and further convert between Rust and other languages' type systems.

### Currently...
Currently, we re-use the `const`-based system exposed via the `pyly_lib` traits, and evaluate the `<_ as pyly_lib::Exposed<_>>::AS` constants.
We managed to then extract this information back into the actual `pyly_lib` type (inside the compiler plugin).