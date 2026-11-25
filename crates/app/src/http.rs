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

fn fetch(agent: &ureq::Agent, method: &str, uri: &str) -> anyhow::Result<(u16, Vec<u8>)> {
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
