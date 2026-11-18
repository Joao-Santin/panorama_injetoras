//! Conta de quem usa o programa (`/api/panorama/contas/*` no servidor).
//!
//! Só entra quem tem conta. A primeira conta criada é a do administrador; as
//! outras esperam o administrador autorizar e abrem só as telas que ele
//! liberar (subtela Permissões, em Configurações).
//!
//! A sessão fica guardada nas configurações deste computador (`token`), para
//! não precisar entrar toda vez. O token vai em `Authorization: Bearer` e o
//! nome da conta em `x-usuario` (histórico, usuários ativos e colunas).

use std::sync::{LazyLock, RwLock};

use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    api::{ErroHttp, Servidor},
    config,
};

/// Telas que o administrador libera, com o nome mostrado. "Geral" (em
/// Configurações) é de todos; "Permissões", só do administrador.
pub const TELAS: [(&str, &str); 8] = [
    ("resumo", "Resumo"),
    ("necessidades", "Necessidades"),
    ("mao_de_obra", "Mão de obra"),
    ("ops", "OPs em aberto"),
    ("alertas", "Alertas"),
    ("produtos", "Produtos"),
    ("historico", "Histórico"),
    ("ativos", "Usuários ativos"),
];

pub fn nome_da_tela(id: &str) -> &str {
    TELAS.iter().find(|(t, _)| *t == id).map_or(id, |(_, n)| n)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Conta {
    pub id: i64,
    pub usuario: String,
    pub admin: bool,
    pub aprovado: bool,
    pub telas: Vec<String>,
    pub criado_em: String,
    #[serde(default)]
    pub aprovado_em: Option<String>,
}

impl Conta {
    /// Pode abrir a tela? O administrador abre todas.
    pub fn pode(&self, tela: &str) -> bool {
        self.admin || self.telas.iter().any(|t| t == tela)
    }
}

#[derive(Deserialize)]
struct Sessao {
    token: String,
    conta: Conta,
}

/// Resultado de "Criar conta".
#[derive(Debug, Clone)]
pub enum Cadastro {
    /// Era a primeira conta: virou administradora e já entrou.
    Entrou(Conta),
    /// Espera o administrador autorizar.
    Pendente,
}

/// Nome da conta em uso (vai no `x-usuario`).
static NOME: LazyLock<RwLock<Option<String>>> = LazyLock::new(RwLock::default);

/// Token da sessão guardada neste computador.
pub fn token() -> Option<String> {
    config::atual().token.filter(|t| !t.is_empty())
}

/// Nome de quem está usando: o da conta; antes de entrar, o usuário do Windows.
pub fn nome_em_uso() -> String {
    NOME.read().ok().and_then(|n| n.clone()).unwrap_or_else(config::usuario_windows)
}

fn guardar(token: Option<String>, conta: Option<&Conta>) -> Result<()> {
    if let Ok(mut n) = NOME.write() {
        *n = conta.map(|c| c.usuario.clone());
    }
    let mut nova = config::atual();
    nova.token = token;
    if let Some(c) = conta {
        nova.ultimo_usuario = Some(c.usuario.clone());
    }
    config::salvar(nova)
}

/// Status HTTP do erro, se foi o servidor que recusou.
fn status(e: &anyhow::Error) -> Option<u16> {
    e.downcast_ref::<ErroHttp>().map(|h| h.status)
}

/// `true` = ainda não há conta nenhuma (a próxima será a do administrador).
pub fn sem_contas() -> Result<bool> {
    let v: serde_json::Value = Servidor::new()?.get("/api/panorama/contas/estado")?;
    Ok(v["tem_contas"] == false)
}

pub fn cadastrar(usuario: &str, senha: &str) -> Result<Cadastro> {
    let corpo = json!({ "usuario": usuario, "senha": senha });
    let r = Servidor::new()?.enviar(Method::POST, "/api/panorama/contas/cadastro", Some(&corpo))?.unwrap_or_default();
    match serde_json::from_value::<Option<Sessao>>(r["sessao"].clone())? {
        Some(s) => {
            guardar(Some(s.token), Some(&s.conta))?;
            Ok(Cadastro::Entrou(s.conta))
        }
        None => Ok(Cadastro::Pendente),
    }
}

pub fn entrar(usuario: &str, senha: &str) -> Result<Conta> {
    let corpo = json!({ "usuario": usuario, "senha": senha });
    let r = Servidor::new()?.enviar(Method::POST, "/api/panorama/contas/entrar", Some(&corpo))?.unwrap_or_default();
    let s: Sessao = serde_json::from_value(r)?;
    guardar(Some(s.token), Some(&s.conta))?;
    Ok(s.conta)
}

/// Confere a sessão guardada e traz a conta com as permissões atuais.
/// `Ok(None)` = não há sessão, ou o servidor a recusou (encerrada, conta
/// apagada ou sem autorização): a sessão guardada é esquecida.
pub fn verificar() -> Result<Option<Conta>> {
    if token().is_none() {
        return Ok(None);
    }
    match Servidor::new()?.get::<Conta>("/api/panorama/contas/eu") {
        Ok(c) => {
            if let Ok(mut n) = NOME.write() {
                *n = Some(c.usuario.clone());
            }
            Ok(Some(c))
        }
        Err(e) if matches!(status(&e), Some(401 | 403)) => {
            guardar(None, None)?;
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Encerra a sessão no servidor (se ele estiver no ar) e a esquece aqui.
pub fn sair() -> Result<()> {
    if token().is_some()
        && let Ok(srv) = Servidor::new()
    {
        let _ = srv.enviar(Method::POST, "/api/panorama/contas/sair", None::<&()>);
    }
    guardar(None, None)
}

// ---- Só o administrador -------------------------------------------------------

pub fn listar() -> Result<Vec<Conta>> {
    Servidor::new()?.get("/api/panorama/contas")
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Alteracao {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aprovado: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telas: Option<Vec<String>>,
}

pub fn alterar(id: i64, a: &Alteracao) -> Result<Conta> {
    let r = Servidor::new()?.enviar(Method::PUT, &format!("/api/panorama/contas/{id}"), Some(a))?.unwrap_or_default();
    Ok(serde_json::from_value(r)?)
}

pub fn excluir(id: i64) -> Result<()> {
    Servidor::new()?.enviar(Method::DELETE, &format!("/api/panorama/contas/{id}"), None::<&()>)?;
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    fn conta(admin: bool, telas: &[&str]) -> Conta {
        Conta {
            id: 1,
            usuario: "x".into(),
            admin,
            aprovado: true,
            telas: telas.iter().map(|t| t.to_string()).collect(),
            criado_em: String::new(),
            aprovado_em: None,
        }
    }

    #[test]
    fn permissoes_das_telas() {
        let c = conta(false, &["resumo", "ops"]);
        assert!(c.pode("ops"));
        assert!(!c.pode("produtos"));
        assert!(conta(true, &[]).pode("produtos"), "o administrador abre todas");
    }

    #[test]
    fn le_a_conta_do_servidor() {
        let c: Conta = serde_json::from_str(
            r#"{"id":3,"usuario":"Ana","admin":false,"aprovado":false,"telas":["ops"],"criado_em":"2026-11-18T12:00:00Z","aprovado_em":null}"#,
        )
        .unwrap();
        assert_eq!((c.usuario.as_str(), c.aprovado, c.telas.len()), ("Ana", false, 1));
    }
}
