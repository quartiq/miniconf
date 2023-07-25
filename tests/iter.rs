use miniconf::{Miniconf, SerDe};

#[derive(Miniconf, Default)]
struct Inner {
    inner: f32,
}

#[derive(Miniconf, Default)]
struct Settings {
    a: f32,
    b: i32,
    #[miniconf(defer)]
    c: Inner,
}

#[test]
fn insufficient_space() {
    let meta = Settings::metadata(1);
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length, "/c/inner".len());
    assert_eq!(meta.count, 3);

    // Ensure that we can't iterate if we make a state vector that is too small.
    assert!(Settings::iter_paths::<1, 256>().is_err());

    // Ensure that we can't iterate if the topic buffer is too small.
    assert!(Settings::iter_paths::<10, 1>().is_err());
}

#[test]
fn test_iteration() {
    let mut iterated = std::collections::HashMap::from([
        ("/a".to_string(), false),
        ("/b".to_string(), false),
        ("/c/inner".to_string(), false),
    ]);

    for field in Settings::iter_paths::<32, 256>().unwrap() {
        assert!(iterated.contains_key(&field.as_str().to_string()));
        iterated.insert(field.as_str().to_string(), true);
    }

    // Ensure that all fields were iterated.
    assert!(iterated.iter().map(|(_, value)| value).all(|&x| x));
}

#[test]
fn test_array_iteration() {
    #[derive(Miniconf, Default)]
    struct Settings {
        #[miniconf(defer)]
        data: [bool; 5],
    }

    let mut settings = Settings::default();

    for field in Settings::iter_paths::<32, 256>().unwrap() {
        settings.set(&field, b"true").unwrap();
    }

    assert!(settings.data.iter().all(|x| *x));
}
