extern crate fred;
extern crate futures;
extern crate hyper;

use fred::RedisClient;
use fred::types::*;
use futures::{future, Future};
use hyper::{StatusCode, Body, Method};
use hyper::server::{Service, Request, Response};
use hyper::header::{Host, Location};

type WebResponse = Box<Future<Item=Response, Error=hyper::Error>>;

static NOT_FOUND: &[u8] = b"404 Not Found";

/// Create a 404 not found response as WebResponse
fn not_found_response() -> WebResponse {
    let body = Body::from(NOT_FOUND);
    let resp = Response::new()
        .with_status(StatusCode::NotFound)
        .with_body(body);

    Box::new(future::ok(resp))
}

/// Create a 503 service unavailable Response
fn service_unavailable() -> Response {
    Response::new()
        .with_status(StatusCode::ServiceUnavailable)
}

/// Create a 307 temporary redirect response from the default_url
fn default_redirect_response(default_url: String) -> Response {
    Response::new()
        .with_status(StatusCode::TemporaryRedirect)
        .with_header(Location::new(default_url))
}

/// Create a 309 redirect response if the value is defined or return None
fn redirect_response(location: Option<RedisValue>) -> Option<Response> {
    match location {
        Some(location_value) => match location_value.into_string() {
            Some(location_url) => Some(Response::new()
                .with_status(StatusCode::MovedPermanently)
                .with_header(Location::new(location_url))
            ),

            _ => None
        }

        _ => None
    }
}


pub struct UrlShortener<'a> {
    default_url: String,
    redis_client: &'a RedisClient
}

impl <'a> UrlShortener<'a> {
    pub fn new(default_url: String, redis_client: &'a RedisClient) -> UrlShortener<'a> {
        UrlShortener { default_url, redis_client }
    }
}

impl <'a> Service for UrlShortener<'a> {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = WebResponse;

    fn call(&self, req: Request) -> WebResponse {
        let host = req.headers().get::<Host>();

        match (req.method(), host) {
            (&Method::Get, Some(host_hdr)) => {
                let default_url = self.default_url.clone();

                // Early return if there is no request path
                let req_path: String = req.path().chars().skip(1).collect();
                if req_path.len() < 1 {
                    return Box::new(
                        future::ok(default_redirect_response(default_url))
                    );
                }

                let redis_client = self.redis_client.clone();
                let redis_get_key = format!("{}:{}", host_hdr.hostname(), req_path);

                Box::new(redis_client.get(redis_get_key).and_then(|(_, value)| {
                    Ok(match redirect_response(value) {
                        Some(resp) => resp,
                        None => default_redirect_response(default_url)
                    })
                }).or_else(|err| {
                    eprintln!("Redis Error: {:?}", err);

                    Ok(service_unavailable())
                }))
            }

            _ => not_found_response()
        }
    }
}
