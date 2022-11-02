use miniconf::Miniconf;

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
    let settings = Settings::default();
    let meta = settings.metadata();
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length, "c/inner".len());

    // Ensure that we can't iterate if we make a state vector that is too small.
    let mut small_state = [0; 1];
    assert!(settings.iter_paths::<256>(&mut small_state).is_err());

    // Ensure that we can't iterate if the topic buffer is too small.
    let mut state = [0; 10];
    assert!(settings.iter_paths::<1>(&mut state).is_err());
}

#[test]
fn test_iteration() {
    let settings = Settings::default();

    let mut iterated = std::collections::HashMap::from([
        ("a".to_string(), false),
        ("b".to_string(), false),
        ("c/inner".to_string(), false),
    ]);

    let mut iter_state = [0; 32];
    for field in settings.iter_paths::<256>(&mut iter_state).unwrap() {
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

    let settings = Settings::default();
    let mut settings_copy = Settings::default();

    let mut iter_state = [0; 32];
    for field in settings.iter_paths::<256>(&mut iter_state).unwrap() {
        settings_copy.set(&field, b"true").unwrap();
    }

    assert!(settings_copy.data.iter().all(|x| *x));
}
