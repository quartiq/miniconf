use miniconf::Miniconf;

#[derive(Miniconf, Default)]
struct Inner {
    inner: f32,
}

#[derive(Miniconf, Default)]
struct Settings {
    a: f32,
    b: i32,
    c: Inner,
}

#[test]
fn insufficient_space() {
    let settings = Settings::default();
    let meta = settings.get_metadata();
    assert_eq!(meta.max_depth, 3);
    assert_eq!(meta.max_topic_size, "c/inner".len());

    // Ensure that we can't iterate if we make a state vector that is too small.
    let mut small_state = [0; 2];
    assert!(settings.iter_settings::<256>(&mut small_state).is_err());

    // Ensure that we can't iterate if the topic buffer is too small.
    let mut state = [0; 10];
    assert!(settings.iter_settings::<1>(&mut state).is_err());
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
    for field in settings.iter_settings::<256>(&mut iter_state).unwrap() {
        assert!(iterated.contains_key(&field.as_str().to_string()));
        iterated.insert(field.as_str().to_string(), true);
    }

    // Ensure that all fields were iterated.
    assert!(iterated.iter().map(|(_, value)| value).all(|&x| x));
}

#[test]
fn test_array_iteration() {
    let settings = [false; 5];
    let mut settings_copy = [false; 5];

    let mut iter_state = [0; 32];
    for field in settings.iter_settings::<256>(&mut iter_state).unwrap() {
        settings_copy.set(&field, b"true").unwrap();
    }

    assert!(settings_copy.iter().all(|x| *x));
}
