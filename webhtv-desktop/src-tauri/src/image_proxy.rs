use percent_encoding::percent_decode_str;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use tiny_http::{Header, Response};
use tauri::Manager;

const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";

pub struct ImageProxyPort(pub Mutex<Option<u16>>);

fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(25))
            .user_agent(UA)
            .build()
            .expect("image proxy client")
    })
}

pub fn start(app: &tauri::App) {
    let Ok(server) = tiny_http::Server::http("127.0.0.1:0") else {
        return;
    };
    let port = server.server_addr().to_ip().map(|addr| addr.port());
    if let Some(port) = port {
        app.manage(ImageProxyPort(Mutex::new(Some(port))));
    }
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let _ = std::thread::spawn(move || handle(request));
        }
    });
}

#[tauri::command]
pub fn image_proxy_port(state: tauri::State<ImageProxyPort>) -> Option<u16> {
    *state.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn handle(request: tiny_http::Request) {
    let upstream = request
        .url()
        .split('?')
        .nth(1)
        .unwrap_or("")
        .split('&')
        .find_map(|pair| pair.strip_prefix("u="))
        .and_then(|value| percent_decode_str(value).decode_utf8().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| {
            value.starts_with("http://") || value.starts_with("https://")
        });
    let Some(upstream) = upstream else {
        let _ = request.respond(Response::from_string("bad request").with_status_code(400));
        return;
    };

    let fetch: Result<(u16, String, Vec<u8>), String> = (|| {
        let mut response = client()
            .get(&upstream)
            .send()
            .map_err(|error| error.to_string())?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .unwrap_or_else(|| guess_content_type(&upstream));
        let mut body = Vec::new();
        Read::take(&mut response, MAX_IMAGE_BYTES)
            .read_to_end(&mut body)
            .map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        Ok((status, content_type, body))
    })();

    match fetch {
        Ok((status, content_type, bytes)) => {
            let mut response = Response::from_data(bytes)
                .with_status_code(status)
                .with_header(Header::from_bytes(&b"Content-Type"[..], content_type).unwrap_or_else(|_| {
                    Header::from_bytes(&b"Content-Type"[..], &b"image/jpeg"[..]).unwrap()
                }));
            if status == 200 {
                response = response
                    .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap())
                    .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
            }
            let _ = request.respond(response);
        }
        Err(error) => {
            let message = format!("proxy error: {error}");
            let _ = request.respond(Response::from_string(message).with_status_code(502));
        }
    }
}

fn guess_content_type(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" | "svgz" => "image/svg+xml",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => "image/jpeg",
    }
    .to_string()
}
