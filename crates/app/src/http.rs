//! Cliente HTTP para GPUI, que lo usa para cargar imágenes por URL (splash, íconos, miniaturas de
//! CommunityDragon y Data Dragon). Las descargas van en unos pocos hilos fijos: una grilla de skins
//! pide muchas imágenes a la vez y no queremos abrir un hilo por cada una.
use futures::future::BoxFuture;
use futures::FutureExt;
use gpui_kit::http_client::http::HeaderValue;
use gpui_kit::http_client::{AsyncBody, HttpClient, Request, Response, Url};
use std::io::Read;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WORKERS: usize = 6;
const MAX_BYTES: u64 = 32 << 20;

type Done = futures::channel::oneshot::Sender<anyhow::Result<(u16, Vec<u8>)>>;

struct Job {
    method: String,
    uri: String,
    done: Done,
}

pub struct ImageHttpClient {
    jobs: Mutex<Sender<Job>>,
}

impl ImageHttpClient {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
        let (tx, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..WORKERS {
            let (rx, agent) = (rx.clone(), agent.clone());
            std::thread::spawn(move || loop {
                let Ok(job) = rx.lock().unwrap().recv() else { return };
                let _ = job.done.send(fetch(&agent, &job.method, &job.uri));
            });
        }
        Self { jobs: Mutex::new(tx) }
    }
}

/// Marca que pide la imagen desenfocada: `?sv_blur=3` (radio en px de pantalla, como el
/// `filter: blur(3px)` de Electron). Se quita antes de pedirla al servidor.
const BLUR_PARAM: &str = "sv_blur=";

/// URL de una imagen que se mostrará desenfocada `radius` px (ver `ImageHttpClient`).
pub fn blurred(url: &str, radius: u32) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{BLUR_PARAM}{radius}")
}

fn split_blur(uri: &str) -> (String, Option<f32>) {
    let Some(at) = uri.find(BLUR_PARAM) else { return (uri.to_string(), None) };
    let radius = uri[at + BLUR_PARAM.len()..].split('&').next().and_then(|r| r.parse().ok());
    let base = uri[..at - 1].to_string(); // sin el ? o & de antes
    (base, radius)
}

/// Desenfoca la imagen. Se achica primero (el desenfoque esconde el detalle igual) y se ajusta el
/// radio a esa escala; un splash de 1280 px se muestra a unos 1200, así que px ≈ px de pantalla.
fn blur_image(bytes: &[u8], radius: f32) -> anyhow::Result<Vec<u8>> {
    const WIDTH: u32 = 960;
    let img = image::load_from_memory(bytes)?.to_rgb8();
    let scale = (WIDTH as f32 / img.width() as f32).min(1.0);
    let small = if scale < 1.0 {
        let h = (img.height() as f32 * scale).round() as u32;
        image::imageops::resize(&img, WIDTH, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    // CSS blur(r) es un gaussiano con desviación r; se escala al tamaño achicado (pantalla ≈ 1200 px).
    let sigma = radius * small.width() as f32 / 1200.0;
    let out = image::imageops::fast_blur(&small, sigma.max(0.5));
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90).encode_image(&out)?;
    Ok(buf)
}

fn fetch(agent: &ureq::Agent, method: &str, uri: &str) -> anyhow::Result<(u16, Vec<u8>)> {
    let (uri, blur) = split_blur(uri);
    let (status, bytes) = download(agent, method, &uri)?;
    match blur {
        Some(r) if status < 400 => Ok((status, blur_image(&bytes, r)?)),
        _ => Ok((status, bytes)),
    }
}

fn download(agent: &ureq::Agent, method: &str, uri: &str) -> anyhow::Result<(u16, Vec<u8>)> {
    let resp = match agent.request(method, uri).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(e.into()),
    };
    let status = resp.status();
    let mut bytes = Vec::new();
    resp.into_reader().take(MAX_BYTES).read_to_end(&mut bytes)?;
    Ok((status, bytes))
}

impl HttpClient for ImageHttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    /// Solo se usa para bajar imágenes: el cuerpo de la petición se ignora.
    fn send(&self, req: Request<AsyncBody>) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let (done, rx) = futures::channel::oneshot::channel();
        let job = Job { method: req.method().as_str().to_string(), uri: req.uri().to_string(), done };
        let sent = self.jobs.lock().unwrap().send(job);
        async move {
            sent.map_err(|_| anyhow::anyhow!("cliente HTTP cerrado"))?;
            let (status, bytes) = rx.await??;
            Ok(Response::builder().status(status).body(AsyncBody::from(bytes))?)
        }
        .boxed()
    }
}
