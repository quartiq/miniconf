use miniconf::{heapless::Vec, Error, Miniconf};

#[test]
fn simple_vector_setting() {
    let mut vec: Vec<u8, 3> = Vec::new();
    vec.push(0).unwrap();

    // Updating the first element should succeed.
    let field = "0".split('/').peekable();
    vec.string_set(field, "7".as_bytes()).unwrap();
    assert_eq!(vec[0], 7);

    // Ensure that setting an out-of-bounds index generates an error.
    let field = "1".split('/').peekable();
    assert_eq!(
        vec.string_set(field, "7".as_bytes()).unwrap_err(),
        Error::BadIndex
    );
}

#[test]
fn simple_vector_getting() {
    let mut vec: Vec<u8, 3> = Vec::new();
    vec.push(7).unwrap();

    // Get the first field
    let field = "0".split('/').peekable();
    let mut buf: [u8; 256] = [0; 256];
    let len = vec.string_get(field, &mut buf).unwrap();
    assert_eq!(&buf[..len], "7".as_bytes());

    // Ensure that getting an out-of-bounds index generates an error.
    let field = "1".split('/').peekable();
    assert_eq!(
        vec.string_get(field, &mut buf).unwrap_err(),
        Error::BadIndex
    );

    // Pushing an item to the vec should make the second field accessible.
    vec.push(2).unwrap();
    let field = "1".split('/').peekable();
    let mut buf: [u8; 256] = [0; 256];
    let len = vec.string_get(field, &mut buf).unwrap();
    assert_eq!(&buf[..len], "2".as_bytes());
}

#[test]
fn vector_iteration() {
    let mut iterated = std::collections::HashMap::from([
        ("0".to_string(), false),
        ("1".to_string(), false),
        ("2".to_string(), false),
    ]);

    let mut vec: Vec<u8, 5> = Vec::new();
    vec.push(0).unwrap();
    vec.push(1).unwrap();
    vec.push(2).unwrap();

    let mut iter_state = [0; 32];
    for field in vec.iter_settings::<256>(&mut iter_state).unwrap() {
        assert!(iterated.contains_key(&field.as_str().to_string()));
        iterated.insert(field.as_str().to_string(), true);
    }

    // Ensure that all fields were iterated.
    assert!(iterated.iter().map(|(_, value)| value).all(|&x| x));
}

#[test]
fn vector_metadata() {
    let mut vec: Vec<u8, 3> = Vec::new();

    // Test metadata when the vector is empty
    let metadata = vec.get_metadata();
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_topic_size, 0);

    // When the vector has items, it can be iterated across and has additional metadata.
    vec.push(0).unwrap();
    let metadata = vec.get_metadata();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_topic_size, "0".len());
}
