extern crate fred;
extern crate futures;
extern crate hyper;

#[macro_use]
extern crate serde_json;

use fred::RedisClient;
use fred::types::*;
use futures::{future, Future};
use hyper::{StatusCode, Body, Method};
use hyper::server::{Service, Request, Response};
use hyper::header::{Headers, Host, Location, UserAgent};
use serde_json::{Value};

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
fn redirect_response(location_value: RedisValue) -> Option<Response> {
    match location_value.into_string() {
        Some(location_url) => {
            Some(Response::new()
                 .with_status(StatusCode::MovedPermanently)
                 .with_header(Location::new(location_url))
            )
        }

        None => None
    }
}

// TODO Get remote address when X-Forwarded-For is not present
fn get_remote_addr(headers: &Headers) -> Option<String> {
    let raw = headers.get_raw("x-forwarded-for")?;
    let raw_addrs = raw.one()?;

    match String::from_utf8(raw_addrs.to_vec()) {
        Ok(string_addrs) => {
            let last = string_addrs.split(", ").last()?;

            Some(last.to_string())
        }

        Err(_) => None
    }
}

fn get_user_agent(headers: &Headers) -> Option<String> {
    let ua_hdr = headers.get::<UserAgent>()?;

    match String::from_utf8(ua_hdr.as_bytes().to_vec()) {
        Ok(ua) => Some(ua),
        Err(_) => None
    }
}

fn generate_request_json(headers: &Headers, host: String, path: String) -> String {
    let raddr = match get_remote_addr(&headers) {
        Some(s) => Value::String(s), None => Value::Null
    };

    let ua = match get_user_agent(&headers) {
        Some(s) => Value::String(s), None => Value::Null
    };

    json!(vec![Value::String(host), Value::String(path), raddr, ua]).to_string()
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
        let headers = req.headers();
        let host = headers.get::<Host>();

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

                let req_host = host_hdr.hostname();
                let req_json = generate_request_json(
                    &headers, req_host.to_owned(), req_path.clone()
                );

                let redis_client = self.redis_client.clone();
                let redis_get_key = format!("r3xx:{}:{}", req_host, req_path);

                Box::new(redis_client.get(redis_get_key).and_then(move |(client, value)| {
                    match value {
                        Some(rv) => {
                            future::ok(match redirect_response(rv) {
                                Some(r) => r,
                                None => service_unavailable()
                            }).join(
                                client.lpush("r3xx:hits", req_json)
                            )
                        }

                        None => {
                            future::ok(
                                default_redirect_response(default_url)
                            ).join(
                                client.lpush("r3xx:misses", req_json)
                            )
                        }
                    }
                }).and_then(|(resp, (_, _))| {
                    Ok(resp)

                }).or_else(|err| {
                    eprintln!("Redis Error: {:?}", err);

                    Ok(service_unavailable())
                }))
            }

            _ => not_found_response()
        }
    }
}
