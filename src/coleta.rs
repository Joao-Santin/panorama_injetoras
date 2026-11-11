//! Dados do eGestor lidos do servidor + análise. Usado tanto pela interface
//! gráfica quanto pelo modo linha de comando.
//!
//! Quem busca no eGestor é o servidor (sozinho, de tempos em tempos, ou quando
//! um programa pede com `pedir_atualizacao`); aqui só se lê o que está gravado.

use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::Servidor,
    calculo::{self, Resultado},
    modelo::{self, ItemComposicao, Op, Produto},
    relatorio::{self, Totais},
};

/// Dados do eGestor como o servidor guardou (o JSON de cada registro é o do eGestor).
#[derive(Debug, Clone)]
pub struct DadosBrutos {
    pub gerado_em: String,
    pub ops: Vec<Value>,
    pub produtos: Vec<Value>,
    /// Relatório `composicoes`. `None` = ainda não coletado.
    pub composicoes: Option<Vec<Value>>,
    pub categorias: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Progresso {
    pub etapa: String,
    /// 0.0 ..= 1.0
    pub fracao: f32,
}

pub fn pasta_saida() -> PathBuf {
    PathBuf::from(std::env::var("SAIDA_DIR").unwrap_or_else(|_| "relatorios".into()))
}

#[derive(Debug, Deserialize)]
struct StatusFonte {
    ultima_ok_em: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusSync {
    egestor: StatusFonte,
}

/// "2026-11-10T18:34:39.7Z" → "10/11/2026 15:34" (hora local).
fn data_local(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

/// Lê do servidor as OPs em aberto, o cadastro de produtos, as composições e as
/// categorias. `None` = o servidor ainda não terminou nenhuma sincronização do eGestor.
pub fn carregar(srv: &Servidor) -> Result<Option<DadosBrutos>> {
    let status: StatusSync = srv.get("/api/sync")?;
    let Some(quando) = status.egestor.ultima_ok_em else { return Ok(None) };
    Ok(Some(DadosBrutos {
        gerado_em: data_local(&quando),
        ops: srv.get("/api/dados/egestor/producao?abertas=true")?,
        produtos: srv.get("/api/dados/egestor/produtos")?,
        composicoes: Some(srv.get("/api/dados/egestor/composicoes")?),
        categorias: srv.get("/api/dados/egestor/categorias")?,
    }))
}

/// Pede ao servidor para buscar de novo no eGestor (`"egestor"`) ou no Trello
/// (`"trello"`). Responde na hora; o andamento chega pelos avisos do servidor.
pub fn pedir_atualizacao(srv: &Servidor, fonte: &str) -> Result<()> {
    srv.enviar(Method::POST, &format!("/api/sync/{fonte}"), None::<&()>)?;
    Ok(())
}

/// Tudo que a interface/relatório precisa, já calculado.
pub struct Analise {
    pub gerado_em: String,
    pub ops: Vec<Op>,
    pub produtos: HashMap<i64, Produto>,
    /// Composição cadastrada de cada produto. `None` = ainda não coletada (cache antigo).
    pub composicoes: Option<HashMap<i64, Vec<ItemComposicao>>>,
    pub categorias: HashMap<i64, String>,
    pub res: Resultado,
    pub tot: Totais,
}

impl Analise {
    pub fn de(d: &DadosBrutos) -> Self {
        let mut ops: Vec<Op> = d.ops.iter().filter_map(Op::from_json).collect();
        ops.sort_by_key(|o| o.codigo);
        let produtos: HashMap<i64, Produto> =
            d.produtos.iter().filter_map(Produto::from_json).map(|p| (p.codigo, p)).collect();
        let res = calculo::calcular(&ops, &produtos);
        let tot = relatorio::totais(&ops, &produtos, &res);
        let composicoes = d.composicoes.as_deref().map(modelo::composicoes_de_json);
        let categorias = d
            .categorias
            .iter()
            .filter_map(|c| Some((c["codigo"].as_i64()?, modelo::texto(&c["nome"]))))
            .collect();
        Self { gerado_em: d.gerado_em.clone(), ops, produtos, composicoes, categorias, res, tot }
    }

    /// Nome da categoria de um produto do cadastro.
    pub fn categoria_de(&self, cod_categoria: i64) -> &str {
        match self.categorias.get(&cod_categoria) {
            Some(nome) => nome,
            None if cod_categoria == 0 => "Sem categoria",
            None => "—",
        }
    }

    /// Gera a planilha Excel e devolve o caminho.
    pub fn exportar_excel(&self) -> Result<PathBuf> {
        let dir = pasta_saida();
        std::fs::create_dir_all(&dir)?;
        let nome = format!("necessidades_materiais_{}.xlsx", chrono::Local::now().format("%Y-%m-%d_%H%M%S"));
        let caminho = dir.join(nome);
        relatorio::gerar(
            &caminho.to_string_lossy(),
            &self.gerado_em,
            &self.ops,
            &self.produtos,
            &self.res,
            &self.tot,
        )?;
        Ok(caminho)
    }
}
