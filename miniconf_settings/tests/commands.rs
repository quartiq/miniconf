#![cfg(feature = "shell")]

use futures::executor::block_on;
use miniconf::TreeSchema;
use miniconf_settings::shell::{
    Command, Completion, ParseError, complete, complete_path, write_schema, write_value,
    write_values,
};

#[path = "../examples/common.rs"]
mod common;

#[test]
fn borrowed_commands() {
    assert_eq!(Command::parse("reboot"), Err(ParseError::UnknownCommand));
    assert_eq!(Command::parse("get relative"), Err(ParseError::Path));
    for line in ["get /serial number", "schema /serial\u{a0}number", "set 42"] {
        assert_eq!(Command::parse(line), Err(ParseError::Arguments));
    }
    assert_eq!(
        Command::parse("schema /output"),
        Ok(Some(Command::Schema("/output")))
    );
    assert_eq!(
        Command::parse("set /name \"output A\""),
        Ok(Some(Command::Set {
            path: "/name",
            value: "\"output A\""
        }))
    );
}

#[test]
fn command_completion_and_context() {
    let commands = &["get", "set", "schema", "help", "reboot"];
    for (line, cursor, expected) in [
        (
            "se",
            2,
            Some(Completion::Replace(0..2, "set ".try_into().unwrap())),
        ),
        (
            "re",
            2,
            Some(Completion::Replace(0..2, "reboot ".try_into().unwrap())),
        ),
        (
            "ge /output",
            2,
            Some(Completion::Replace(0..2, "get".try_into().unwrap())),
        ),
        (
            "get /output/dac/0",
            8,
            Some(Completion::Replace(5..11, "output".try_into().unwrap())),
        ),
        (
            "get /output/dac/0",
            11,
            Some(Completion::Matches(Some(4..11))),
        ),
        (
            "get /output/dac/",
            16,
            Some(Completion::Matches(Some(4..16))),
        ),
        (
            "set /output/dac/0",
            17,
            Some(Completion::Replace(16..17, "0 ".try_into().unwrap())),
        ),
        (
            "set /output/dac/0 2048",
            17,
            Some(Completion::Replace(16..18, "0 ".try_into().unwrap())),
        ),
        (
            "get /ser",
            8,
            Some(Completion::Replace(5..8, "serial ".try_into().unwrap())),
        ),
        ("get /missing/x", 14, None),
        ("set /serial 12", 14, None),
        ("reboot now", 10, None),
    ] {
        assert_eq!(
            complete(common::Settings::SCHEMA, line, cursor, commands),
            expected
        );
    }
    assert_eq!(
        complete(common::Settings::SCHEMA, "get /out", 8, &["reboot"]),
        None
    );
    assert_eq!(
        complete(common::Settings::SCHEMA, "ge", 2, &["get", "get"]),
        Some(Completion::Replace(0..2, "get ".try_into().unwrap()))
    );
}

#[test]
fn command_completion_omits_whitespace_names() {
    use miniconf::{Meta, Named, Schema};

    assert_eq!(
        complete(common::Settings::SCHEMA, "se", 2, &["send value", "set"]),
        Some(Completion::Replace(0..2, "set ".try_into().unwrap()))
    );
    const SCHEMA: &Schema = &Schema::named(&[
        Named::new("serial number", &Schema::LEAF, Meta::EMPTY),
        Named::new("serial", &Schema::LEAF, Meta::EMPTY),
    ]);
    let mut candidates = Vec::new();
    complete_path(SCHEMA, "/ser", 4, |_, name| {
        candidates.push(name.to_owned())
    });
    assert_eq!(candidates, ["serial number", "serial"]);
    assert_eq!(
        complete(SCHEMA, "set /ser", 8, &["set"]),
        Some(Completion::Replace(5..8, "serial ".try_into().unwrap()))
    );
}

#[test]
fn numeric_completion_selects_an_index() {
    for (line, cursor, expected) in [
        ("get /", 5, Some(Completion::Matches(Some(4..5)))),
        (
            "get /1",
            6,
            Some(Completion::Replace(5..6, "1 ".try_into().unwrap())),
        ),
        (
            "get /999",
            8,
            Some(Completion::Replace(5..8, "999 ".try_into().unwrap())),
        ),
        ("get /1000", 9, Some(Completion::Matches(Some(4..9)))),
    ] {
        assert_eq!(
            complete(<[u16; 1000]>::SCHEMA, line, cursor, &["get"]),
            expected
        );
    }
    assert_eq!(
        complete(<(u16, [u16; 2])>::SCHEMA, "set /1", 6, &["set"]),
        Some(Completion::Matches(Some(4..6)))
    );
}

#[test]
fn empty_path_segments() {
    use miniconf::{Meta, Named, Schema};

    const EMPTY: &Schema = &Schema::named(&[Named::new("", &Schema::LEAF, Meta::EMPTY)]);
    const NESTED: &Schema =
        &Schema::named(&[Named::new("", common::Settings::SCHEMA, Meta::EMPTY)]);
    assert_eq!(Command::parse("get /"), Ok(Some(Command::Get("/"))));
    assert_eq!(
        complete(EMPTY, "get /", 5, &["get"]),
        Some(Completion::Replace(5..5, " ".try_into().unwrap()))
    );
    assert_eq!(
        complete(NESTED, "get //ser", 9, &["get"]),
        Some(Completion::Replace(6..9, "serial ".try_into().unwrap()))
    );
}

#[test]
fn path_completion_preserves_the_suffix() {
    for (path, cursor, expected) in [
        ("/out", 4, vec![(1..4, "output")]),
        ("/out/dac/0", 4, vec![(1..4, "output")]),
        ("/output/dac/", 12, vec![(12..12, "0"), (12..12, "1")]),
        ("/missing/", 9, vec![]),
        ("output/", 7, vec![]),
    ] {
        let mut found = Vec::new();
        complete_path(common::Settings::SCHEMA, path, cursor, |range, text| {
            found.push((range, text.to_owned()));
        });
        assert_eq!(
            found,
            expected
                .into_iter()
                .map(|(r, s)| (r, s.to_owned()))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn schema_inspection_needs_no_settings_instance() {
    let mut output = Vec::new();
    block_on(write_schema(
        &mut output,
        common::Settings::SCHEMA,
        "/calibration",
        usize::MAX,
    ))
    .unwrap();
    assert_eq!(output, b"/calibration [named] [sem maybe_absent] [edge doc=\"Factory calibration applied to measurements.\"] [node typename=\"Calibration\"]\r\n  offset [leaf] [sem ty=i32]\r\n  slope [leaf] [sem ty=i16] [edge unit=\"ppm\"]\r\n");
    output.clear();
    block_on(write_schema(
        &mut output,
        common::Settings::SCHEMA,
        "relative",
        usize::MAX,
    ))
    .unwrap();
    assert_eq!(output, b"error: NotFound\r\n");
}

#[test]
fn nested_homogeneous_and_numbered() {
    let mut output = Vec::new();
    block_on(write_schema(
        &mut output,
        <([[u16; 2]; 3], bool)>::SCHEMA,
        "",
        usize::MAX,
    ))
    .unwrap();
    assert_eq!(
        std::str::from_utf8(&output).unwrap(),
        concat!(
            "(root) [numbered]\r\n",
            "  0 [homogeneous]\r\n",
            "    0..3 [homogeneous]\r\n",
            "      0..2 [leaf] [sem ty=u16]\r\n",
            "  1 [leaf] [sem ty=bool]\r\n",
        )
    );
    output.clear();
    block_on(write_schema(
        &mut output,
        <([[u16; 2]; 3], bool)>::SCHEMA,
        "",
        1,
    ))
    .unwrap();
    assert_eq!(
        output,
        b"(root) [numbered]\r\n  0 [homogeneous]\r\n  1 [leaf] [sem ty=bool]\r\n"
    );
}

#[test]
fn inspect_values_and_errors() {
    env_logger::builder().is_test(true).try_init().unwrap();
    defmt2log::init_from_current_exe();
    block_on(async {
        let settings = common::Settings::new();
        let mut output = Vec::new();
        write_values(&mut output, &settings, "/output/dac", &mut [0; 32])
            .await
            .unwrap();
        assert_eq!(output, b"/output/dac/0: 1024\r\n/output/dac/1: 1024\r\n");
        output.clear();
        write_values(&mut output, &settings, "/missing", &mut [0; 32])
            .await
            .unwrap();
        assert_eq!(output, b"error: Key(NotFound)\r\n");
    });
}

#[test]
fn shared_settings_during_output() {
    use std::{cell::RefCell, future::poll_fn, task::Poll};

    use miniconf::{ConstPath, NodeIter, json_core};
    use miniconf_settings::{MAX_DEPTH, MAX_PATH_LENGTH};

    struct Slow(Vec<u8>);
    impl embedded_io_async::ErrorType for Slow {
        type Error = std::convert::Infallible;
    }
    impl embedded_io_async::Write for Slow {
        fn write(&mut self, bytes: &[u8]) -> impl Future<Output = Result<usize, Self::Error>> {
            let mut pending = true;
            poll_fn(move |cx| {
                if std::mem::take(&mut pending) {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                self.0.extend_from_slice(bytes);
                Poll::Ready(Ok(bytes.len()))
            })
        }

        async fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    block_on(async {
        let settings = RefCell::new(common::Settings::new());
        let mut reply = std::pin::pin!(async {
            let mut output = Slow(Vec::new());
            let paths = NodeIter::<
                ConstPath<heapless::String<MAX_PATH_LENGTH>, '/'>,
                MAX_DEPTH,
            >::with_root(common::Settings::SCHEMA, "/output/dac")
            .unwrap();
            let mut scratch = [0; 32];
            for path in paths {
                let path = path.unwrap();
                let reply = write_value(
                    &mut output,
                    &*settings.borrow(),
                    path.as_ref(),
                    &mut scratch,
                );
                reply.await.unwrap();
            }
            output.0
        });
        assert!(futures::poll!(&mut reply).is_pending());
        json_core::set(&mut *settings.borrow_mut(), "/output/dac/0", b"2048").unwrap();
        json_core::set(&mut *settings.borrow_mut(), "/output/dac/1", b"2048").unwrap();
        assert_eq!(
            reply.await,
            b"/output/dac/0: 1024\r\n/output/dac/1: 2048\r\n"
        );
    });
}
