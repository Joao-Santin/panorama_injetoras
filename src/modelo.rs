//! Estruturas de dados lidas do eGestor. A leitura é tolerante: campos
//! ausentes viram 0/"" e números que venham como texto são convertidos.

use std::collections::HashMap;

use serde_json::Value;

pub fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().replace(',', ".").parse().unwrap_or(0.0),
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

pub fn texto(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        outro => outro.to_string(),
    }
}

fn tags(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(texto).collect()).unwrap_or_default()
}

/// Os relatórios do eGestor devolvem até os códigos como texto.
fn inteiro(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.trim().parse().ok())
}

#[derive(Debug, Clone)]
pub struct Produto {
    pub codigo: i64,
    pub codigo_proprio: String,
    pub descricao: String,
    pub unidade: String,
    pub estoque: f64,
    pub estoque_minimo: f64,
    pub controlar_estoque: bool,
    pub peso_liquido: f64,
    pub peso_bruto: f64,
    pub preco_custo: f64,
    pub preco_venda: f64,
    pub tipo: String,
    pub tags: Vec<String>,
    /// 0 = produto sem categoria.
    pub cod_categoria: i64,
}

impl Produto {
    pub fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            codigo: v["codigo"].as_i64()?,
            codigo_proprio: texto(&v["codigoProprio"]),
            descricao: texto(&v["descricao"]),
            unidade: texto(&v["unidadeTributada"]),
            estoque: num(&v["estoque"]),
            estoque_minimo: num(&v["estoqueMinimo"]),
            controlar_estoque: v["controlarEstoque"].as_bool().unwrap_or(num(&v["controlarEstoque"]) != 0.0),
            peso_liquido: num(&v["pesoLiquido"]),
            peso_bruto: num(&v["pesoBruto"]),
            preco_custo: num(&v["precoCusto"]),
            preco_venda: num(&v["precoVenda"]),
            tipo: texto(&v["tipoProduto"]),
            tags: tags(&v["tags"]),
            cod_categoria: inteiro(&v["codCategoria"]).unwrap_or(0),
        })
    }
}

/// Um insumo da composição cadastrada de um produto (relatório `composicoes`).
#[derive(Debug, Clone)]
pub struct ItemComposicao {
    pub cod_insumo: i64,
    pub codigo_proprio: String,
    pub descricao: String,
    pub unidade: String,
    /// Consumo por unidade produzida, sem perda.
    pub quant: f64,
    pub perc_perda: f64,
}

impl ItemComposicao {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            cod_insumo: inteiro(&v["codInsumo"])?,
            codigo_proprio: texto(&v["codProprio"]),
            descricao: texto(&v["insumo"]),
            unidade: texto(&v["unidade"]),
            quant: num(&v["quant"]),
            perc_perda: num(&v["pPerda"]),
        })
    }

    pub fn qtd_com_perda(&self) -> f64 {
        self.quant * (1.0 + self.perc_perda / 100.0)
    }
}

/// Agrupa o relatório `composicoes` por produto. O eGestor pode devolver o mesmo
/// produto em várias linhas, cada uma com parte dos insumos (ex.: a resina numa e
/// o master na outra), por isso as linhas são somadas, nunca sobrescritas.
pub fn composicoes_de_json(linhas: &[Value]) -> HashMap<i64, Vec<ItemComposicao>> {
    let mut mapa: HashMap<i64, Vec<ItemComposicao>> = HashMap::new();
    for linha in linhas {
        let Some(codigo) = inteiro(&linha["cod"]) else { continue };
        let itens: Vec<ItemComposicao> = linha["insumos"]
            .as_array()
            .map(|a| a.iter().filter_map(ItemComposicao::from_json).collect())
            .unwrap_or_default();
        if !itens.is_empty() {
            mapa.entry(codigo).or_default().extend(itens);
        }
    }
    mapa
}

#[derive(Debug, Clone)]
pub struct Insumo {
    pub cod_insumo: i64,
    pub codigo_proprio: String,
    pub descricao: String,
    pub categoria: String,
    /// Consumo por unidade produzida, já com perda.
    pub qtd_por_un: f64,
    pub perc_perda: f64,
    pub custo_unit: f64,
}

#[derive(Debug, Clone)]
pub struct ItemOp {
    pub cod_produto: i64,
    pub codigo_proprio: String,
    pub descricao: String,
    pub quant: f64,
    pub insumos: Vec<Insumo>,
}

#[derive(Debug, Clone)]
pub struct Op {
    pub codigo: i64,
    pub tags: Vec<String>,
    pub obs: String,
    pub dt_inicio: String,
    pub dt_entrega: String,
    pub dt_cad: String,
    pub valor_total: f64,
    pub itens: Vec<ItemOp>,
}

impl Op {
    pub fn from_json(v: &Value) -> Option<Self> {
        let itens = v["produtos"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .filter_map(|p| {
                        let insumos = p["insumos"]
                            .as_array()
                            .map(|is| {
                                is.iter()
                                    .filter_map(|i| {
                                        let quant = num(&i["quant"]);
                                        let perda = num(&i["pPerda"]);
                                        let com_perda = num(&i["qntComPerda"]);
                                        Some(Insumo {
                                            cod_insumo: i["codInsumo"].as_i64()?,
                                            codigo_proprio: texto(&i["codigoProprio"]),
                                            descricao: texto(&i["descricao"]),
                                            categoria: texto(&i["categoria"]),
                                            qtd_por_un: if com_perda > 0.0 {
                                                com_perda
                                            } else {
                                                quant * (1.0 + perda / 100.0)
                                            },
                                            perc_perda: perda,
                                            custo_unit: num(&i["custoInsumo"]),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        Some(ItemOp {
                            cod_produto: p["codProduto"].as_i64()?,
                            codigo_proprio: texto(&p["codigoProprio"]),
                            descricao: texto(&p["descricao"]),
                            quant: num(&p["quant"]),
                            insumos,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Some(Self {
            codigo: v["codigo"].as_i64()?,
            tags: tags(&v["tags"]),
            obs: texto(&v["obs"]),
            dt_inicio: texto(&v["dtInicio"]),
            dt_entrega: texto(&v["dtEntrega"]),
            dt_cad: texto(&v["dtCad"]),
            valor_total: num(&v["valorTotal"]),
            itens,
        })
    }
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;

    #[test]
    fn composicao_em_varias_linhas_e_somada() {
        let linhas = [
            json!({"cod": "1004", "produto": "  PORTA CHUPETA", "insumos": [
                {"codInsumo": "50", "insumo": "RESINA", "codProprio": "3000050", "unidade": "KG", "quant": "0.022207", "pPerda": "0.0000"}
            ]}),
            json!({"cod": "1004", "produto": "  PORTA CHUPETA", "insumos": [
                {"codInsumo": "140", "insumo": "MASTER", "codProprio": "3000140", "unidade": "KG", "quant": "0.000453", "pPerda": "20.0000"}
            ]}),
            json!({"cod": "7", "produto": "SEM INSUMOS", "insumos": []}),
        ];
        let mapa = composicoes_de_json(&linhas);

        let itens = &mapa[&1004];
        assert_eq!(itens.iter().map(|i| i.cod_insumo).collect::<Vec<_>>(), [50, 140]);
        assert_eq!(itens[0].qtd_com_perda(), 0.022207);
        assert!((itens[1].qtd_com_perda() - 0.000453 * 1.2).abs() < 1e-12);
        assert!(!mapa.contains_key(&7), "sem insumos = sem composição");
    }
}
