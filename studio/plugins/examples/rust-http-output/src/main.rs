//! HTTP Output (Rust example): a Vixen Studio controller type that POSTs its lights' colours as JSON to a URL, at most
//! once per `minIntervalMs` and only when they changed, from a worker thread so the show's frames never wait on the
//! network. That is the shape a cloud-API light needs (slow, rate-limited requests). It speaks plain `http://` with the
//! standard library to stay dependency-free; for `https://` add an HTTP client crate (ureq, say) and declare the API's
//! host in `permissions.network` (`https:api.example.com`).
//!
//! Each POST's body: `{"controller": "<instanceId>", "seq": 12, "lights": [{"r": 255, "g": 0, "b": 0}, …]}`, one light
//! per three outputs in the configured colour order.
//!
//! Build with ./build.sh (cargo build --release, then copies the program to bin/, where the manifest's entry points).
mod vixen_plugin_sdk;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use vixen_plugin_sdk::{triplets, ConfigError, Controller, Frame, Instance, Plugin};

const TIMEOUT: Duration = Duration::from_secs(3);

/// What the `status` tool reports, per controller.
#[derive(Clone, Default)]
struct Report {
    url: String,
    sent: u64,
    failed: u64,
    last: String,
    failing: bool,
}

type Reports = Arc<Mutex<HashMap<String, Report>>>;

/// The newest colours waiting for the worker, and whether it should stop.
#[derive(Default)]
struct Outbox {
    lights: Option<Vec<[u16; 3]>>,
    seq: u64,
    stop: bool,
}

#[derive(Clone)]
struct Settings {
    url: Url,
    interval: Duration,
    order: String,
}

struct HttpOutput {
    settings: Option<Settings>,
    reports: Reports,
    outbox: Arc<(Mutex<Outbox>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    last_sent: Option<Vec<[u16; 3]>>,
}

impl Controller for HttpOutput {
    fn configure(&mut self, instance: &Instance, config: &Value) -> Result<Option<Value>, ConfigError> {
        let url = config["url"].as_str().unwrap_or_default();
        let parsed = Url::parse(url).map_err(|why| ConfigError::new("/url", why))?;
        let settings = Settings {
            url: parsed,
            interval: Duration::from_millis(config["minIntervalMs"].as_u64().unwrap_or(1000)),
            order: config["colorOrder"].as_str().unwrap_or("rgb").to_string(),
        };
        self.reports.lock().unwrap().entry(instance.instance_id.clone()).or_default().url = url.to_string();
        let restart = self.worker.is_some();
        if restart {
            self.stop_worker();
        }
        self.settings = Some(settings);
        if restart {
            self.start_worker(instance);
        }
        Ok(None)
    }

    fn start(&mut self, instance: &Instance) -> Result<(), String> {
        self.start_worker(instance);
        instance.status("running", None);
        Ok(())
    }

    fn stop(&mut self, _instance: &Instance) {
        self.stop_worker();
    }

    fn frame(&mut self, instance: &Instance, frame: &Frame) {
        let order = self.settings.as_ref().map(|s| s.order.as_str()).unwrap_or("rgb");
        let lights = triplets(&frame.levels[..frame.levels.len().min(instance.output_count)], order);
        if self.last_sent.as_ref() == Some(&lights) {
            return;
        }
        self.last_sent = Some(lights.clone());
        let (outbox, wake) = &*self.outbox;
        let mut outbox = outbox.lock().unwrap();
        outbox.lights = Some(lights);
        outbox.seq = frame.seq;
        wake.notify_one();
    }

    fn destroy(&mut self, instance: &Instance) {
        self.stop_worker();
        self.reports.lock().unwrap().remove(&instance.instance_id);
    }
}

impl HttpOutput {
    fn start_worker(&mut self, instance: &Instance) {
        let Some(settings) = self.settings.clone() else { return };
        self.outbox = Arc::default();
        self.last_sent = None;
        let (outbox, reports, instance) = (self.outbox.clone(), self.reports.clone(), instance.clone());
        self.worker = Some(std::thread::spawn(move || post_loop(&settings, &outbox, &reports, &instance)));
    }

    fn stop_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            let (outbox, wake) = &*self.outbox;
            outbox.lock().unwrap().stop = true;
            wake.notify_one();
            let _ = worker.join();
        }
    }
}

/// Sends the newest colours, waiting out the interval between requests.
fn post_loop(settings: &Settings, outbox: &(Mutex<Outbox>, Condvar), reports: &Reports, instance: &Instance) {
    let mut last_post: Option<Instant> = None;
    loop {
        let (lights, seq) = {
            let (lock, wake) = outbox;
            let mut state = lock.lock().unwrap();
            while state.lights.is_none() && !state.stop {
                state = wake.wait(state).unwrap();
            }
            if state.stop {
                return;
            }
            if let Some(wait) = last_post.map(|t| settings.interval.saturating_sub(t.elapsed())).filter(|w| !w.is_zero()) {
                // Newer colours may arrive meanwhile; the request sends whatever is newest when the wait ends.
                state = wake.wait_timeout_while(state, wait, |s| !s.stop).unwrap().0;
                if state.stop {
                    return;
                }
            }
            (state.lights.take().unwrap_or_default(), state.seq)
        };
        last_post = Some(Instant::now());
        let lights: Vec<Value> = lights.iter().map(|[r, g, b]| json!({"r": r, "g": g, "b": b})).collect();
        let body = json!({"controller": instance.instance_id, "seq": seq, "lights": lights}).to_string();
        let outcome = post(&settings.url, &body);
        let mut reports = reports.lock().unwrap();
        let report = reports.entry(instance.instance_id.clone()).or_default();
        match outcome {
            Ok(status) => {
                report.sent += 1;
                report.last = format!("HTTP {status}");
                instance.stats(1, body.len() as u64, 0);
                if std::mem::take(&mut report.failing) {
                    instance.status("running", None);
                }
            }
            Err(why) => {
                report.failed += 1;
                report.last = why.clone();
                report.failing = true;
                instance.stats(0, 0, 1);
                instance.status("unavailable", Some(&why));
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Url {
    host: String,
    port: u16,
    path: String,
}

impl Url {
    fn parse(text: &str) -> Result<Url, String> {
        if text.starts_with("https://") {
            return Err("https needs a TLS library; this example speaks plain http://".into());
        }
        let rest = text.strip_prefix("http://").ok_or("must start with http://")?;
        let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse::<u16>().map_err(|_| "has an invalid port")?),
            None => (authority, 80),
        };
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c)) {
            return Err("needs a host name or IPv4 address".into());
        }
        Ok(Url { host: host.to_string(), port, path })
    }
}

/// One POST; the status code on 2xx, else why not.
fn post(url: &Url, body: &str) -> Result<u16, String> {
    let address = (url.host.as_str(), url.port).to_socket_addrs().map_err(|e| format!("can't resolve {}: {e}", url.host))?.next().ok_or("no address")?;
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT).map_err(|e| format!("can't connect to {address}: {e}"))?;
    stream.set_read_timeout(Some(TIMEOUT)).ok();
    stream.set_write_timeout(Some(TIMEOUT)).ok();
    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nUser-Agent: vixen-rust-http-output/1.0\r\n\r\n{body}",
        url.path,
        url.host,
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("sending failed: {e}"))?;
    let mut head = [0u8; 64];
    let read = stream.read(&mut head).map_err(|e| format!("no answer: {e}"))?;
    let line = String::from_utf8_lossy(&head[..read]);
    let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("the answer isn't HTTP")?;
    if (200..300).contains(&status) {
        Ok(status)
    } else {
        Err(format!("the server answered HTTP {status}"))
    }
}

fn main() {
    let reports: Reports = Arc::default();
    let for_tool = reports.clone();
    Plugin::new()
        .controller(None, move |_| {
            Box::new(HttpOutput { settings: None, reports: reports.clone(), outbox: Arc::default(), worker: None, last_sent: None })
        })
        .tool("status", move |_, _| {
            let reports = for_tool.lock().unwrap();
            if reports.is_empty() {
                return Ok(json!("No HTTP JSON controllers are set up."));
            }
            let lines: Vec<String> = reports
                .iter()
                .map(|(id, r)| format!("{id}: {} — {} sent, {} failed, last: {}", r.url, r.sent, r.failed, if r.last.is_empty() { "nothing yet" } else { &r.last }))
                .collect();
            Ok(json!(lines.join("\n")))
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(Url::parse("http://10.0.0.5:8080/set").unwrap(), Url { host: "10.0.0.5".into(), port: 8080, path: "/set".into() });
        assert_eq!(Url::parse("http://lights.local").unwrap().path, "/");
        assert!(Url::parse("https://api.example.com").unwrap_err().contains("TLS"));
        assert!(Url::parse("ftp://x").is_err());
    }
}
