// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

use super::*;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::captions::CaptionPreset;
use crate::config::{Format, VideoInputMode, VideoProvider};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("reelmaestro-{name}-{nonce}"));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug)]
struct Request {
    method: String,
    path: String,
    headers: String,
    body: Vec<u8>,
}

enum Reply {
    Json(u16, Value),
    Transient,
    Bytes(Vec<u8>),
    Disconnect,
}

struct Mock {
    origin: String,
    requests: Arc<Mutex<Vec<Request>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Mock {
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let thread = thread::spawn(move || {
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(10);
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "mock request deadline exceeded");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("mock accept failed: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let request = read_request(&mut stream);
                seen.lock().unwrap().push(request);
                match reply {
                    Reply::Disconnect => {}
                    Reply::Transient => write_response(
                        &mut stream,
                        "503 Service Unavailable",
                        "text/plain",
                        b"GPU warming",
                        &["Retry-After: 1"],
                    ),
                    Reply::Json(code, value) => {
                        let status = match code {
                            200 => "200 OK",
                            201 => "201 Created",
                            202 => "202 Accepted",
                            400 => "400 Bad Request",
                            429 => "429 Too Many Requests",
                            _ => panic!("unsupported mock status"),
                        };
                        write_response(
                            &mut stream,
                            status,
                            "application/json",
                            &serde_json::to_vec(&value).unwrap(),
                            &[],
                        );
                    }
                    Reply::Bytes(bytes) => {
                        write_response(&mut stream, "200 OK", "video/mp4", &bytes, &[])
                    }
                }
            }
        });
        Self {
            origin,
            requests,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            handle.join().unwrap();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Request {
    let mut bytes = Vec::new();
    let mut buf = [0; 4096];
    let header_end = loop {
        let n = stream.read(&mut buf).unwrap();
        assert!(n > 0, "client disconnected before request headers");
        bytes.extend_from_slice(&buf[..n]);
        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let n = stream.read(&mut buf).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
    }
    let mut first = headers.lines().next().unwrap().split_whitespace();
    Request {
        method: first.next().unwrap().into(),
        path: first.next().unwrap().into(),
        headers,
        body: bytes[header_end..header_end + length].to_vec(),
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    extra: &[&str],
) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for line in extra {
        head.push_str(line);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
}

fn cfg(origin: &str, mode: VideoInputMode) -> Config {
    Config {
        api_key: String::new(),
        format: Format::Youtube,
        minutes: 3.0,
        text_model: "unused".into(),
        judge_model: "unused".into(),
        image_model: "unused".into(),
        tts_model: "unused".into(),
        music_model: "unused".into(),
        video_model: "local/minimax-h3".into(),
        video_provider: VideoProvider::Local,
        video_base_url: origin.into(),
        video_api_token: Some("test-token".into()),
        video_input_mode: mode,
        video_seed: 42,
        video_steps: 50,
        video_wait_timeout: 1,
        video_size_explicit: false,
        video_model_explicit: false,
        voice: None,
        video_resolution: "960x544".into(),
        validate_scene: 0,
        whisper_cmd: "unused".into(),
        whisper_model: "unused".into(),
        caption_style: CaptionPreset::Burst,
        caption_font: None,
        no_captions: true,
        no_narration: true,
        scene_seconds: 5.0,
        max_cost: None,
    }
}

fn discovery() -> Value {
    json!({"data":[{"id":"local/minimax-h3","supported_durations":[5,10],
        "supported_sizes":["544x960","960x544"],"local":{"metered_charge":false,
        "steps":{"min":2,"max":100},"input_modes":["frames","text"]}}]})
}

#[test]
fn local_size_tracks_stored_format_unless_explicit() {
    let mut config = cfg("http://127.0.0.1:9", VideoInputMode::Text);
    config.apply_video_format(Format::Reel);
    assert_eq!(config.video_resolution, "544x960");
    config.apply_video_format(Format::Youtube);
    assert_eq!(config.video_resolution, "960x544");
    config.video_size_explicit = true;
    config.video_resolution = "768x768".into();
    config.apply_video_format(Format::Reel);
    assert_eq!(config.video_resolution, "768x768");
    assert_eq!(
        crate::config::video_cost_per_second(&config.video_model, &config.video_resolution),
        0.0
    );
    assert!(!crate::config::video_cost_is_guess(&config.video_model));
}

fn failed(id: &str) -> Value {
    json!({"id":id,"status":"failed","error":{"message":"bounded test failure"},"local":{"gpu":"none"}})
}

fn accepted(id: &str) -> Value {
    json!({"id":id,"status":"queued","polling_url":format!("/jobs/{id}")})
}

fn assert_auth(requests: &[Request]) {
    assert!(requests.iter().all(|r| r
        .headers
        .to_ascii_lowercase()
        .contains("authorization: bearer test-token")));
}

#[tokio::test]
async fn preflight_accepts_cold_worker_and_provider_discovery_contract() {
    let mock = Mock::start(vec![
        Reply::Json(200, json!({"http":"ready","model_ready":false})),
        Reply::Json(200, discovery()),
    ]);
    preflight_local(&cfg(&mock.origin, VideoInputMode::FirstFrame))
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests[0].path, "/health");
    assert_eq!(requests[1].path, "/api/v1/videos/models");
    assert_auth(&requests);
}

#[tokio::test]
async fn local_requests_require_authentication() {
    let mut config = cfg("http://127.0.0.1:9", VideoInputMode::Text);
    config.video_api_token = None;
    let err = preflight_local(&config).await.unwrap_err();
    assert!(err.to_string().contains("authentication is missing"));
}

#[tokio::test]
async fn upload_and_submit_are_exact_and_lost_submit_resumes_without_reupload() {
    let dir = TempDir::new("lost-submit");
    let image = dir.0.join("frame.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\nfixture").unwrap();
    let mock = Mock::start(vec![
        Reply::Json(201, json!({"url":"/uploads/frame"})),
        Reply::Disconnect,
        Reply::Json(202, accepted("job-1")),
        Reply::Json(200, failed("job-1")),
    ]);
    let config = cfg(&mock.origin, VideoInputMode::FirstFrame);
    assert!(
        generate_local_clip(&config, 0, "move", &image, 5, "960x544", &dir.0)
            .await
            .is_err()
    );
    // The retry uses the persisted request and durable key. Finish it with a failed poll so the
    // test is bounded and does not need a GPU.
    assert!(
        generate_local_clip(&config, 0, "move", &image, 5, "960x544", &dir.0)
            .await
            .is_err()
    );
    let requests = mock.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path == "/api/v1/videos/images")
            .count(),
        1
    );
    let submits: Vec<_> = requests
        .iter()
        .filter(|r| r.method == "POST" && r.path == "/api/v1/videos")
        .collect();
    assert_eq!(submits.len(), 2);
    let key = |r: &Request| {
        r.headers
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("idempotency-key:"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(key(submits[0]), key(submits[1]));
    assert_eq!(submits[0].body, submits[1].body);
    let request: Value = serde_json::from_slice(&submits[0].body).unwrap();
    assert_eq!(request["model"], "local/minimax-h3");
    assert_eq!(request["prompt"], "move");
    assert_eq!(request["duration"], 5);
    assert_eq!(request["size"], "960x544");
    assert_eq!(request["seed"], 42);
    assert_eq!(request["generate_audio"], false);
    assert_eq!(request["local"]["export"], "original");
    assert_eq!(
        request["provider"]["options"]["local"]["parameters"]["num_inference_steps"],
        50
    );
    assert_eq!(request["frame_images"][0]["frame_type"], "first_frame");
    assert_eq!(requests[0].body, b"\x89PNG\r\n\x1a\nfixture");
    assert_auth(&requests);
}

#[tokio::test]
async fn transient_plain_text_poll_is_retried_then_failed() {
    let dir = TempDir::new("transient");
    let image = dir.0.join("unused");
    let mock = Mock::start(vec![
        Reply::Json(202, accepted("job-1")),
        Reply::Transient,
        Reply::Json(200, failed("job-1")),
        Reply::Json(200, failed("job-1")),
    ]);
    let err = generate_local_clip(
        &cfg(&mock.origin, VideoInputMode::Text),
        0,
        "move",
        &image,
        5,
        "960x544",
        &dir.0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("bounded test failure"));
    // An accepted manifest resumes directly at its saved polling URL; it must not POST again.
    assert!(generate_local_clip(
        &cfg(&mock.origin, VideoInputMode::Text),
        0,
        "move",
        &image,
        5,
        "960x544",
        &dir.0,
    )
    .await
    .is_err());
    let requests = mock.requests();
    assert_eq!(
        requests.iter().filter(|r| r.path == "/jobs/job-1").count(),
        3
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.method == "POST" && r.path == "/api/v1/videos")
            .count(),
        1
    );
}

#[tokio::test]
async fn completed_job_downloads_and_records_manifest_metadata() {
    let media_dir = TempDir::new("media-fixture");
    let clip = media_dir.0.join("fixture.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=960x544:r=24",
            "-frames:v",
            "124",
            "-an",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&clip)
        .status()
        .expect("ffmpeg must be installed for local-provider contract tests");
    assert!(status.success());
    let bytes = std::fs::read(clip).unwrap();

    let dir = TempDir::new("completed");
    let mock = Mock::start(vec![
        Reply::Json(202, accepted("job-ok")),
        Reply::Json(
            200,
            json!({"id":"job-ok","status":"completed","unsigned_urls":["/content/job-ok"],
                "local":{"seed":42,"steps":50,"frames":124,"width":960,"height":544}}),
        ),
        Reply::Bytes(bytes),
    ]);
    let outcome = generate_local_clip(
        &cfg(&mock.origin, VideoInputMode::Text),
        0,
        "move",
        &dir.0.join("unused"),
        5,
        "960x544",
        &dir.0,
    )
    .await
    .unwrap();
    assert!(
        matches!(outcome, LocalOutcome::Complete(ref path) if path == &dir.0.join("scene-00.mp4"))
    );
    let manifest: LocalManifest =
        serde_json::from_slice(&std::fs::read(dir.0.join("scene-00.video-job.json")).unwrap())
            .unwrap();
    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.provider, "local");
    assert_eq!(manifest.model, "local/minimax-h3");
    assert_eq!(manifest.job_id.as_deref(), Some("job-ok"));
    assert_eq!(manifest.status.as_deref(), Some("completed"));
    assert_eq!(manifest.metadata["frames"], 124);
    assert_eq!(manifest.metadata["width"], 960);
    assert_eq!(manifest.metadata["height"], 544);
    assert_auth(&mock.requests());
}

#[tokio::test]
async fn changed_seed_is_rejected_before_network() {
    let dir = TempDir::new("changed-seed");
    let image = dir.0.join("unused");
    let manifest = LocalManifest {
        version: 1,
        provider: "local".into(),
        provider_origin: "http://127.0.0.1:9".into(),
        model: "local/minimax-h3".into(),
        input_fingerprint: "different".into(),
        idempotency_key: "fixed".into(),
        request: json!({}),
        job_id: None,
        polling_url: None,
        status: Some("prepared".into()),
        metadata: Value::Null,
    };
    atomic_json(&dir.0.join("scene-00.video-job.json"), &manifest).unwrap();
    let mut config = cfg("http://127.0.0.1:9", VideoInputMode::Text);
    config.video_seed += 1;
    let err = generate_local_clip(&config, 0, "move", &image, 5, "960x544", &dir.0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("inputs changed"));
}

#[tokio::test]
async fn changed_endpoint_and_saved_cross_origin_poll_are_rejected_before_network() {
    let dir = TempDir::new("origins");
    let image = dir.0.join("unused");
    let config = cfg("http://127.0.0.1:9", VideoInputMode::Text);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    config.video_input_mode.hash(&mut h);
    "local/minimax-h3".hash(&mut h);
    "move".hash(&mut h);
    5_u32.hash(&mut h);
    "960x544".hash(&mut h);
    config.video_seed.hash(&mut h);
    config.video_steps.hash(&mut h);
    Option::<Vec<u8>>::None.hash(&mut h);
    let manifest = LocalManifest {
        version: 1,
        provider: "local".into(),
        provider_origin: config.video_base_url.clone(),
        model: "local/minimax-h3".into(),
        input_fingerprint: format!("{:016x}", h.finish()),
        idempotency_key: "fixed".into(),
        request: json!({}),
        job_id: Some("job-1".into()),
        polling_url: Some("http://example.com/steal".into()),
        status: Some("accepted".into()),
        metadata: Value::Null,
    };
    atomic_json(&dir.0.join("scene-00.video-job.json"), &manifest).unwrap();
    let err = generate_local_clip(&config, 0, "move", &image, 5, "960x544", &dir.0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cross-origin"));

    let mut changed = config;
    changed.video_base_url = "http://127.0.0.1:8".into();
    let err = generate_local_clip(&changed, 0, "move", &image, 5, "960x544", &dir.0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("endpoint changed"));
}

#[tokio::test]
async fn cross_origin_upload_and_completed_content_urls_are_denied() {
    let upload_dir = TempDir::new("upload-origin");
    let image = upload_dir.0.join("frame.jpg");
    std::fs::write(&image, b"jpeg fixture").unwrap();
    let upload = Mock::start(vec![Reply::Json(
        201,
        json!({"url":"http://example.com/steal"}),
    )]);
    let err = generate_local_clip(
        &cfg(&upload.origin, VideoInputMode::FirstFrame),
        0,
        "move",
        &image,
        5,
        "960x544",
        &upload_dir.0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cross-origin"));

    let content_dir = TempDir::new("content-origin");
    let content = Mock::start(vec![
        Reply::Json(202, accepted("job-1")),
        Reply::Json(
            200,
            json!({"id":"job-1","status":"completed","unsigned_urls":["http://example.com/steal"],"local":{}}),
        ),
    ]);
    let err = generate_local_clip(
        &cfg(&content.origin, VideoInputMode::Text),
        0,
        "move",
        &content_dir.0.join("unused"),
        5,
        "960x544",
        &content_dir.0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cross-origin"));
    assert_eq!(
        content.requests().len(),
        2,
        "must not fetch foreign content"
    );
}

#[tokio::test]
async fn pending_deadline_retains_job_and_changed_inputs_do_not_resubmit() {
    let dir = TempDir::new("pending-deadline");
    let mock = Mock::start(vec![
        Reply::Json(202, accepted("job-pending")),
        Reply::Json(
            200,
            json!({"status":"in_progress","local":{"phase":"denoising"}}),
        ),
        Reply::Json(200, failed("job-pending")),
    ]);
    let mut config = cfg(&mock.origin, VideoInputMode::Text);
    // Bypass CLI's minimum to exercise the deadline without a minute-long test.
    config.video_wait_timeout = 0;
    let image = dir.0.join("unused");
    let result = generate_local_clip(&config, 0, "move", &image, 5, "960x544", &dir.0)
        .await
        .unwrap();
    assert!(matches!(result, LocalOutcome::Pending(_)));
    config.video_seed += 1;
    let error = generate_local_clip(&config, 0, "move", &image, 5, "960x544", &dir.0)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("inputs changed"));
    config.video_seed -= 1;
    assert!(
        generate_local_clip(&config, 0, "move", &image, 5, "960x544", &dir.0)
            .await
            .is_err()
    );
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}

#[tokio::test]
async fn overload_keeps_key_but_independent_runs_get_different_keys() {
    let first = TempDir::new("overload-first");
    let second = TempDir::new("overload-second");
    let mock = Mock::start(vec![
        Reply::Json(429, json!({"error":"queue full"})),
        Reply::Json(429, json!({"error":"queue full"})),
        Reply::Json(429, json!({"error":"queue full"})),
    ]);
    let config = cfg(&mock.origin, VideoInputMode::Text);
    for dir in [&first, &first, &second] {
        assert!(matches!(
            generate_local_clip(
                &config,
                0,
                "move",
                &dir.0.join("unused"),
                5,
                "960x544",
                &dir.0
            )
            .await
            .unwrap(),
            LocalOutcome::Pending(_)
        ));
    }
    let requests = mock.requests();
    let key = |r: &Request| {
        r.headers
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("idempotency-key:"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(key(&requests[0]), key(&requests[1]));
    assert_ne!(key(&requests[0]), key(&requests[2]));
    assert_eq!(requests[0].body, requests[1].body);
}
