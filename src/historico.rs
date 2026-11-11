//! Histórico das ações dos usuários e programas abertos agora, lidos do
//! servidor (`/api/historico` e `/api/clientes`). Quem grava o histórico é o
//! servidor, a cada ação; aqui só se lê e se descreve.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::Value;

use crate::{api::Servidor, colunas, fmt, modelo::Produto};

pub const PROGRAMA: &str = "panorama";
/// Registros por página do histórico.
pub const POR_PAGINA: usize = 100;

#[derive(Debug, Clone, Deserialize)]
pub struct Registro {
    pub id: i64,
    pub em: String,
    pub usuario: Option<String>,
    pub maquina: Option<String>,
    pub ip: Option<String>,
    pub acao: String,
    #[serde(default)]
    pub detalhe: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClienteAtivo {
    pub usuario: Option<String>,
    pub maquina: Option<String>,
    pub ip: Option<String>,
    pub cliente_id: Option<String>,
    pub conectado_em: String,
}

/// Codifica para a URL (percent-encoding UTF-8).
pub fn codificar(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Uma página do histórico do panorama: de `usuario` (ou de todos), antes do id `antes_de`.
pub fn buscar(usuario: Option<&str>, antes_de: Option<i64>) -> Result<Vec<Registro>> {
    let mut caminho = format!("/api/historico?programa={PROGRAMA}&limite={POR_PAGINA}");
    if let Some(u) = usuario {
        caminho.push_str(&format!("&usuario={}", codificar(u)));
    }
    if let Some(id) = antes_de {
        caminho.push_str(&format!("&antes_de={id}"));
    }
    Servidor::new()?.get(&caminho)
}

pub fn usuarios() -> Result<Vec<String>> {
    Servidor::new()?.get(&format!("/api/historico/usuarios?programa={PROGRAMA}"))
}

pub fn ativos() -> Result<Vec<ClienteAtivo>> {
    Servidor::new()?.get(&format!("/api/clientes?programa={PROGRAMA}"))
}

/// Data e hora local, ex.: "11/11/2026 14:05:09". Se não entender, mostra como veio.
pub fn hora_local(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .map(|d| d.with_timezone(&Local).format("%d/%m/%Y %H:%M:%S").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

/// Texto da ação, ex.: "Realizado parcial da OP 1323, item 1 (TAMPA): 10 → 12".
pub fn descrever(r: &Registro, produtos: Option<&HashMap<i64, Produto>>) -> String {
    let d = &r.detalhe;
    let produto = |v: &Value| {
        let cod = v.as_i64().unwrap_or(0);
        produtos.and_then(|p| p.get(&cod)).map_or_else(|| format!("produto {cod}"), |p| p.descricao.clone())
    };
    let qtd = |v: &Value| v.as_f64().map_or_else(|| "—".to_string(), fmt::qtd);
    match r.acao.as_str() {
        "parcial" => {
            let antes = qtd(&d["anterior"]);
            let depois = if d["realizado"].is_null() { "apagado".to_string() } else { qtd(&d["realizado"]) };
            format!(
                "Realizado parcial da OP {}, item {} ({}): {antes} → {depois}",
                d["op"].as_i64().unwrap_or(0),
                d["linha"].as_i64().unwrap_or(0) + 1,
                produto(&d["cod_produto"]),
            )
        }
        "simulacao_criada" => format!("Criou simulação: {} × {}", qtd(&d["quantidade"]), produto(&d["cod_produto"])),
        "simulacao_excluida" => format!("Excluiu simulação: {} × {}", qtd(&d["quantidade"]), produto(&d["cod_produto"])),
        "sync" => {
            let fonte = match d["fonte"].as_str() {
                Some("egestor") => "eGestor",
                Some("trello") => "Trello",
                Some(f) => f,
                None => "?",
            };
            let ja = if d["iniciada"] == false { " (já estava em andamento)" } else { "" };
            format!("Pediu atualização do {fonte}{ja}")
        }
        "colunas" => colunas::descrever_mudanca(d["tabela"].as_str().unwrap_or(""), &d["antes"], &d["depois"]),
        outra => format!("{outra}: {d}"),
    }
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;

    fn reg(acao: &str, detalhe: Value) -> Registro {
        Registro { id: 1, em: String::new(), usuario: None, maquina: None, ip: None, acao: acao.into(), detalhe }
    }

    #[test]
    fn descreve_as_acoes() {
        let p = reg("parcial", json!({"op": 1323, "linha": 0, "cod_produto": 7, "anterior": null, "realizado": 12.0}));
        assert_eq!(descrever(&p, None), "Realizado parcial da OP 1323, item 1 (produto 7): — → 12");
        let p = reg("parcial", json!({"op": 5, "linha": 2, "cod_produto": 7, "anterior": 3.0, "realizado": null}));
        assert!(descrever(&p, None).ends_with("3 → apagado"));
        assert_eq!(descrever(&reg("sync", json!({"fonte": "trello", "iniciada": true})), None), "Pediu atualização do Trello");
        assert!(descrever(&reg("sync", json!({"fonte": "egestor", "iniciada": false})), None).contains("já estava"));
        assert_eq!(
            descrever(&reg("simulacao_criada", json!({"cod_produto": 9, "quantidade": 100.0})), None),
            "Criou simulação: 100 × produto 9"
        );
    }

    #[test]
    fn descreve_mudanca_de_colunas() {
        let antes = json!([["Material", true], ["Codigo", true], ["Unidade", false]]);
        let depois = json!([["Material", true], ["Codigo", false], ["Unidade", true]]);
        let t = descrever(&reg("colunas", json!({"tabela": "necessidades", "antes": antes, "depois": depois})), None);
        assert!(t.starts_with("Colunas de Necessidades: mostrou "), "{t}");
        assert!(t.contains("ocultou "), "{t}");
        assert!(!t.contains("ordem"), "{t}");
    }

    #[test]
    fn codifica_para_url() {
        assert_eq!(codificar("João Silva"), "Jo%C3%A3o%20Silva");
        assert_eq!(codificar("abc-1"), "abc-1");
    }
}
