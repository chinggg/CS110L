mod request;
mod response;

use std::{sync::Arc, collections::HashMap};
use clap::Parser;
use rand::{Rng, SeedableRng};
use tokio::{net::{TcpListener, TcpStream}, stream::StreamExt, sync::RwLock, time};

/// Contains information parsed from the command-line invocation of balancebeam. The Parser macros
/// provide a fancy way to automatically construct a command-line argument parser.
#[derive(Parser, Debug)]
#[clap(help = "Fun with load balancing")]
struct CmdOptions {
    #[clap(
        short,
        long,
        help = "IP/port to bind to",
        default_value = "0.0.0.0:1100"
    )]
    bind: String,
    #[clap(short, long, help = "Upstream host to forward requests to")]
    upstream: Vec<String>,
    #[clap(
        long,
        help = "Perform active health checks on this interval (in seconds)",
        default_value = "10"
    )]
    active_health_check_interval: usize,
    #[clap(
    long,
    help = "Path to send request to for active health checks",
    default_value = "/"
    )]
    active_health_check_path: String,
    #[clap(
        long,
        help = "Maximum number of requests to accept per IP per minute (0 = unlimited)",
        default_value = "0"
    )]
    max_requests_per_minute: usize,
}

/// Contains information about the state of balancebeam (e.g. what servers we are currently proxying
/// to, what servers have failed, rate limiting counts, etc.)
///
/// You should add fields to this struct in later milestones.
struct ProxyState {
    /// How frequently we check whether upstream servers are alive (Milestone 4)
    #[allow(dead_code)]
    active_health_check_interval: usize,
    /// Where we should send requests when doing active health checks (Milestone 4)
    #[allow(dead_code)]
    active_health_check_path: String,
    /// Maximum number of requests an individual IP can make in a minute (Milestone 5)
    #[allow(dead_code)]
    max_requests_per_minute: usize,
    /// Addresses of servers that we are proxying to
    upstream_addresses: Vec<String>,
    /// Whether upstream servers are still alive (true/false), wrapped in a RwLock
    /// NOTE: (alive_cnt, alive_bools), I just ignored the cnt completely
    upstream_alives: RwLock<(usize, Vec<bool>)>,
    /// Counts of requests made by each client IP per minute
    requests_counter: RwLock<HashMap<String, usize>>,
}

#[tokio::main]
async fn main() {
    // Initialize the logging library. You can print log messages using the `log` macros:
    // https://docs.rs/log/0.4.8/log/ You are welcome to continue using print! statements; this
    // just looks a little prettier.
    if let Err(_) = std::env::var("RUST_LOG") {
        std::env::set_var("RUST_LOG", "debug");
    }
    pretty_env_logger::init();

    // Parse the command line arguments passed to this program
    let options = CmdOptions::parse();
    let n_upstream = options.upstream.len();
    if n_upstream < 1 {
        log::error!("At least one upstream server must be specified using the --upstream option.");
        std::process::exit(1);
    }

    // Start listening for connections
    let mut listener = match TcpListener::bind(&options.bind).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Could not bind to {}: {}", options.bind, err);
            std::process::exit(1);
        }
    };
    log::info!("Listening for requests on {}", options.bind);

    // Handle incoming connections
    let state = ProxyState {
        upstream_alives: RwLock::new((n_upstream, vec![true; n_upstream])),
        upstream_addresses: options.upstream,
        active_health_check_interval: options.active_health_check_interval,
        active_health_check_path: options.active_health_check_path,
        max_requests_per_minute: options.max_requests_per_minute,
        requests_counter: RwLock::new(HashMap::new()),
    };
    // NOTE: Arc here should be fine, the hang of program is not caused by deadlock
    let shared_state = Arc::new(state);
    // Actively check health of all upstreams per interval
    let shared_state_ref = shared_state.clone();
    tokio::spawn(async move {
        loop {
            time::delay_for(time::Duration::from_secs(shared_state_ref.active_health_check_interval as u64)).await;
            active_health_check(&shared_state_ref).await;
        }
    });
    // Reset rate with fixed window per minute
    if options.max_requests_per_minute > 0 {
        let shared_state_ref = shared_state.clone();
        tokio::spawn(async move {
            loop {
                time::delay_for(time::Duration::from_secs(60)).await;
                reset_rate_fixed_window(&shared_state_ref).await;
            }
        });
    }
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await{
        if let Ok(stream) = stream {
            // Handle the connection!
            handle_connection(stream, &shared_state).await;
        }
    }
}

async fn pick_known_alive_upstream(state: &ProxyState) -> Option<usize> {
    let mut rng = rand::rngs::StdRng::from_entropy();
    let alives_read = state.upstream_alives.read().await;
    while alives_read.0 > 0 {  // NOTE: infinite loop happens when I don't check alive_cnt
        let upstream_idx = rng.gen_range(0, state.upstream_addresses.len());
        if alives_read.1[upstream_idx] {
            return Some(upstream_idx);
        }
    }
    None
    // NOTE: I just ignored the alive_cnt stored in the lock
    // so there was infinite loop when all upstreams are known to be dead
}

async fn connect_to_upstream(state: &ProxyState) -> Result<TcpStream, std::io::Error> {
    loop {  // NOTE: we need the loop since the picked upstream that are known to alive can actually be down
        let upstream_idx = pick_known_alive_upstream(state).await.ok_or("No upstream alive").unwrap();
        let upstream_ip = &state.upstream_addresses[upstream_idx];
        match TcpStream::connect(upstream_ip).await {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                log::error!("Failed to connect to upstream {}: {}", upstream_ip, err);
                let mut alives_write = state.upstream_alives.write().await;
                if alives_write.1[upstream_idx] == true {  // NOTE: check before modify since we have more than one writer
                    alives_write.1[upstream_idx] = false;
                    alives_write.0 -= 1;
                }
            }
        }
    }
}

async fn send_response(client_conn: &mut TcpStream, response: &http::Response<Vec<u8>>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("{} <- {}", client_ip, response::format_response_line(&response));
    if let Err(error) = response::write_to_stream(&response, client_conn).await {
        log::warn!("Failed to send response to client: {}", error);
        return;
    }
}

async fn handle_connection(mut client_conn: TcpStream, state: &ProxyState) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("Connection received from {}", client_ip);

    // Open a connection to a random destination server
    let mut upstream_conn = match connect_to_upstream(state).await {
        Ok(stream) => stream,
        Err(_error) => {
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
    };
    // NOTE: the starter code had a typo here, making upstream_ip same as client_ip
    // luckily upstream_ip is just used for log and does not affect the correctness of the program
    let upstream_ip = upstream_conn.peer_addr().unwrap().ip().to_string();

    // The client may now send us one or more requests. Keep trying to read requests until the
    // client hangs up or we get an error.
    loop {
        // Read a request from the client
        let mut request = match request::read_from_stream(&mut client_conn).await {
            Ok(request) => request,
            // Handle case where client closed connection and is no longer sending requests
            Err(request::Error::IncompleteRequest(0)) => {
                log::debug!("Client finished sending requests. Shutting down connection");
                return;
            }
            // Handle I/O error in reading from the client
            Err(request::Error::ConnectionError(io_err)) => {
                log::info!("Error reading request from client stream: {}", io_err);
                return;
            }
            Err(error) => {
                log::debug!("Error parsing request: {:?}", error);
                let response = response::make_http_error(match error {
                    request::Error::IncompleteRequest(_)
                    | request::Error::MalformedRequest(_)
                    | request::Error::InvalidContentLength
                    | request::Error::ContentLengthMismatch => http::StatusCode::BAD_REQUEST,
                    request::Error::RequestBodyTooLarge => http::StatusCode::PAYLOAD_TOO_LARGE,
                    request::Error::ConnectionError(_) => http::StatusCode::SERVICE_UNAVAILABLE,
                });
                send_response(&mut client_conn, &response).await;
                continue;
            }
        };

        // Respond 429 if client hit the rate limit
        // NOTE: respond here after reading request completely otherwise the test fails sometimes
        if state.max_requests_per_minute > 0 {
            let mut request_counter = state.requests_counter.write().await;
            let cnt = request_counter.entry(client_ip.clone()).or_insert(0);
            *cnt += 1;
            if *cnt > state.max_requests_per_minute {
                log::warn!("Too many requests from {}, {} exceeding rate limit {}", &client_ip, *cnt, state.max_requests_per_minute);
                let response = response::make_http_error(http::StatusCode::TOO_MANY_REQUESTS);
                send_response(&mut client_conn, &response).await;
                return;
            }
        }

        log::info!(
            "{} -> {}: {}",
            client_ip,
            upstream_ip,
            request::format_request_line(&request)
        );

        // Add X-Forwarded-For header so that the upstream server knows the client's IP address.
        // (We're the ones connecting directly to the upstream server, so without this header, the
        // upstream server will only know our IP, not the client's.)
        request::extend_header_value(&mut request, "x-forwarded-for", &client_ip);

        // Forward the request to the server
        if let Err(error) = request::write_to_stream(&request, &mut upstream_conn).await {
            log::error!("Failed to send request to upstream {}: {}", upstream_ip, error);
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
        log::debug!("Forwarded request to server");

        // Read the server's response
        let response = match response::read_from_stream(&mut upstream_conn, request.method()).await {
            Ok(response) => response,
            Err(error) => {
                log::error!("Error reading response from server: {:?}", error);
                let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
                send_response(&mut client_conn, &response).await;
                return;
            }
        };
        // Forward the response to the client
        send_response(&mut client_conn, &response).await;
        log::debug!("Forwarded response to client");
    }
}

async fn active_health_check(state: &ProxyState) {
    for (idx, upstream ) in state.upstream_addresses.iter().enumerate() {
        let req = http::Request::builder()
            .method(http::Method::GET)
            .uri(&state.active_health_check_path)
            .header("Host", upstream)
            .body(Vec::<u8>::new())
            .unwrap();
        match TcpStream::connect(upstream).await {
            Ok(mut stream) => {
                match request::write_to_stream(&req, &mut stream).await {
                    Ok(_) => {
                        match response::read_from_stream(&mut stream, req.method()).await {
                            Ok(response) => {
                                match response.status() {
                                    http::StatusCode::OK => {  // NOTE: 200 OK is not 202 Accepted
                                        // NOTE: don't forget to bring upstream alive
                                        let mut alives_write = state.upstream_alives.write().await;
                                        if alives_write.1[idx] == false {
                                            log::info!("Upstream {} returns OK again", upstream);
                                            alives_write.1[idx] = true;
                                            alives_write.0 += 1;
                                        }
                                    },
                                    status => {
                                        log::info!("Upstream {} returns {} instead of OK", upstream, status);
                                        let mut alives_write = state.upstream_alives.write().await;
                                        if alives_write.1[idx] == true {
                                            alives_write.1[idx] = false;
                                            alives_write.0 -= 1;
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                log::info!("Fail to read response from {} : {:?}", upstream, err);
                            }
                        }
                    },
                    Err(err) => log::error!("Failed to send request {:?} to upstream {}: {:?}", req, upstream, err)
                }
            },
            Err(err) => {
                log::error!("Failed to connect to upstream {}: {}", upstream, err);
                continue;
            }
        }
    }
}

async fn reset_rate_fixed_window (state: &ProxyState) {
    let mut requests_write = state.requests_counter.write().await;
    requests_write.clear();
}