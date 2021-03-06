// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

//! Hyper Server Handler that fetches a file during a request (proxy).

use std::{fs, fmt};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::sync::atomic::AtomicBool;
use std::time::{Instant, Duration};

use hyper::{header, server, Decoder, Encoder, Next, Method, Control};
use hyper::net::HttpStream;
use hyper::status::StatusCode;

use handlers::ContentHandler;
use handlers::client::{Client, FetchResult};
use apps::redirection_address;

const FETCH_TIMEOUT: u64 = 30;

enum FetchState<T: fmt::Debug> {
	NotStarted(String),
	Error(ContentHandler),
	InProgress {
		deadline: Instant,
		receiver: mpsc::Receiver<FetchResult>,
	},
	Done((String, T)),
}

pub trait ContentValidator {
	type Error: fmt::Debug + fmt::Display;
	type Result: fmt::Debug;

	fn validate_and_install(&self, app: PathBuf) -> Result<(String, Self::Result), Self::Error>;
	fn done(&self, Option<&Self::Result>);
}

pub struct ContentFetcherHandler<H: ContentValidator> {
	abort: Arc<AtomicBool>,
	control: Option<Control>,
	status: FetchState<H::Result>,
	client: Option<Client>,
	using_dapps_domains: bool,
	installer: H,
}

impl<H: ContentValidator> Drop for ContentFetcherHandler<H> {
	fn drop(&mut self) {
		let result = match self.status {
			FetchState::Done((_, ref result)) => Some(result),
			_ => None,
		};
		self.installer.done(result);
	}
}

impl<H: ContentValidator> ContentFetcherHandler<H> {

	pub fn new(
		url: String,
		abort: Arc<AtomicBool>,
		control: Control,
		using_dapps_domains: bool,
		handler: H) -> Self {

		let client = Client::new();
		ContentFetcherHandler {
			abort: abort,
			control: Some(control),
			client: Some(client),
			status: FetchState::NotStarted(url),
			using_dapps_domains: using_dapps_domains,
			installer: handler,
		}
	}

	fn close_client(client: &mut Option<Client>) {
		client.take()
			.expect("After client is closed we are going into write, hence we can never close it again")
			.close();
	}


	fn fetch_content(client: &mut Client, url: &str, abort: Arc<AtomicBool>, control: Control) -> Result<mpsc::Receiver<FetchResult>, String> {
		client.request(url, abort, Box::new(move || {
			trace!(target: "dapps", "Fetching finished.");
			// Ignoring control errors
			let _ = control.ready(Next::read());
		})).map_err(|e| format!("{:?}", e))
	}
}

impl<H: ContentValidator> server::Handler<HttpStream> for ContentFetcherHandler<H> {
	fn on_request(&mut self, request: server::Request<HttpStream>) -> Next {
		let status = if let FetchState::NotStarted(ref url) = self.status {
			Some(match *request.method() {
				// Start fetching content
				Method::Get => {
					trace!(target: "dapps", "Fetching content from: {:?}", url);
					let control = self.control.take().expect("on_request is called only once, thus control is always Some");
					let client = self.client.as_mut().expect("on_request is called before client is closed.");
					let fetch = Self::fetch_content(client, url, self.abort.clone(), control);
					match fetch {
						Ok(receiver) => FetchState::InProgress {
							deadline: Instant::now() + Duration::from_secs(FETCH_TIMEOUT),
							receiver: receiver,
						},
						Err(e) => FetchState::Error(ContentHandler::error(
							StatusCode::BadGateway,
							"Unable To Start Dapp Download",
							"Could not initialize download of the dapp. It might be a problem with the remote server.",
							Some(&format!("{}", e)),
						)),
					}
				},
				// or return error
				_ => FetchState::Error(ContentHandler::error(
					StatusCode::MethodNotAllowed,
					"Method Not Allowed",
					"Only <code>GET</code> requests are allowed.",
					None,
				)),
			})
		} else { None };

		if let Some(status) = status {
			self.status = status;
		}

		Next::read()
	}

	fn on_request_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
		let (status, next) = match self.status {
			// Request may time out
			FetchState::InProgress { ref deadline, .. } if *deadline < Instant::now() => {
				trace!(target: "dapps", "Fetching dapp failed because of timeout.");
				let timeout = ContentHandler::error(
					StatusCode::GatewayTimeout,
					"Download Timeout",
					&format!("Could not fetch content within {} seconds.", FETCH_TIMEOUT),
					None
				);
				Self::close_client(&mut self.client);
				(Some(FetchState::Error(timeout)), Next::write())
			},
			FetchState::InProgress { ref receiver, .. } => {
				// Check if there is an answer
				let rec = receiver.try_recv();
				match rec {
					// Unpack and validate
					Ok(Ok(path)) => {
						trace!(target: "dapps", "Fetching content finished. Starting validation ({:?})", path);
						Self::close_client(&mut self.client);
						// Unpack and verify
						let state = match self.installer.validate_and_install(path.clone()) {
							Err(e) => {
								trace!(target: "dapps", "Error while validating content: {:?}", e);
								FetchState::Error(ContentHandler::error(
									StatusCode::BadGateway,
									"Invalid Dapp",
									"Downloaded bundle does not contain a valid content.",
									Some(&format!("{:?}", e))
								))
							},
							Ok(result) => FetchState::Done(result)
						};
						// Remove temporary zip file
						let _ = fs::remove_file(path);
						(Some(state), Next::write())
					},
					Ok(Err(e)) => {
						warn!(target: "dapps", "Unable to fetch content: {:?}", e);
						let error = ContentHandler::error(
							StatusCode::BadGateway,
							"Download Error",
							"There was an error when fetching the content.",
							Some(&format!("{:?}", e)),
						);
						(Some(FetchState::Error(error)), Next::write())
					},
					// wait some more
					_ => (None, Next::wait())
				}
			},
			FetchState::Error(ref mut handler) => (None, handler.on_request_readable(decoder)),
			_ => (None, Next::write()),
		};

		if let Some(status) = status {
			self.status = status;
		}

		next
	}

	fn on_response(&mut self, res: &mut server::Response) -> Next {
		match self.status {
			FetchState::Done((ref id, _)) => {
				trace!(target: "dapps", "Fetching content finished. Redirecting to {}", id);
				res.set_status(StatusCode::Found);
				res.headers_mut().set(header::Location(redirection_address(self.using_dapps_domains, id)));
				Next::write()
			},
			FetchState::Error(ref mut handler) => handler.on_response(res),
			_ => Next::end(),
		}
	}

	fn on_response_writable(&mut self, encoder: &mut Encoder<HttpStream>) -> Next {
		match self.status {
			FetchState::Error(ref mut handler) => handler.on_response_writable(encoder),
			_ => Next::end(),
		}
	}
}

