extern crate docopt;
extern crate fred;
extern crate futures;
extern crate hyper;
extern crate url_shortener;

use docopt::Docopt;
use fred::RedisClient;
use fred::types::*;
use futures::{Future};
use hyper::server::{Http};

use std::net::{SocketAddr};
use std::env;

use url_shortener::UrlShortener;

const USAGE: &'static str = "
UrlShortener HTTP Server

Usage:
  url_shortener [--bind=<bind>] [--port=<port>] [--redis-host=<redis_host>] [--redis-port=<redis_port>]
  url_shortener (-h | --help)
  url_shortener (-v | --version)

  Options:
  -h --help                  Show this screen.
  --bind=<bind>              Bind to specific IP [default: 127.0.0.1]
  --port=<port>              Run on a specific port number [default: 5000]
  --redis-host=<redis_host>  Connect to redis using specific IP [default: 127.0.0.1]
  --redis-port=<redis_port>  Connect to redis using specific port number [default: 6379]
";

fn parse_addr_from_args(bind: &str, port: &str) -> SocketAddr {
    match format!("{}:{}", bind, port).parse() {
        Ok(addr) => addr,
        Err(err) => panic!("Addr parse error: {:?}", err)
    }
}

fn parse_port(port: &str) -> u16 {
    match u16::from_str_radix(port, 10) {
        Ok(num) => num,
        Err(err) => panic!("Port number parse error: {:?}", err)
    }
}

fn get_default_redirect_url_from_env() -> String {
    match env::var("REDIRECT_URL") {
        Ok(val) => val,
        Err(_) => "127.0.0.1:5000".to_owned()
    }
}

fn main() {
    let default_redirect_url = get_default_redirect_url_from_env();

    let args = Docopt::new(USAGE)
        .and_then(|dopt| dopt.parse())
        .unwrap_or_else(|e| e.exit());

    let redis_config = RedisConfig::new_centralized(
        args.get_str("--redis-host"), parse_port(args.get_str("--redis-port")), None
    );

    let redis_policy = ReconnectPolicy::Constant {
        delay: 2000, attempts: 0, max_attempts: 0
    };

    let redis_client = RedisClient::new(redis_config);
    let redis_service_client = redis_client.clone();

    let socket_addr = parse_addr_from_args(args.get_str("--bind"), args.get_str("--port"));
    let server = Http::new().bind(&socket_addr, move || {
        Ok(UrlShortener::new(default_redirect_url.clone(), &redis_service_client))
    });

    match server {
        Ok(svr) => {
            let redis_conn = redis_client.connect_with_policy(&svr.handle(), redis_policy);

            println!("HTTP Server Started");

            match svr.run_until(redis_conn.map_err(|_| ())) {
                Ok(_) => println!("HTTP Server Finished Running"),
                Err(err) => panic!("HTTP Server Error: {:?}", err)
            }
        }

        Err(err) => panic!("HTTP Server Error: {:?}", err)
    }
}
