//! Cliente HTTP para o gateway `egestor-rust-server` (que repassa para a API
//! do eGestor). Faz throttling (o eGestor limita a 60 req/min), repete em
//! caso de 429 e trata a paginação.

use std::{
    collections::HashSet,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Intervalo mínimo entre requisições (60 req/min no eGestor → ~1 por segundo).
const INTERVALO_MIN: Duration = Duration::from_millis(1050);
const MAX_TENTATIVAS: u32 = 6;

pub struct Api {
    http: reqwest::blocking::Client,
    base: String,
    ultima: Option<Instant>,
}

impl Api {
    pub fn new(base: impl Into<String>) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self { http, base: base.into().trim_end_matches('/').to_string(), ultima: None })
    }

    /// Verifica se o gateway está no ar (`GET /health`, fora do prefixo /api).
    pub fn health(&self) -> Result<()> {
        let raiz = self.base.trim_end_matches("/api");
        let resp = self
            .http
            .get(format!("{raiz}/health"))
            .send()
            .with_context(|| format!("Não consegui conectar ao gateway em {raiz}. Ele está rodando?"))?;
        if !resp.status().is_success() {
            bail!("Gateway respondeu {} em /health", resp.status());
        }
        Ok(())
    }

    /// GET em `{base}{caminho}` (caminho já com query string, se houver).
    pub fn get(&mut self, caminho: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, caminho);
        for tentativa in 1..=MAX_TENTATIVAS {
            if let Some(t) = self.ultima {
                let passou = t.elapsed();
                if passou < INTERVALO_MIN {
                    thread::sleep(INTERVALO_MIN - passou);
                }
            }
            self.ultima = Some(Instant::now());

            let resp = match self.http.get(&url).send() {
                Ok(r) => r,
                Err(e) if tentativa < MAX_TENTATIVAS => {
                    eprintln!("  ! erro de rede em {caminho} ({e}); tentando de novo...");
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
                Err(e) => return Err(e).with_context(|| format!("GET {url}")),
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
                bail!("GET {url} → {status}: {corpo}");
            }
            return resp.json().with_context(|| format!("JSON inválido em GET {url}"));
        }
        bail!("GET {url}: desisti após {MAX_TENTATIVAS} tentativas")
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
