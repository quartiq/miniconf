# Cast from `dyn Any` to other trait objects

* `no_std` no alloc support
* No proc macros

## Usage

```toml
[dependencies]
crosstrait = "0.1"
```

Then use the `register!{ Type => Trait }` declarative macro and the [`Cast`] traits.

For embedded, the linker needs to be informed of the type registry.

## Example

```rust
use core::any::Any;
use crosstrait::{Cast, Castable, CastableRef, register, REGISTRY};

// Some example traits to play with
use core::{fmt::{Debug, Formatter, Write}, ops::{AddAssign, SubAssign}};

// Add types and trait implementations in the default global registy
// Implementation status is verified at compile time
register!{ i32 => dyn Debug }

// Registering foreign types and traits works fine
// Serialization/deserialization of `dyn Any` is a major use case
// register! { i32 => dyn erased_serde::Serialize }

// Check for trait impl registration on concrete type
assert!(i32::castable::<dyn Debug>());

// Check for trait impl registration on Any
let any: &dyn Any = &42i32;
assert!(any.castable::<dyn Debug>());

// SubAssign<i32> is impl'd for i32 but not registered
assert!(!any.castable::<dyn SubAssign<i32>>());

// Cast ref
let a: &dyn Debug = any.cast().unwrap();
println!("42 = {a:?}");

// Cast mut
let mut value = 5i32;
let any: &mut dyn Any = &mut value;
let v: &mut dyn AddAssign<i32> = any.cast().unwrap();
*v += 3;
assert_eq!(value, 5 + 3);

// Cast Box
let any: Box<dyn Any> = Box::new(0i32);
let _: Box<dyn Debug> = any.cast().unwrap();

// Cast Rc
use std::rc::Rc;
let any: Rc<dyn Any> = Rc::new(0i32);
let _: Rc<dyn Debug> = any.cast().unwrap();

// Cast Arc
use std::sync::Arc;
let any: Arc<dyn Any + Sync + Send> = Arc::new(0i32);
let _: Arc<dyn Debug> = any.cast().unwrap();

// Explicit registry usage
let any: &dyn Any = &0i32;
let _: &dyn Debug = REGISTRY.cast_ref(any).unwrap();

// Autotraits and type/const generics are distinct
let a: Option<&(dyn Debug + Sync)> = any.cast();
assert!(a.is_none());

// Registration can happen anywhere in any order in any downstream crate
register!{ i32 => dyn AddAssign<i32> }

// If a type is not Send + Sync, it can't cast as Arc. `no_arc` accounts for that
register!{ Formatter => dyn Write, no_arc }
```

## Related crates

* [`intertrait`](https://crates.io/crates/intertrait): similar goals, `std`
* [`miniconf`](https://crates.io/crates/miniconf): provides several ways to get `dyn Any` from nodes in
  heterogeneous nested data structures, `no_std`, no alloc
* [`erased_serde`](https://crates.io/crates/erased-serde): `Serialize`/`Serializer`/`Deserializer` trait objects
* [`linkme`](https://crates.io/crates/linkme): linker magic used to build distributed static type registry

## Limitations

### Registry size on `no_std`

Currently the size of the global registry on `no_std` is fixed and arbitrarily set to 128 entries.

### Auto traits

Since adding any combination of auto traits (in particular `Send`, `Sync`, `Unpin`) to a trait results in a distinct trait,
all relevant combinations of traits plus auto traits needs to be registered explicitly.

### Global registry

A custom non-static [`Registry`] can be built and used explicitly but the `Cast` traits will not use it.
