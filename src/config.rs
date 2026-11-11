//! Configurações locais deste programa (cada computador tem as suas), editadas
//! na aba Configurações. Ficam em `%APPDATA%\panorama_injetados\config.json`
//! (ou ao lado do executável, se `APPDATA` não existir).

use std::{
    path::PathBuf,
    sync::{LazyLock, RwLock},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const URL_PADRAO: &str = "http://localhost:3000";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Endereço do servidor, sem o `/api`. `None` = usar `BIPLAS_SERVER_URL` ou o padrão.
    pub servidor_url: Option<String>,
    /// Nome de quem usa o programa neste computador. `None` = usuário do Windows.
    pub usuario: Option<String>,
}

static ATUAL: LazyLock<RwLock<Config>> = LazyLock::new(|| RwLock::new(ler().unwrap_or_default()));

/// Arquivo onde as configurações ficam guardadas.
pub fn caminho() -> PathBuf {
    let pasta = match std::env::var_os("APPDATA") {
        Some(p) => PathBuf::from(p).join("panorama_injetados"),
        None => std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    pasta.join("config.json")
}

fn ler() -> Result<Config> {
    let p = caminho();
    if !p.exists() {
        return Ok(Config::default());
    }
    let texto = std::fs::read_to_string(&p).with_context(|| format!("não consegui ler {}", p.display()))?;
    serde_json::from_str(&texto).with_context(|| format!("{} está inválido", p.display()))
}

/// Configurações em uso.
pub fn atual() -> Config {
    ATUAL.read().map(|c| c.clone()).unwrap_or_default()
}

/// Grava no arquivo e passa a usar as novas configurações.
pub fn salvar(nova: Config) -> Result<()> {
    let p = caminho();
    if let Some(pasta) = p.parent() {
        std::fs::create_dir_all(pasta).with_context(|| format!("não consegui criar {}", pasta.display()))?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(&nova)?).with_context(|| format!("não consegui gravar {}", p.display()))?;
    if let Ok(mut c) = ATUAL.write() {
        *c = nova;
    }
    Ok(())
}

/// Endereço do servidor em uso: o das configurações; senão `BIPLAS_SERVER_URL`; senão o padrão.
pub fn url_servidor() -> String {
    match atual().servidor_url.filter(|u| !u.trim().is_empty()) {
        Some(u) => u.trim().trim_end_matches('/').to_string(),
        None => url_padrao(),
    }
}

/// Endereço usado quando as configurações não definem um: `BIPLAS_SERVER_URL` ou o padrão.
pub fn url_padrao() -> String {
    std::env::var("BIPLAS_SERVER_URL")
        .unwrap_or_else(|_| URL_PADRAO.into())
        .trim()
        .trim_end_matches('/')
        .to_string()
}

/// Usuário do Windows (`USERNAME`), usado quando não há nome nas configurações.
pub fn usuario_windows() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "desconhecido".into())
}

/// Nome de quem usa o programa: o das configurações, senão o do Windows.
pub fn usuario() -> String {
    atual().usuario.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()).unwrap_or_else(usuario_windows)
}

/// Nome deste computador na rede.
pub fn maquina() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "desconhecida".into())
}

/// IP deste computador na rede: o da placa usada para chegar ao servidor.
/// Não envia nada: só pergunta ao sistema qual seria a rota. Se o servidor for
/// este mesmo computador (localhost), usa a rota da internet.
pub fn ip_local() -> Option<String> {
    let rota = |destino: &str| -> Option<std::net::IpAddr> {
        let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect(destino).ok()?;
        Some(s.local_addr().ok()?.ip())
    };
    let url = url_servidor();
    let host = url.split_once("://").map_or(url.as_str(), |(_, r)| r).split('/').next().unwrap_or("").to_string();
    let destino = if host.contains(':') { host } else { format!("{host}:80") };
    rota(&destino).filter(|ip| !ip.is_loopback()).or_else(|| rota("8.8.8.8:80")).map(|ip| ip.to_string())
}

/// Confere e arruma o endereço digitado. Vazio = `None` (volta ao padrão).
pub fn normalizar_url(texto: &str) -> Result<Option<String>> {
    let t = texto.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let (esquema, resto) = match t.split_once("://") {
        Some((e, r)) if e.eq_ignore_ascii_case("http") || e.eq_ignore_ascii_case("https") => (e.to_ascii_lowercase(), r),
        Some(_) => bail!("o endereço deve começar com http:// ou https://"),
        None => ("http".to_string(), t),
    };
    let resto = resto.trim_end_matches('/');
    if resto.is_empty() || resto.contains(char::is_whitespace) {
        bail!("endereço inválido");
    }
    if resto.ends_with("/api") {
        bail!("informe o endereço sem o /api no final");
    }
    Ok(Some(format!("{esquema}://{resto}")))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn normaliza_o_endereco() {
        assert_eq!(normalizar_url("  ").unwrap(), None);
        assert_eq!(normalizar_url("192.168.0.10:3000/").unwrap().as_deref(), Some("http://192.168.0.10:3000"));
        assert_eq!(normalizar_url("https://srv:8443").unwrap().as_deref(), Some("https://srv:8443"));
        assert!(normalizar_url("ftp://srv").is_err());
        assert!(normalizar_url("http://srv:3000/api").is_err());
        assert!(normalizar_url("http://").is_err());
    }
}
