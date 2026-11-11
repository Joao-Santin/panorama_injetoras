//! Avisos do servidor (`GET /api/notificacoes`, Server-Sent Events): andamento
//! das sincronizações e mudanças nos dados gravados por outros programas.
//! É assim que todos os panoramas abertos se atualizam juntos.
//!
//! A conexão fica numa thread própria e é refeita sozinha se cair.

use std::{
    io::{BufRead, BufReader},
    time::Duration,
};

use futures::channel::mpsc;
use serde::Deserialize;

/// Espera antes de tentar reconectar.
const RECONEXAO: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoSync {
    Andamento,
    Concluido,
    Erro,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Aviso {
    /// (Re)conectou: pode ter perdido avisos, então recarregue tudo.
    Conectado,
    /// A conexão caiu ou não abriu; já vai tentar de novo.
    Desconectado(String),
    Sync { fonte: String, estado: EstadoSync, etapa: String, fracao: f32, erro: Option<String> },
    /// Outro programa mudou este recurso (`colunas`, `simulacoes`, `parciais`).
    Dados(String),
}

/// Mensagem como vem do servidor.
#[derive(Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
enum Bruto {
    Conectado,
    Atrasado,
    Sync { fonte: String, estado: EstadoSync, etapa: String, fracao: f32, erro: Option<String> },
    Dados { recurso: String, origem: Option<String> },
}

/// Converte o `data:` de um evento. `None` = ignorar (inclusive os avisos das
/// mudanças feitas por este mesmo programa).
fn interpretar(dados: &str, eu: &str) -> Option<Aviso> {
    Some(match serde_json::from_str::<Bruto>(dados).ok()? {
        Bruto::Conectado | Bruto::Atrasado => Aviso::Conectado,
        Bruto::Sync { fonte, estado, etapa, fracao, erro } => Aviso::Sync { fonte, estado, etapa, fracao, erro },
        Bruto::Dados { origem: Some(o), .. } if o == eu => return None,
        Bruto::Dados { recurso, .. } => Aviso::Dados(recurso),
    })
}

/// Fluxo de avisos do servidor em `raiz` para a `Subscription` da interface.
/// Trocar o endereço cria outro fluxo; este para quando a interface o solta.
#[allow(clippy::ptr_arg)] // `Subscription::run_with` passa `&D`, e aqui D = String
pub fn fluxo(raiz: &String) -> mpsc::UnboundedReceiver<Aviso> {
    let raiz = raiz.clone();
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        loop {
            if let Err(e) = escutar(&raiz, &tx)
                && tx.unbounded_send(Aviso::Desconectado(format!("{e:#}"))).is_err()
            {
                return;
            }
            if tx.is_closed() {
                return;
            }
            std::thread::sleep(RECONEXAO);
        }
    });
    rx
}

/// Fica lendo os avisos até a conexão cair (erro) ou a interface fechar (Ok).
fn escutar(raiz: &str, tx: &mpsc::UnboundedSender<Aviso>) -> anyhow::Result<()> {
    let resp = crate::api::cliente_http(None)?
        .get(format!("{raiz}/api/notificacoes"))
        .send()
        .map_err(|e| anyhow::anyhow!("sem conexão com o servidor em {raiz} ({e})"))?;
    if !resp.status().is_success() {
        anyhow::bail!("o servidor em {raiz} respondeu {} aos avisos", resp.status());
    }

    let eu = crate::api::cliente_id();
    let mut dados = String::new();
    for linha in BufReader::new(resp).lines() {
        let linha = linha?;
        if let Some(resto) = linha.strip_prefix("data:") {
            dados.push_str(resto.strip_prefix(' ').unwrap_or(resto));
        } else if linha.is_empty() && !dados.is_empty() {
            if let Some(aviso) = interpretar(&dados, eu)
                && tx.unbounded_send(aviso).is_err()
            {
                return Ok(());
            }
            dados.clear();
        }
    }
    anyhow::bail!("o servidor encerrou a conexão de avisos")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn le_os_avisos_e_ignora_os_proprios() {
        assert_eq!(interpretar(r#"{"tipo":"conectado"}"#, "eu"), Some(Aviso::Conectado));
        assert_eq!(interpretar(r#"{"tipo":"atrasado"}"#, "eu"), Some(Aviso::Conectado));
        assert_eq!(
            interpretar(r#"{"tipo":"dados","recurso":"simulacoes","origem":"outro"}"#, "eu"),
            Some(Aviso::Dados("simulacoes".into()))
        );
        assert_eq!(interpretar(r#"{"tipo":"dados","recurso":"parciais"}"#, "eu"), Some(Aviso::Dados("parciais".into())));
        assert_eq!(interpretar(r#"{"tipo":"dados","recurso":"parciais","origem":"eu"}"#, "eu"), None);
        assert_eq!(
            interpretar(r#"{"tipo":"sync","fonte":"egestor","estado":"andamento","etapa":"X","fracao":0.5}"#, "eu"),
            Some(Aviso::Sync { fonte: "egestor".into(), estado: EstadoSync::Andamento, etapa: "X".into(), fracao: 0.5, erro: None })
        );
        assert_eq!(interpretar(r#"{"tipo":"novidade"}"#, "eu"), None, "tipo desconhecido é ignorado");
    }
}
