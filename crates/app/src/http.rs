//! Cliente HTTP para GPUI, que lo usa para cargar imágenes por URL (splash, íconos, miniaturas de
//! CommunityDragon y Data Dragon). Las descargas van en unos pocos hilos fijos: una grilla de skins
//! pide muchas imágenes a la vez y no queremos abrir un hilo por cada una.
//!
//! Cada imagen se guarda en el disco (image-cache), ya procesada (el splash ya desenfocado): desde
//! la segunda vez sale del disco al tiro, como la caché de Chromium en la versión Electron. Además se
//! pueden precargar al disco las imágenes que se van a ver (`prefetch`), con menos prioridad que las
//! que están en pantalla. Nada de esto ocupa RAM: solo se decodifica lo que se muestra.
use futures::future::BoxFuture;
use futures::FutureExt;
use gpui_kit::http_client::http::HeaderValue;
use gpui_kit::http_client::{AsyncBody, HttpClient, Request, Response, Url};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// CommunityDragon contesta cada imagen en ~0,4 s: con más descargas a la vez una grilla de skins se
/// llena antes.
const WORKERS: usize = 12;
const MAX_BYTES: u64 = 32 << 20;
/// Las imágenes de CommunityDragon "latest" pueden cambiar con un parche: se renuevan al mes.
const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Tope de la caché en el disco; al pasarse se borran las más viejas hasta bajar a CACHE_TRIM_TO.
const CACHE_MAX_BYTES: u64 = 400 << 20;
const CACHE_TRIM_TO: u64 = 250 << 20;

type Done = futures::channel::oneshot::Sender<anyhow::Result<(u16, Vec<u8>)>>;

struct Job {
    method: String,
    uri: String,
    /// None = precarga (solo deja la imagen en el disco).
    done: Option<Done>,
}

#[derive(Default)]
struct Queue {
    /// Lo que se está mostrando: primero, y lo último pedido antes (al buscar otra skin, lo nuevo en
    /// pantalla no espera a lo que se pidió antes y quizás ya no se ve).
    urgent: VecDeque<Job>,
    /// Precargas: cuando no hay nada urgente.
    background: VecDeque<Job>,
}

pub struct ImageHttpClient {
    queue: Arc<(Mutex<Queue>, Condvar)>,
}

static CLIENT: OnceLock<Arc<ImageHttpClient>> = OnceLock::new();

/// Precarga imágenes en el disco (en segundo plano, después de lo que está en pantalla).
pub fn prefetch(urls: impl IntoIterator<Item = String>) {
    if let Some(c) = CLIENT.get() {
        let (lock, cv) = &*c.queue;
        let mut q = lock.lock().unwrap();
        for uri in urls {
            q.background.push_back(Job { method: "GET".into(), uri, done: None });
        }
        cv.notify_all();
    }
}

impl ImageHttpClient {
    /// Crea el cliente (guarda en `cache_dir`) y lo deja disponible para `prefetch`.
    pub fn install(cache_dir: PathBuf) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&cache_dir);
        let dir = cache_dir.clone();
        std::thread::spawn(move || trim_cache(&dir));
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
        let queue: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        for _ in 0..WORKERS {
            let (queue, agent, dir) = (queue.clone(), agent.clone(), cache_dir.clone());
            std::thread::spawn(move || loop {
                let job = {
                    let (lock, cv) = &*queue;
                    let mut q = lock.lock().unwrap();
                    loop {
                        if let Some(j) = q.urgent.pop_back().or_else(|| q.background.pop_front()) {
                            // Si ya nadie la espera (la imagen salió de pantalla), no se baja.
                            if j.done.as_ref().is_some_and(|d| d.is_canceled()) {
                                continue;
                            }
                            break j;
                        }
                        q = cv.wait(q).unwrap();
                    }
                };
                #[cfg(debug_assertions)]
                let started = std::time::Instant::now();
                let res = fetch_cached(&agent, &dir, &job.method, &job.uri);
                // Solo en desarrollo: SV_DEV_IMGLOG anota cada imagen (en pantalla o precarga) y cuánto tardó.
                #[cfg(debug_assertions)]
                if std::env::var("SV_DEV_IMGLOG").is_ok() {
                    let kind = if job.done.is_some() { "pantalla" } else { "precarga" };
                    eprintln!("img {kind} {:.0} ms {}", started.elapsed().as_secs_f64() * 1000., job.uri.rsplit('/').next().unwrap_or(""));
                }
                if let Some(done) = job.done {
                    let _ = done.send(res);
                }
            });
        }
        let client = Arc::new(Self { queue });
        let _ = CLIENT.set(client.clone());
        client
    }
}

fn cache_file(dir: &Path, uri: &str) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut h);
    dir.join(format!("{:016x}.img", h.finish()))
}

/// La imagen desde el disco si está y no es muy vieja; si no, se descarga y se guarda.
fn fetch_cached(agent: &ureq::Agent, dir: &Path, method: &str, uri: &str) -> anyhow::Result<(u16, Vec<u8>)> {
    let file = cache_file(dir, uri);
    let fresh = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < CACHE_MAX_AGE);
    if method == "GET" && fresh {
        if let Ok(bytes) = std::fs::read(&file) {
            return Ok((200, bytes));
        }
    }
    let (status, bytes) = fetch(agent, method, uri)?;
    if method == "GET" && status == 200 && !bytes.is_empty() {
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, &bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    Ok((status, bytes))
}

/// Si la caché pasó el tope, borra las imágenes más viejas.
fn trim_cache(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= CACHE_MAX_BYTES {
        return;
    }
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= CACHE_TRIM_TO {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
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
        let job = Job { method: req.method().as_str().to_string(), uri: req.uri().to_string(), done: Some(done) };
        {
            let (lock, cv) = &*self.queue;
            lock.lock().unwrap().urgent.push_back(job);
            cv.notify_one();
        }
        async move {
            let (status, bytes) = rx.await??;
            Ok(Response::builder().status(status).body(AsyncBody::from(bytes))?)
        }
        .boxed()
    }
}
