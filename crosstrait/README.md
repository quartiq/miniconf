# Cast from `dyn Any` to other trait objects

* `no_std` no alloc support
* No proc macros

## Usage

```toml
[dependencies]
crosstrait = "0.1"
```

Then use the `register!{}` declarative macro and the [`Cast`] traits.

For embedded, the linker needs to be informed of the type registry.

## Example

```rust
use core::any::Any;
use crosstrait::{register, Cast, Castable, CastableRef};

// Some example traits
use core::{fmt::{Debug, Formatter, Write}, ops::AddAssign};

// Add types and implementations in the default global registy.
// Implementation status is verified at compile time
register! { u8 => dyn Debug }
// Auto-traits are distinct and must be explicitly registered
register! { i32 => dyn AddAssign<i32> + Sync }
// If a type is not Send + Sync, it can't cast as Arc:
register! { Formatter => dyn Write, no_arc }
// Registering foreign types and traits works fine
register! { String => dyn Write }
// Serialization/deserialization of `dyn Any` is a major use case.
// register! { i32 => dyn erased_serde::Serialize }

fn main() {
    // Check for trait impl registration
    let any: &dyn Any = &0u8;
    assert!(any.castable::<dyn Debug>());
    // AddAssign is not registered for u8
    assert!(!any.castable::<dyn AddAssign<i32>>());
    // Check based on type
    assert!(u8::castable::<dyn Debug>());

    // Cast ref
    let a: &dyn Debug = any.cast().unwrap();
    println!("{a:?}");

    // Autotraits are distinct
    assert!(Cast::<&dyn AddAssign<i32>>::cast(any).is_none());

    // Cast mut
    let mut value = 5i32;
    let any: &mut dyn Any = &mut value;
    let v: &mut (dyn AddAssign<i32> + Sync) = any.cast().unwrap();
    *v += 3;
    assert_eq!(value, 5 + 3);

    // Cast Box
    let any: Box<dyn Any> = Box::new(0u8);
    let _: Box<dyn Debug> = any.cast().unwrap();

    // Cast Rc
    use std::rc::Rc;
    let any: Rc<dyn Any> = Rc::new(0u8);
    let _: Rc<dyn Debug> = any.cast().unwrap();

    // Cast Arc
    use std::sync::Arc;
    let any: Arc<dyn Any + Sync + Send> = Arc::new(0u8);
    let _: Arc<dyn Debug> = any.cast().unwrap();

    // Use an explicit registry
    crosstrait::REGISTRY.cast_ref::<dyn Debug>(&0u8 as &dyn Any).unwrap();
}
```

## Related crates

* [`intertrait`](https://crates.io/crates/intertrait): similar goals, `std`
* [`miniconf`](https://crates.io/crates/miniconf): provides several ways to get `dyn Any` from heterogeneous
  nested data structures, `no_std`, no alloc
* [`erased_serde`](https://crates.io/crates/erased-serde): serialization on trait objects, serializer/deserializer trait objects
* [`linkme`](https://crates.io/crates/linkme): linker magic to build distributed static slices

## Limitations

### Registry size on `no_std`

Currently the size of the global registry on `no_std` is fixed and arbitrarily set to 128 entries.

### Auto traits

Since adding any combination of auto traits (in particular `Send`, `Sync`, `Unpin`) to a trait results in a distinct trait,
all relevant combinations of traits plus auto traits needs to be registered explicitly.
