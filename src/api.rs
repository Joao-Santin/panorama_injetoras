//! Cliente HTTP para o servidor `biplas_server_claude`, que repassa para a API
//! do eGestor (`/api/egestor/*`) e serve o quadro do Trello (`/api/trello/*`).
//! Faz throttling (o eGestor limita a 60 req/min), repete em caso de 429 e
//! trata a paginação.

use std::{
    collections::HashSet,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use reqwest::{
    Method,
    header::{HeaderMap, HeaderValue},
};
use serde_json::Value;

/// Intervalo mínimo entre requisições (60 req/min no eGestor → ~1 por segundo).
const INTERVALO_MIN: Duration = Duration::from_millis(1050);
const MAX_TENTATIVAS: u32 = 6;

/// Endereço do servidor, sem o `/api` (variável `BIPLAS_SERVER_URL`).
pub fn url_servidor() -> String {
    std::env::var("BIPLAS_SERVER_URL")
        .unwrap_or_else(|_| "http://localhost:3000".into())
        .trim_end_matches('/')
        .to_string()
}

/// Cliente HTTP que já manda o `x-api-key` quando `BIPLAS_API_KEY` está definida
/// (o servidor exige o header em `/api/*` se tiver `API_KEY` no `.env` dele).
pub fn cliente_http(tempo_limite: Duration) -> Result<reqwest::blocking::Client> {
    let mut headers = HeaderMap::new();
    if let Ok(chave) = std::env::var("BIPLAS_API_KEY") {
        let mut valor = HeaderValue::from_str(chave.trim()).context("BIPLAS_API_KEY tem caracteres inválidos")?;
        valor.set_sensitive(true);
        headers.insert("x-api-key", valor);
    }
    Ok(reqwest::blocking::Client::builder().timeout(tempo_limite).default_headers(headers).build()?)
}

/// Verifica se o servidor está no ar e com a integração pedida (`"egestor"` ou
/// `"trello"`) configurada, via `GET /health`.
pub fn verificar_servidor(http: &reqwest::blocking::Client, raiz: &str, integracao: &str) -> Result<()> {
    let resp = http
        .get(format!("{raiz}/health"))
        .send()
        .with_context(|| format!("Não consegui conectar ao servidor em {raiz}. Ele está rodando?"))?;
    if !resp.status().is_success() {
        bail!("Servidor respondeu {} em /health", resp.status());
    }
    let v: Value = resp.json().context("Resposta de /health não é JSON")?;
    if v[integracao] == Value::Bool(false) {
        bail!("O servidor em {raiz} está sem a integração \"{integracao}\" configurada (veja o .env dele)");
    }
    Ok(())
}

pub struct Api {
    http: reqwest::blocking::Client,
    raiz: String,
    base: String,
    ultima: Option<Instant>,
}

impl Api {
    /// `raiz` é o endereço do servidor (ex.: `http://localhost:3000`).
    pub fn new(raiz: &str) -> Result<Self> {
        let raiz = raiz.trim_end_matches('/').to_string();
        let base = format!("{raiz}/api/egestor");
        Ok(Self { http: cliente_http(Duration::from_secs(60))?, raiz, base, ultima: None })
    }

    /// Verifica se o servidor está no ar e com o eGestor configurado.
    pub fn health(&self) -> Result<()> {
        verificar_servidor(&self.http, &self.raiz, "egestor")
    }

    /// GET em `{base}{caminho}` (caminho já com query string, se houver).
    pub fn get(&mut self, caminho: &str) -> Result<Value> {
        self.requisitar(Method::GET, caminho, None)
    }

    /// POST em `{base}{caminho}` com corpo JSON (usado pelos relatórios do eGestor).
    pub fn post(&mut self, caminho: &str, corpo: &Value) -> Result<Value> {
        self.requisitar(Method::POST, caminho, Some(corpo))
    }

    fn requisitar(&mut self, metodo: Method, caminho: &str, corpo: Option<&Value>) -> Result<Value> {
        let url = format!("{}{}", self.base, caminho);
        for tentativa in 1..=MAX_TENTATIVAS {
            if let Some(t) = self.ultima {
                let passou = t.elapsed();
                if passou < INTERVALO_MIN {
                    thread::sleep(INTERVALO_MIN - passou);
                }
            }
            self.ultima = Some(Instant::now());

            let mut req = self.http.request(metodo.clone(), &url);
            if let Some(c) = corpo {
                req = req.json(c);
            }
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) if tentativa < MAX_TENTATIVAS => {
                    eprintln!("  ! erro de rede em {caminho} ({e}); tentando de novo...");
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
                Err(e) => return Err(e).with_context(|| format!("{metodo} {url}")),
            };

            let status = resp.status();
            if status.as_u16() == 429 || status.is_server_error() {
                let espera = 10 * tentativa as u64;
                eprintln!("  ! {status} em {caminho}; aguardando {espera}s...");
                thread::sleep(Duration::from_secs(espera));
                continue;
            }
            if !status.is_success() {
                let corpo = resp.text().unwrap_or_default();
                bail!("{metodo} {url} → {status}: {corpo}");
            }
            return resp.json().with_context(|| format!("JSON inválido em {metodo} {url}"));
        }
        bail!("{metodo} {url}: desisti após {MAX_TENTATIVAS} tentativas")
    }

    /// Lê todas as páginas de uma listagem (`data`, `last_page`).
    ///
    /// Observação: o `total`/`last_page` do eGestor conta registros excluídos,
    /// então uma página pode vir com menos itens (ou vazia). Por isso
    /// percorremos até o maior `last_page` visto, sem parar em página vazia,
    /// e removemos duplicados pelo campo `codigo`.
    pub fn get_todas_paginas(
        &mut self,
        caminho: &str,
        progresso: &mut dyn FnMut(u64, u64, usize),
    ) -> Result<Vec<Value>> {
        let sep = if caminho.contains('?') { '&' } else { '?' };
        let mut itens = Vec::new();
        let mut vistos = HashSet::new();
        let mut pagina = 1u64;
        let mut ultima = 1u64;
        loop {
            let v = self.get(&format!("{caminho}{sep}page={pagina}"))?;
            ultima = ultima.max(v["last_page"].as_u64().unwrap_or(pagina));
            if let Some(dados) = v["data"].as_array() {
                for item in dados {
                    let chave = item["codigo"].as_i64();
                    if chave.is_none_or(|c| vistos.insert(c)) {
                        itens.push(item.clone());
                    }
                }
            }
            progresso(pagina, ultima, itens.len());
            if pagina >= ultima {
                break;
            }
            pagina += 1;
        }
        Ok(itens)
    }
}
