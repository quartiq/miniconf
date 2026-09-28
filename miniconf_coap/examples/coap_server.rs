use std::{env, thread, time::Duration};

use coap_handler::{Handler, Reporting};
use coap_handler_implementations::{
    HandlerBuilder as _, ReportingHandlerBuilder as _, new_dispatcher,
};
use coap_message::{MinimalWritableMessage, MutableWritableMessage, ReadableMessage};
use embedded_nal::{UdpClientStack as _, UdpFullStack as _, nb};
use miniconf::{Tree, TreeSchema};
use miniconf_coap::{Error, Json, MiniconfCoapHandler, SchemaCoapHandler, ValueRequest};

#[derive(Default, Tree)]
struct Settings {
    enabled: bool,
}

struct Application(MiniconfCoapHandler<Settings, Json>);

impl Handler for Application {
    type RequestData = ValueRequest;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<ValueRequest, Error> {
        let request = self.0.extract_request_data(request)?;
        if let ValueRequest::Written(key) = &request {
            println!(
                "assigned {:?}: enabled = {}",
                key.as_ref(),
                self.0.settings().enabled
            );
        }
        Ok(request)
    }

    fn estimate_length(&mut self, request: &ValueRequest) -> usize {
        self.0.estimate_length(request)
    }

    fn build_response<M: MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: ValueRequest,
    ) -> Result<(), M::UnionError> {
        self.0.build_response(response, request)
    }
}

impl Reporting for Application {
    type Record<'a> = <MiniconfCoapHandler<Settings, Json> as Reporting>::Record<'a>;
    type Reporter<'a> = <MiniconfCoapHandler<Settings, Json> as Reporting>::Reporter<'a>;

    fn report(&self) -> Self::Reporter<'_> {
        self.0.report()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    defmt2log::init_from_current_exe();
    let port = env::args()
        .nth(1)
        .map(|port| port.parse())
        .transpose()?
        .unwrap_or(56830);
    let mut stack = std_embedded_nal::Stack;
    let mut socket = stack.socket()?;
    stack.bind(&mut socket, port)?;
    let mut handler = new_dispatcher()
        .below(
            &["settings"],
            Application(MiniconfCoapHandler::json(Settings::default())),
        )
        .below(&["schema"], SchemaCoapHandler::json(Settings::SCHEMA))
        .with_wkc();
    println!("CoAP on UDP port {port}; GET /settings/enabled, /schema or /.well-known/core");
    loop {
        match embedded_nal_minimal_coapserver::poll(&mut stack, &mut socket, &mut handler) {
            Ok(()) => {}
            Err(nb::Error::WouldBlock) => thread::sleep(Duration::from_millis(10)),
            Err(nb::Error::Other(error)) => return Err(error.into()),
        }
    }
}
