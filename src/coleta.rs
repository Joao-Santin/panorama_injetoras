//! Coleta dos dados no eGestor + análise. Usado tanto pela interface
//! gráfica quanto pelo modo linha de comando.

use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::Api,
    calculo::{self, Resultado},
    modelo::{self, ItemComposicao, Op, Produto},
    relatorio::{self, Totais},
};

pub const SITUACAO_ABERTA: i64 = 10;

/// Respostas cruas do eGestor (guardadas em cache para abrir o app rápido).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DadosBrutos {
    pub gerado_em: String,
    pub ops: Vec<Value>,
    pub produtos: Vec<Value>,
    /// Relatório `composicoes`. `None` = cache antigo, coletado antes deste campo existir.
    #[serde(default)]
    pub composicoes: Option<Vec<Value>>,
    #[serde(default)]
    pub categorias: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Progresso {
    pub etapa: String,
    /// 0.0 ..= 1.0
    pub fracao: f32,
}

fn prog(etapa: impl Into<String>, fracao: f32) -> Progresso {
    Progresso { etapa: etapa.into(), fracao: fracao.clamp(0.0, 1.0) }
}

pub fn pasta_saida() -> PathBuf {
    PathBuf::from(std::env::var("SAIDA_DIR").unwrap_or_else(|_| "relatorios".into()))
}

fn caminho_cache() -> PathBuf {
    pasta_saida().join("ultima_coleta.json")
}

pub fn salvar_cache(d: &DadosBrutos) -> Result<()> {
    std::fs::create_dir_all(pasta_saida())?;
    std::fs::write(caminho_cache(), serde_json::to_vec(d)?)?;
    Ok(())
}

pub fn carregar_cache() -> Option<DadosBrutos> {
    let bytes = std::fs::read(caminho_cache()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Busca OPs em aberto (com composição) e o cadastro completo de produtos.
/// `raiz` é o endereço do servidor (ex.: `http://localhost:3000`).
pub fn coletar(raiz: &str, progresso: &mut dyn FnMut(Progresso)) -> Result<DadosBrutos> {
    let mut api = Api::new(raiz)?;
    progresso(prog("Conectando ao servidor...", 0.0));
    api.health()?;

    // Composições cadastradas e categorias: chamadas rápidas, feitas antes da parte demorada.
    progresso(prog("Lendo composições cadastradas...", 0.0));
    let filtros = json!({ "codProds": "", "categoria": "", "tags": "" });
    let Value::Array(composicoes) = api.post("/relatorios/composicoes", &filtros)? else {
        bail!("O relatório de composições não devolveu uma lista");
    };
    progresso(prog("Lendo categorias...", 0.0));
    let categorias = api.get_todas_paginas("/categorias", &mut |_, _, _| {})?;

    // 1) Lista de OPs em aberto (0% – 5%)
    let lista = api.get_todas_paginas(&format!("/producao?situacao={SITUACAO_ABERTA}"), &mut |p, u, _| {
        progresso(prog(format!("Listando OPs em aberto (página {p}/{u})"), 0.05 * p as f32 / u as f32))
    })?;
    let mut codigos: Vec<i64> = lista
        .iter()
        .filter(|o| o["situacao"].as_i64() == Some(SITUACAO_ABERTA))
        .filter(|o| o["ativo"].as_i64().unwrap_or(1) == 1)
        .filter_map(|o| o["codigo"].as_i64())
        .collect();
    codigos.sort_unstable();

    // 2) Detalhe de cada OP (5% – 50%)
    let mut ops = Vec::with_capacity(codigos.len());
    let total = codigos.len().max(1);
    for (i, cod) in codigos.iter().enumerate() {
        progresso(prog(
            format!("Lendo composição da OP {cod} ({}/{})", i + 1, codigos.len()),
            0.05 + 0.45 * i as f32 / total as f32,
        ));
        ops.push(api.get(&format!("/producao/{cod}")).with_context(|| format!("OP {cod}"))?);
    }

    // 3) Cadastro de produtos (50% – 97%)
    let mut produtos = api.get_todas_paginas("/produtos", &mut |p, u, n| {
        progresso(prog(
            format!("Lendo cadastro de produtos (página {p}/{u}, {n} produtos)"),
            0.50 + 0.47 * p as f32 / u as f32,
        ))
    })?;

    // 4) Insumos/produtos das OPs que não vieram na listagem (97% – 100%)
    let conhecidos: std::collections::HashSet<i64> =
        produtos.iter().filter_map(|p| p["codigo"].as_i64()).collect();
    let mut faltantes: Vec<i64> = ops
        .iter()
        .filter_map(Op::from_json)
        .flat_map(|o| o.itens)
        .flat_map(|i| std::iter::once(i.cod_produto).chain(i.insumos.into_iter().map(|x| x.cod_insumo)))
        .filter(|c| !conhecidos.contains(c))
        .collect();
    faltantes.sort_unstable();
    faltantes.dedup();
    for cod in faltantes {
        progresso(prog(format!("Buscando produto {cod}"), 0.98));
        if let Ok(v) = api.get(&format!("/produtos/{cod}")) {
            produtos.push(v);
        }
    }

    progresso(prog("Concluído", 1.0));
    Ok(DadosBrutos {
        gerado_em: chrono::Local::now().format("%d/%m/%Y %H:%M").to_string(),
        ops,
        produtos,
        composicoes: Some(composicoes),
        categorias,
    })
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
