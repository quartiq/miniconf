#![cfg(any(feature = "json-core", feature = "cbor"))]

#[cfg(feature = "json-core")]
use coap_message::MinimalWritableMessage as _;
#[cfg(feature = "json-core")]
use coap_message_implementations::heap::HeapMessage;
use coap_numbers::code;
#[cfg(feature = "json-core")]
use coap_numbers::option;
#[cfg(feature = "json-core")]
use miniconf::Tree;
use miniconf_coap::{Outcome, RequestParts};

#[cfg(feature = "json-core")]
#[derive(Tree)]
struct Settings {
    #[tree(meta(title = "Demo number"))]
    number: u32,
    #[tree(with = label)]
    label: heapless::String<16>,
    visible: Option<Visible>,
}

#[cfg(feature = "json-core")]
#[derive(Tree)]
struct Visible {
    value: u8,
}

#[cfg(feature = "json-core")]
impl Default for Settings {
    fn default() -> Self {
        Self {
            number: 7,
            label: "demo".try_into().unwrap(),
            visible: Some(Visible { value: 9 }),
        }
    }
}

#[cfg(feature = "json-core")]
mod label {
    use miniconf::{Keys, SerdeError, TreeDeserializer, ValueError, leaf};

    pub use leaf::{mut_any_by_key, probe_by_key, ref_any_by_key, schema, serialize_by_key};

    pub fn deserialize_by_key<'de, D: TreeDeserializer<'de>>(
        value: &mut heapless::String<16>,
        mut keys: impl Keys,
        de: D,
    ) -> Result<D::Ok, SerdeError<D::Error>> {
        keys.finalize()?;
        let (next, output) = de.deserialize::<heapless::String<16>>()?;
        if next.contains('<') {
            return Err(ValueError::Access("bad label").into());
        }
        *value = next;
        Ok(output)
    }
}

#[cfg(feature = "json-core")]
fn init_host_logging() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        defmt2log::init_from_current_exe();
    });
}

fn request<'a>(
    code: u8,
    path: &[&str],
    format: Option<u16>,
    payload: &'a [u8],
) -> RequestParts<'a> {
    RequestParts::new(code, path, None, format, payload).unwrap()
}

#[cfg(feature = "json-core")]
fn message(code: u8, path: &[&str], format: Option<u16>, payload: &[u8]) -> HeapMessage {
    let mut request = HeapMessage::new();
    request.set_code(code);
    for segment in path {
        request.add_option_str(option::URI_PATH, segment).unwrap();
    }
    if let Some(format) = format {
        request
            .add_option_uint(option::CONTENT_FORMAT, format)
            .unwrap();
    }
    request.set_payload(payload).unwrap();
    request
}

#[cfg(feature = "json-core")]
mod json {
    use super::*;
    use miniconf_coap::{JsonValueRoute, SchemaRoute};

    #[test]
    fn json_get_set_and_failed_write() {
        init_host_logging();
        let route = JsonValueRoute::json("/settings");
        let mut settings = Settings::default();
        let mut buf = [0; 128];
        let req = request(code::GET, &["settings", "number"], None, b"");
        let response = route
            .handle(&req, &mut settings, &mut buf)
            .response()
            .unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(response.content_format, Some(50));
        assert_eq!(response.payload, b"7");

        for (payload, expected) in [
            (&b"12"[..], code::CHANGED),
            (&b"13 trailing"[..], code::BAD_REQUEST),
        ] {
            let req = request(code::PUT, &["settings", "number"], Some(50), payload);
            let outcome = route.handle(&req, &mut settings, &mut buf);
            assert_eq!(
                matches!(outcome, Outcome::Written { .. }),
                expected == code::CHANGED
            );
            assert_eq!(outcome.response().unwrap().code, expected);
            assert_eq!(settings.number, 12);
        }
    }

    #[test]
    fn lookup_and_access_errors() {
        init_host_logging();
        let route = JsonValueRoute::json("/settings");
        let mut settings = Settings {
            visible: None,
            ..Settings::default()
        };
        let outside = request(code::GET, &["settings-other", "number"], None, b"");
        assert!(matches!(
            route.handle(&outside, &mut settings, &mut [0; 128]),
            Outcome::Unhandled
        ));
        for (path, expected) in [
            (&["settings", "visible", "value"][..], code::CONFLICT),
            (&["settings", "missing"][..], code::NOT_FOUND),
            (&["settings", "number", "extra"][..], code::NOT_FOUND),
            (&["settings"][..], code::METHOD_NOT_ALLOWED),
        ] {
            let req = request(code::GET, path, None, b"");
            let mut buf = [0; 128];
            let response = route
                .handle(&req, &mut settings, &mut buf)
                .response()
                .unwrap();
            assert_eq!(response.code, expected);
        }
        let req = request(
            code::PUT,
            &["settings", "label"],
            Some(50),
            br#""bad<label""#,
        );
        assert_eq!(
            route
                .handle(&req, &mut settings, &mut [0; 128])
                .response()
                .unwrap()
                .code,
            code::UNPROCESSABLE_ENTITY
        );
        assert_eq!(settings.label, "demo");
    }

    #[test]
    fn schema_route_is_paged() {
        use miniconf::{TreeSchema, compact_schema::SchemaDefs};
        use miniconf_coap::MAX_SCHEMA_DEFS;
        use yafnv::Fnv;

        init_host_logging();
        let handler = SchemaRoute::new("/schema", Settings::SCHEMA, 128);
        #[derive(serde::Deserialize)]
        struct Manifest {
            pages: usize,
            schema_rev: u32,
        }
        let mut buf = [0; 512];
        let root = request(code::GET, &["schema"], None, b"");
        let response = handler.handle(&root, &mut buf).response().unwrap();
        let (manifest, _) = serde_json_core::from_slice::<Manifest>(response.payload).unwrap();
        let mut pages = Vec::new();
        for page in 0..manifest.pages {
            let req = request(code::GET, &["schema", &page.to_string()], None, b"");
            let response = handler.handle(&req, &mut buf).response().unwrap();
            assert_eq!(response.code, code::CONTENT);
            pages.extend_from_slice(response.payload);
        }
        // Serialize definitions independently of pagination to detect missing or repeated pages.
        let defs = SchemaDefs::<MAX_SCHEMA_DEFS>::new(Settings::SCHEMA).unwrap();
        let mut expected = Vec::new();
        for id in 0..defs.len() {
            let len = serde_json_core::to_slice(&defs.definition(id).unwrap(), &mut buf).unwrap();
            expected.extend_from_slice(&buf[..len]);
            expected.push(b'\n');
        }
        assert_eq!(pages, expected);
        assert_eq!(manifest.schema_rev, u32::OFFSET_BASIS.fnv1a(pages));
        for path in [
            &["schema"][..],
            &["schema", "0"],
            &["schema", "1"],
            &["schema", "99"],
        ] {
            let req = request(code::GET, path, None, b"");
            let mut small = [0; 128];
            let mut large = [0; 512];
            let small = handler.handle(&req, &mut small);
            let large = handler.handle(&req, &mut large);
            let small = small.response().unwrap();
            let large = large.response().unwrap();
            assert_eq!(
                small.code,
                if path.last() == Some(&"99") {
                    code::NOT_FOUND
                } else {
                    code::CONTENT
                }
            );
            assert_eq!(small, large);
        }
        let leaf = SchemaRoute::new("/schema", &miniconf::Schema::LEAF, 3);
        let root = request(code::GET, &["schema"], None, b"");
        assert_eq!(
            leaf.handle(&root, &mut [0; 128]).response().unwrap().code,
            code::CONTENT
        );
        let req = request(code::GET, &["schema", "0"], None, b"");
        let mut short = [0; 127];
        assert_eq!(
            handler.handle(&req, &mut short).response().unwrap().code,
            code::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn option_rules_follow_criticality() {
        init_host_logging();
        use option::{ACCEPT, CONTENT_FORMAT};
        let json = &[50][..];
        let invalid = &[0, 0, 50][..];
        for (method, options, expected) in [
            (code::GET, &[(ACCEPT, json)][..], code::CONTENT),
            (code::GET, &[(ACCEPT, &[][..])][..], code::NOT_ACCEPTABLE),
            (
                code::GET,
                &[(ACCEPT, json), (ACCEPT, json)][..],
                code::BAD_OPTION,
            ),
            (code::GET, &[(ACCEPT, invalid)][..], code::BAD_OPTION),
            (code::GET, &[(99, &[][..])][..], code::BAD_OPTION),
            (
                code::PUT,
                &[(CONTENT_FORMAT, json), (CONTENT_FORMAT, &[][..])][..],
                code::CHANGED,
            ),
            (
                code::PUT,
                &[(CONTENT_FORMAT, invalid)][..],
                code::UNSUPPORTED_CONTENT_FORMAT,
            ),
            (
                code::PUT,
                &[(CONTENT_FORMAT, invalid), (CONTENT_FORMAT, json)][..],
                code::UNSUPPORTED_CONTENT_FORMAT,
            ),
        ] {
            let mut msg = message(method, &["settings", "number"], None, b"12");
            for &(number, value) in options {
                msg.add_option(number, value).unwrap();
            }
            let req = RequestParts::from_message(&msg).unwrap();
            let mut settings = Settings::default();
            assert_eq!(
                JsonValueRoute::json("/settings")
                    .handle(&req, &mut settings, &mut [0; 128])
                    .response()
                    .unwrap()
                    .code,
                expected
            );
            assert_eq!(
                settings.number,
                if expected == code::CHANGED { 12 } else { 7 }
            );
        }
    }

    #[test]
    fn get_serialization_failures_are_not_bad_payload() {
        init_host_logging();
        let handler = JsonValueRoute::json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 0];
        let req = request(code::GET, &["settings", "number"], None, b"");
        let outcome = handler.handle(&req, &mut settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::INTERNAL_SERVER_ERROR);
        assert_eq!(response.payload, b"");
    }
}

#[cfg(feature = "cbor")]
mod cbor {
    use super::*;
    use miniconf_coap::CborValueRoute;

    #[test]
    fn cbor_assignment_waits_for_complete_leaf() {
        #[derive(miniconf::Tree)]
        struct Pair {
            pair: miniconf::Leaf<[u32; 2]>,
        }
        let mut settings = Pair {
            pair: miniconf::Leaf([7, 8]),
        };
        let route = CborValueRoute::cbor("");
        let mut buf = [0; 128];
        for payload in [
            &[0x82, 9, 0xf5][..],  // Valid CBOR, wrong second element type.
            &[0x82, 9, 10, 0][..], // Valid pair followed by another item.
        ] {
            let req = RequestParts::new(code::PUT, &["pair"], None, Some(60), payload).unwrap();
            assert_eq!(
                route
                    .handle(&req, &mut settings, &mut buf)
                    .response()
                    .unwrap()
                    .code,
                code::BAD_REQUEST
            );
            assert_eq!(settings.pair.0, [7, 8]);
        }
        let req = RequestParts::new(code::PUT, &["pair"], None, Some(60), &[0x82, 9, 10]).unwrap();
        assert!(matches!(
            route.handle(&req, &mut settings, &mut buf),
            Outcome::Written { .. }
        ));
        assert_eq!(settings.pair.0, [9, 10]);
        let req = request(code::GET, &["pair"], None, b"");
        let response = route
            .handle(&req, &mut settings, &mut buf)
            .response()
            .unwrap();
        assert_eq!(response.payload, &[0x82, 9, 10]);
        assert_eq!(response.content_format, Some(60));
        assert_eq!(response.max_age, Some(0));
    }
}

#[cfg(all(feature = "json-core", feature = "coap-handler"))]
mod adapter {
    use super::*;
    use coap_handler::Handler as _;
    use coap_handler_implementations::{
        HandlerBuilder as _, ReportingHandlerBuilder as _, SimpleRendered, new_dispatcher,
    };
    use coap_message::{MessageOption as _, ReadableMessage as _, error::RenderableOnMinimal as _};
    use coap_message_implementations::inmemory;
    use miniconf_coap::{Json, MiniconfCoapHandler, SchemaCoapHandler};

    #[test]
    fn assignment_is_observable_before_response_and_never_repeated_by_rendering() {
        use miniconf_coap::ValueRequest;
        init_host_logging();
        let mut handler = MiniconfCoapHandler::<Settings, Json>::json(Settings::default());
        let request = message(code::PUT, &["number"], Some(50), b"31");
        let prepared = handler.extract_request_data(&request).unwrap();
        let ValueRequest::Written(key) = &prepared else {
            panic!("expected assignment");
        };
        assert_eq!(key.as_ref(), &[0]);
        assert_eq!(handler.settings().number, 31);
        assert_eq!(handler.estimate_length(&prepared), 0);
        handler.settings_mut().number = 42;
        for _ in 0..2 {
            let mut code = 0;
            let mut buf = [];
            let mut response = inmemory::MessageMut::new_in_slice(&mut code, &mut buf);
            handler
                .build_response(&mut response, prepared.clone())
                .unwrap();
            assert_eq!(response.code(), code::CHANGED);
            assert_eq!(handler.settings().number, 42);
        }
        assert_eq!(handler.into_inner().number, 42);
    }

    #[test]
    fn direct_and_adapted_responses_agree() {
        init_host_logging();
        for (method, path, payload) in [
            (code::GET, "number", &b""[..]),
            (code::GET, "missing", &b""[..]),
            (code::PUT, "number", &b"12"[..]),
            (code::PUT, "number", &b"13 trailing"[..]),
            (code::POST, "number", &b""[..]),
        ] {
            let request = message(method, &[path], Some(50), payload);
            let mut settings = Settings::default();
            let mut adapter = MiniconfCoapHandler::<Settings, Json>::json(Settings::default());
            let parts = RequestParts::from_message(&request).unwrap();
            let mut buf = [0; 512];
            let outcome =
                miniconf_coap::JsonValueRoute::json("").handle(&parts, &mut settings, &mut buf);
            let direct = outcome.response().unwrap();
            let mut expected = HeapMessage::new();
            direct.write_to(&mut expected).unwrap();
            let actual = handle_heap(&mut adapter, &request);
            assert_eq!(actual.code(), expected.code());
            assert_eq!(actual.payload(), expected.payload());
            assert_eq!(
                actual
                    .options()
                    .map(|o| (o.number(), o.value().to_vec()))
                    .collect::<Vec<_>>(),
                expected
                    .options()
                    .map(|o| (o.number(), o.value().to_vec()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(settings.number, adapter.settings().number);
        }
    }

    #[test]
    fn reports_and_serves_well_known_core() {
        init_host_logging();

        let mut handler = new_dispatcher()
            .below(
                &["settings"],
                MiniconfCoapHandler::<Settings, Json>::json(Settings::default()),
            )
            .below(
                &["schema"],
                SchemaCoapHandler::json(<Settings as miniconf::TreeSchema>::SCHEMA),
            )
            .at(&["status"], SimpleRendered::new_typed_str("ready", None))
            .with_wkc();

        let request = message(code::GET, &[".well-known", "core"], None, b"");
        let response = handle_heap(&mut handler, &request);

        assert_eq!(response.code(), code::CONTENT);
        assert_eq!(
            response
                .options()
                .find(|o| o.number() == option::CONTENT_FORMAT)
                .unwrap()
                .value_uint::<u16>(),
            Some(40)
        );
        let payload = core::str::from_utf8(response.payload()).unwrap();
        assert!(payload.contains("</settings/number>;ct=50;title=\"Demo number\""));
        assert!(payload.contains("</schema>;ct=50;rt=\"miniconf.schema\""));
        assert!(payload.contains("</status>"));
    }

    #[test]
    fn response_estimate_fits_a_full_payload_and_options() {
        let mut value = heapless::String::<510>::new();
        for _ in 0..510 {
            value.push('x').unwrap();
        }
        let mut handler = MiniconfCoapHandler::json(miniconf::Leaf(value));
        let request = handler
            .extract_request_data(&message(code::GET, &[], None, b""))
            .unwrap();
        let mut buf = vec![0; handler.estimate_length(&request)];
        let mut code = 0;
        let mut response = inmemory::MessageMut::new_in_slice(&mut code, &mut buf);
        handler.build_response(&mut response, request).unwrap();
        let (len, _) = response.finish();
        let response = inmemory::Message::new_from_slice(code, &buf[..len]);
        assert_eq!(response.code(), coap_numbers::code::CONTENT);
        assert_eq!(response.payload().len(), 512);
        assert_eq!(
            response
                .options()
                .find(|o| o.number() == option::MAX_AGE)
                .unwrap()
                .value_uint::<u32>(),
            Some(0)
        );
    }

    fn handle_heap<H>(handler: &mut H, request: &HeapMessage) -> HeapMessage
    where
        H: coap_handler::Handler,
    {
        let mut response = HeapMessage::new();
        match handler.extract_request_data(request) {
            Ok(data) => handler.build_response(&mut response, data).unwrap(),
            Err(error) => error.render(&mut response).unwrap(),
        }
        response
    }
}
