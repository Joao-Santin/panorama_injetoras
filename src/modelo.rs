//! Estruturas de dados lidas do eGestor. A leitura é tolerante: campos
//! ausentes viram 0/"" e números que venham como texto são convertidos.

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
        })
    }
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
