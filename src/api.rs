//! Cliente HTTP para o servidor `biplas_server_claude`. O servidor busca os
//! dados no eGestor e no Trello e guarda no banco; este programa só lê o que
//! está lá e grava os próprios dados (colunas, simulações, realizado parcial).

use std::{sync::OnceLock, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{
    Method,
    header::{HeaderMap, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};

/// Endereço do servidor, sem o `/api` (aba Configurações, `BIPLAS_SERVER_URL` ou o padrão).
pub fn url_servidor() -> String {
    crate::config::url_servidor()
}

/// Identifica este programa aberto. O servidor devolve nos avisos de mudança,
/// e assim o programa ignora o aviso das mudanças que ele mesmo fez.
pub fn cliente_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        format!("panorama-{}-{}", std::process::id(), agora.as_nanos())
    })
}

/// Cliente HTTP que já manda o `x-api-key` (quando `BIPLAS_API_KEY` está
/// definida) e o `x-cliente-id`. `None` = sem tempo limite.
pub fn cliente_http(tempo_limite: Option<Duration>) -> Result<reqwest::blocking::Client> {
    let mut headers = HeaderMap::new();
    if let Ok(chave) = std::env::var("BIPLAS_API_KEY") {
        let mut valor = HeaderValue::from_str(chave.trim()).context("BIPLAS_API_KEY tem caracteres inválidos")?;
        valor.set_sensitive(true);
        headers.insert("x-api-key", valor);
    }
    headers.insert("x-cliente-id", HeaderValue::from_str(cliente_id()).expect("id só com ASCII"));
    Ok(reqwest::blocking::Client::builder().timeout(tempo_limite).default_headers(headers).build()?)
}

pub struct Servidor {
    http: reqwest::blocking::Client,
    raiz: String,
}

impl Servidor {
    pub fn new() -> Result<Self> {
        Ok(Self { http: cliente_http(Some(Duration::from_secs(60)))?, raiz: url_servidor() })
    }

    /// GET em `{raiz}{caminho}` (ex.: `/api/sync`).
    pub fn get<T: DeserializeOwned>(&self, caminho: &str) -> Result<T> {
        let resp = self.requisitar(Method::GET, caminho, None::<&()>)?;
        resp.json().with_context(|| format!("Resposta inválida do servidor em {caminho}"))
    }

    /// POST/PUT/DELETE em `{raiz}{caminho}`, com corpo JSON opcional.
    /// Devolve o corpo da resposta, se houver.
    pub fn enviar<C: Serialize>(&self, metodo: Method, caminho: &str, corpo: Option<&C>) -> Result<Option<serde_json::Value>> {
        let resp = self.requisitar(metodo, caminho, corpo)?;
        let texto = resp.text().with_context(|| format!("Resposta incompleta do servidor em {caminho}"))?;
        if texto.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&texto).with_context(|| format!("Resposta inválida do servidor em {caminho}"))?))
    }

    fn requisitar<C: Serialize>(&self, metodo: Method, caminho: &str, corpo: Option<&C>) -> Result<reqwest::blocking::Response> {
        let url = format!("{}{caminho}", self.raiz);
        let mut req = self.http.request(metodo.clone(), &url);
        if let Some(c) = corpo {
            req = req.json(c);
        }
        let resp = req
            .send()
            .with_context(|| format!("Não consegui conectar ao servidor em {}. Ele está rodando?", self.raiz))?;
        let status = resp.status();
        if !status.is_success() {
            // O servidor responde {"erro": "..."}; mostra só a mensagem.
            let corpo = resp.text().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&corpo)
                .ok()
                .and_then(|v| v["erro"].as_str().map(String::from))
                .unwrap_or(corpo);
            bail!("{metodo} {caminho} → {status}: {msg}");
        }
        Ok(resp)
    }
}
