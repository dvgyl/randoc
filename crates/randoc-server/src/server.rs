use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use randoc_core::{Error, Result};

use crate::http::{Deadline, Request, RequestReader, Response, write_response};
use crate::{ServerConfig, ServerLimits, routes};

const CONNECTION_WORKERS: usize = 64;
const MIN_WRITE_TIME: Duration = Duration::from_secs(5);
const CONVERSION_WORKERS: usize = 4;
const WRITE_GRACE: Duration = Duration::from_millis(100);
const ACCEPT_INTERVAL: Duration = Duration::from_millis(5);
const MAX_PREPARATION_WAIT: Duration = Duration::from_secs(5);

/// A listening server that stops and joins its workers on shutdown or drop.
pub struct ServerHandle {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    task: Option<JoinHandle<io::Result<()>>>,
}

impl ServerHandle {
    /// Returns the address on which the server listens.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops accepting connections and waits for accepted requests to finish.
    pub fn shutdown(mut self) -> io::Result<()> {
        self.join()
    }

    fn join(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            task.thread().unpark();
            task.join()
                .map_err(|_| io::Error::other("The server thread stopped unexpectedly."))??;
        }
        Ok(())
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

/// Starts a server in the background with the default byte limits.
pub fn bind_server(config: &ServerConfig) -> io::Result<ServerHandle> {
    bind_server_with_limits(config, ServerLimits::default())
}

/// Starts a server in the background with the specified byte limits.
pub fn bind_server_with_limits(
    config: &ServerConfig,
    limits: ServerLimits,
) -> io::Result<ServerHandle> {
    let listener = listen(config)?;
    let addr = listener.local_addr()?;
    let timeout = Duration::from_secs(config.timeout_secs);
    let service = Service::new(limits, timeout)?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stop);
    let task = thread::Builder::new()
        .name("randoc-server".into())
        .spawn(move || service.accept(listener, timeout, &signal))?;
    Ok(ServerHandle {
        addr,
        stop,
        task: Some(task),
    })
}

/// Listens for HTTP requests and writes the startup line to standard error.
///
/// Returns if the listener or a server setup operation fails.
pub fn run_server(config: &ServerConfig) -> Result<()> {
    run(config).map_err(|error| Error::App(error.to_string()))
}

fn run(config: &ServerConfig) -> io::Result<()> {
    let listener = listen(config)?;
    let timeout = Duration::from_secs(config.timeout_secs);
    let service = Service::new(ServerLimits::default(), timeout)?;
    {
        let mut stderr = io::stderr().lock();
        writeln!(
            stderr,
            "Starting server on port {}...",
            listener.local_addr()?.port()
        )?;
        stderr.flush()?;
    }
    service.accept(listener, timeout, &AtomicBool::new(false))
}

fn listen(config: &ServerConfig) -> io::Result<TcpListener> {
    let listener = TcpListener::bind((config.bind_addr.as_str(), config.port))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

struct Service {
    connections: Workers<Connection>,
    _conversions: Workers<ConversionJob>,
    _preparation: Option<Preparation>,
}

impl Service {
    fn new(limits: ServerLimits, timeout: Duration) -> io::Result<Self> {
        let preparation = Preparation::start(timeout)?;
        let conversions = Workers::start(
            "randoc-conversion",
            CONVERSION_WORKERS,
            8 << 20,
            move |job| {
                run_conversion(job, |request, deadline| {
                    routes::route(request, limits, deadline)
                })
            },
        )?;
        let jobs = conversions.sender()?.clone();
        let connections = Workers::start(
            "randoc-connection",
            CONNECTION_WORKERS,
            256 << 10,
            move |connection| {
                serve_connection(connection, limits.max_body_bytes, &jobs);
            },
        )?;
        Ok(Self {
            connections,
            _conversions: conversions,
            _preparation: preparation,
        })
    }

    fn accept(self, listener: TcpListener, timeout: Duration, stop: &AtomicBool) -> io::Result<()> {
        let connections = self.connections.sender()?;
        while !stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    let connection = Connection {
                        stream,
                        deadline: Deadline::new(timeout),
                    };
                    if let Err(error) = connections.try_send(connection) {
                        let connection = match error {
                            TrySendError::Full(connection) => connection,
                            TrySendError::Disconnected(_) => {
                                return Err(io::Error::other("Connection workers stopped."));
                            }
                        };
                        send_timeout(&connection.stream, false);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::park_timeout(ACCEPT_INTERVAL)
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

struct Preparation(Option<JoinHandle<()>>);

impl Preparation {
    fn start(timeout: Duration) -> io::Result<Option<Self>> {
        if timeout.is_zero() {
            return Ok(None);
        }
        let (done, result) = mpsc::sync_channel(1);
        let task = thread::Builder::new()
            .name("randoc-prepare".into())
            .stack_size(8 << 20)
            .spawn(move || {
                let _ = done.send(crate::convert::prepare());
            })?;
        let preparation = Self(Some(task));
        // Initialization can continue while the server handles requests
        // that do not use the highlighting registry.
        let wait = timeout.saturating_mul(2).min(MAX_PREPARATION_WAIT);
        match result.recv_timeout(wait) {
            Ok(result) => result.map_err(io::Error::other)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other(
                    "The preparation thread stopped unexpectedly.",
                ));
            }
        }
        Ok(Some(preparation))
    }
}

impl Drop for Preparation {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            let _ = task.join();
        }
    }
}

struct Connection {
    stream: TcpStream,
    deadline: Deadline,
}

struct ConversionJob {
    request: Request,
    deadline: Deadline,
    response: SyncSender<Response>,
}

fn run_conversion(job: ConversionJob, run: impl FnOnce(Request, Deadline) -> Response) {
    let response = if job.deadline.remaining().is_zero() {
        Response::empty(503)
    } else {
        catch_unwind(AssertUnwindSafe(|| run(job.request, job.deadline)))
            .unwrap_or_else(|_| Response::empty(500))
    };
    let _ = job.response.send(response);
}

fn serve_connection(connection: Connection, body_limit: usize, jobs: &SyncSender<ConversionJob>) {
    let Connection { stream, deadline } = connection;
    if deadline.remaining().is_zero() {
        send_timeout(&stream, true);
        return;
    }
    let mut reader = RequestReader::new(&stream, deadline);
    let mut request = match reader.head() {
        Ok(request) => request,
        Err(error) => {
            if let Some(status) = error.status() {
                send(&stream, Response::empty(status), false, deadline, true);
            }
            return;
        }
    };
    let is_head = request.method == "HEAD";
    request.body = match reader.body(&request, body_limit) {
        Ok(body) => body,
        Err(error) => {
            if let Some(status) = error.status() {
                let mut response = Response::empty(status);
                if status == 400 && request.header("origin").is_some() {
                    response.allow_origin();
                }
                send(&stream, response, is_head, deadline, true);
            }
            return;
        }
    };
    let (response, result) = mpsc::sync_channel(1);
    let job = ConversionJob {
        request,
        deadline,
        response,
    };
    let response = match jobs.try_send(job) {
        Ok(()) => result
            .recv_timeout(deadline.remaining())
            .unwrap_or_else(|_| Response::empty(503)),
        Err(_) => Response::empty(503),
    };
    send(&stream, response, is_head, deadline, false);
}

fn send(stream: &TcpStream, response: Response, head: bool, deadline: Deadline, drain: bool) {
    if response.status == 503 {
        send_timeout(stream, drain);
        return;
    }
    let write_deadline = Deadline::new(deadline.remaining().max(MIN_WRITE_TIME));
    finish_response(stream, response, head, write_deadline, drain);
}

fn send_timeout(stream: &TcpStream, drain: bool) {
    finish_response(
        stream,
        Response::empty(503),
        false,
        Deadline::new(WRITE_GRACE),
        drain,
    );
}

fn finish_response(
    stream: &TcpStream,
    response: Response,
    head: bool,
    deadline: Deadline,
    drain: bool,
) {
    let _ = write_response(stream, response, head, deadline);
    let _ = stream.shutdown(Shutdown::Write);
    if drain {
        drain_unread_body(stream);
    }
}

fn drain_unread_body(mut stream: &TcpStream) {
    let deadline = Deadline::new(WRITE_GRACE);
    let mut buffer = [0_u8; 8192];
    let mut bytes_left = 32 << 20;
    while bytes_left != 0 {
        let remaining = deadline.remaining();
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            break;
        }
        let chunk = bytes_left.min(buffer.len());
        match stream.read(&mut buffer[..chunk]) {
            Ok(0) | Err(_) => break,
            Ok(bytes) => bytes_left -= bytes,
        }
    }
}

struct Workers<T> {
    jobs: Option<SyncSender<T>>,
    threads: Vec<JoinHandle<()>>,
}

impl<T: Send + 'static> Workers<T> {
    fn start(
        name: &str,
        count: usize,
        stack_size: usize,
        run: impl Fn(T) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(count);
        let receiver = Arc::new(Mutex::new(receiver));
        let run = Arc::new(run);
        let mut pool = Self {
            jobs: Some(sender),
            threads: Vec::with_capacity(count),
        };
        for index in 0..count {
            let receiver = Arc::clone(&receiver);
            let run = Arc::clone(&run);
            let task = thread::Builder::new()
                .name(format!("{name}-{index}"))
                .stack_size(stack_size)
                .spawn(move || {
                    while let Some(job) = receive(&receiver) {
                        let _ = catch_unwind(AssertUnwindSafe(|| run(job)));
                    }
                })?;
            pool.threads.push(task);
        }
        Ok(pool)
    }

    fn sender(&self) -> io::Result<&SyncSender<T>> {
        self.jobs
            .as_ref()
            .ok_or_else(|| io::Error::other("The worker pool is closed."))
    }
}

fn receive<T>(receiver: &Mutex<Receiver<T>>) -> Option<T> {
    receiver.lock().ok()?.recv().ok()
}

impl<T> Drop for Workers<T> {
    fn drop(&mut self) {
        drop(self.jobs.take());
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn submit(
        pool: &Workers<ConversionJob>,
        target: &str,
        deadline: Deadline,
    ) -> Receiver<Response> {
        let (response, result) = mpsc::sync_channel(1);
        let request = Request::test_get(target);
        pool.sender()
            .unwrap()
            .send(ConversionJob {
                request,
                deadline,
                response,
            })
            .unwrap();
        result
    }

    #[test]
    fn conversion_panic_returns_500_and_the_worker_accepts_the_next_job() {
        let pool = Workers::start("test-conversion", 1, 256 << 10, |job| {
            run_conversion(job, |request, _| {
                assert_ne!(request.target, "/panic");
                Response::empty(200)
            });
        })
        .unwrap();
        let wait = Duration::from_secs(2);
        let first = submit(&pool, "/panic", Deadline::new(wait));
        assert_eq!(first.recv_timeout(wait).unwrap().status, 500);
        let next = submit(&pool, "/version", Deadline::new(wait));
        assert_eq!(next.recv_timeout(wait).unwrap().status, 200);
    }

    #[test]
    fn connection_worker_continues_after_a_panic() {
        let (finished, outcome) = mpsc::sync_channel(1);
        let pool = Workers::start("test-connection", 1, 256 << 10, move |job| {
            assert_ne!(job, 0);
            finished.send(job).unwrap();
        })
        .unwrap();
        pool.sender().unwrap().send(0).unwrap();
        pool.sender().unwrap().send(1).unwrap();
        assert_eq!(outcome.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    }

    #[test]
    fn a_conversion_that_checks_its_deadline_frees_the_worker() {
        let (entered, started) = mpsc::sync_channel(1);
        let pool = Workers::start("test-conversion", 1, 256 << 10, move |job| {
            run_conversion(job, |request, deadline| {
                if request.target == "/slow" {
                    let mut context = randoc_core::Context::pure();
                    context.set_deadline(deadline.instant());
                    entered.send(()).unwrap();
                    while context.check_deadline().is_ok() {
                        thread::park_timeout(Duration::from_millis(1));
                    }
                    return Response::empty(503);
                }
                Response::empty(200)
            });
        })
        .unwrap();
        let wait = Duration::from_secs(2);
        let first = submit(&pool, "/slow", Deadline::new(Duration::from_millis(100)));
        started.recv_timeout(wait).unwrap();
        let next = submit(&pool, "/version", Deadline::new(wait));
        assert_eq!(first.recv_timeout(wait).unwrap().status, 503);
        assert_eq!(next.recv_timeout(wait).unwrap().status, 200);
    }

    #[test]
    fn response_can_be_written_after_the_conversion_deadline() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        send(
            &stream,
            Response::with_body(200, "text/plain", "ready"),
            false,
            Deadline::new(Duration::ZERO),
            false,
        );
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "));
        assert!(response.ends_with("ready"));
    }
}
