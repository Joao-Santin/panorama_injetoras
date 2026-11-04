//! Levantamento de necessidades de materiais (explosão de 1 nível da
//! composição de cada produto das OPs em aberto).

use std::collections::{BTreeSet, HashMap};

use crate::modelo::{Op, Produto};

/// Insumos dessa categoria são horas de máquina/mão de obra, não material.
pub fn eh_mao_de_obra(categoria: &str, descricao: &str) -> bool {
    let c = categoria.to_lowercase();
    c.contains("mão") || c.contains("mao") || descricao.to_uppercase().starts_with("MO -")
}

#[derive(Debug, Clone)]
pub struct Necessidade {
    pub cod_insumo: i64,
    pub codigo_proprio: String,
    pub descricao: String,
    pub categoria: String,
    pub unidade: String,
    pub necessidade: f64,
    pub estoque: f64,
    pub estoque_minimo: f64,
    pub controla_estoque: bool,
    pub custo_unit: f64,
    pub ops: BTreeSet<i64>,
    pub produtos: BTreeSet<i64>,
    /// Tags (palavras-chave) do cadastro do material no eGestor.
    pub tags: Vec<String>,
    pub mao_de_obra: bool,
}

impl Necessidade {
    pub fn saldo(&self) -> f64 {
        self.estoque - self.necessidade
    }
    /// Quanto falta para atender todas as OPs (ignora estoque mínimo).
    pub fn falta(&self) -> f64 {
        (self.necessidade - self.estoque.max(0.0)).max(0.0)
    }
    /// Quanto comprar para atender as OPs e ainda manter o estoque mínimo.
    pub fn comprar_com_minimo(&self) -> f64 {
        (self.necessidade + self.estoque_minimo - self.estoque.max(0.0)).max(0.0)
    }
}

/// Uma linha "OP × produto × insumo" para o detalhamento.
#[derive(Debug, Clone)]
pub struct LinhaDetalhe {
    pub op: i64,
    pub op_tags: String,
    pub dt_inicio: String,
    pub cod_produto: i64,
    pub produto_cod_proprio: String,
    pub produto: String,
    pub quant: f64,
    pub peso_un: f64,
    pub cod_insumo: i64,
    pub insumo_cod_proprio: String,
    pub insumo: String,
    pub categoria: String,
    pub unidade: String,
    pub consumo_un: f64,
    pub perc_perda: f64,
    pub necessidade: f64,
}

pub struct Resultado {
    pub necessidades: Vec<Necessidade>,
    pub detalhe: Vec<LinhaDetalhe>,
    pub alertas: Vec<Alerta>,
}

#[derive(Debug, Clone)]
pub struct Alerta {
    pub gravidade: &'static str,
    pub tipo: &'static str,
    pub op: Option<i64>,
    pub codigo: i64,
    pub descricao: String,
    pub mensagem: String,
}

/// Acima disso por peça, o consumo de hora-máquina/MO é quase certamente erro de cadastro.
const MO_MAX_H_POR_PECA: f64 = 0.5;
/// Tolerância entre consumo de MP (kg/peça) e o peso líquido da peça.
const RAZAO_MP_PESO_MAX: f64 = 1.5;
const RAZAO_MP_PESO_MIN: f64 = 0.7;

fn eh_kg(un: &str) -> bool {
    matches!(un.trim().to_uppercase().as_str(), "KG" | "KGS" | "QUILO")
}

fn verificar(ops: &[Op], produtos: &HashMap<i64, Produto>, necessidades: &[Necessidade]) -> Vec<Alerta> {
    let mut alertas = Vec::new();
    for op in ops {
        for item in &op.itens {
            let prod = produtos.get(&item.cod_produto);
            let peso = prod.map(|p| p.peso_liquido).unwrap_or(0.0);
            let mut kg_por_peca = 0.0;
            let mut tem_kg = false;

            if item.insumos.is_empty() {
                alertas.push(Alerta {
                    gravidade: "ALTA",
                    tipo: "Produto sem composição",
                    op: Some(op.codigo),
                    codigo: item.cod_produto,
                    descricao: item.descricao.clone(),
                    mensagem: "Produto da OP sem insumos cadastrados — nenhuma necessidade calculada para ele.".into(),
                });
            }

            for ins in &item.insumos {
                let un = produtos.get(&ins.cod_insumo).map(|p| p.unidade.as_str()).unwrap_or("");
                if eh_mao_de_obra(&ins.categoria, &ins.descricao) {
                    if ins.qtd_por_un > MO_MAX_H_POR_PECA {
                        alertas.push(Alerta {
                            gravidade: "ALTA",
                            tipo: "Consumo de MO suspeito",
                            op: Some(op.codigo),
                            codigo: item.cod_produto,
                            descricao: item.descricao.clone(),
                            mensagem: format!(
                                "{}: {:.4} h por peça × {} pç = {:.1} h. Verifique a composição.",
                                ins.descricao,
                                ins.qtd_por_un,
                                item.quant,
                                ins.qtd_por_un * item.quant
                            ),
                        });
                    }
                } else if eh_kg(un) {
                    tem_kg = true;
                    kg_por_peca += ins.qtd_por_un;
                }
            }

            if tem_kg && peso > 0.0 {
                let razao = kg_por_peca / peso;
                if !(RAZAO_MP_PESO_MIN..=RAZAO_MP_PESO_MAX).contains(&razao) {
                    alertas.push(Alerta {
                        gravidade: "MÉDIA",
                        tipo: "MP × peso da peça",
                        op: Some(op.codigo),
                        codigo: item.cod_produto,
                        descricao: item.descricao.clone(),
                        mensagem: format!(
                            "Linha de {} pç: consumo de MP {kg_por_peca:.4} kg/pç vs. peso líquido {peso:.4} kg ({:.0}%). \
                             Confira peso ou composição.",
                            item.quant,
                            razao * 100.0
                        ),
                    });
                }
            } else if tem_kg && prod.is_some() {
                alertas.push(Alerta {
                    gravidade: "BAIXA",
                    tipo: "Peça sem peso",
                    op: Some(op.codigo),
                    codigo: item.cod_produto,
                    descricao: item.descricao.clone(),
                    mensagem: format!("Produto sem peso líquido cadastrado (MP na composição: {kg_por_peca:.4} kg/pç)."),
                });
            }
        }
    }

    for n in necessidades.iter().filter(|n| !n.mao_de_obra) {
        if n.estoque < 0.0 {
            alertas.push(Alerta {
                gravidade: "MÉDIA",
                tipo: "Estoque negativo",
                op: None,
                codigo: n.cod_insumo,
                descricao: n.descricao.clone(),
                mensagem: format!(
                    "Estoque {:.3} {} no eGestor (considerado 0 no cálculo da falta).",
                    n.estoque, n.unidade
                ),
            });
        }
        if n.unidade.is_empty() {
            alertas.push(Alerta {
                gravidade: "BAIXA",
                tipo: "Sem unidade",
                op: None,
                codigo: n.cod_insumo,
                descricao: n.descricao.clone(),
                mensagem: "Insumo sem unidade cadastrada.".into(),
            });
        }
        if n.falta() > 0.0 && n.custo_unit <= 0.0 {
            alertas.push(Alerta {
                gravidade: "MÉDIA",
                tipo: "Falta sem custo",
                op: None,
                codigo: n.cod_insumo,
                descricao: n.descricao.clone(),
                mensagem: format!(
                    "Falta {:.3} {} mas o preço de custo está zerado — o custo das faltas fica subestimado{}.",
                    n.falta(),
                    n.unidade,
                    if n.categoria == "Clientes" { " (categoria Clientes: talvez seja fornecido pelo cliente)" } else { "" }
                ),
            });
        }
        if !n.controla_estoque {
            alertas.push(Alerta {
                gravidade: "BAIXA",
                tipo: "Não controla estoque",
                op: None,
                codigo: n.cod_insumo,
                descricao: n.descricao.clone(),
                mensagem: "Material com 'controlar estoque' desligado — o estoque informado pode não ser real.".into(),
            });
        }
    }

    let ordem = |g: &str| match g {
        "ALTA" => 0,
        "MÉDIA" => 1,
        _ => 2,
    };
    alertas.sort_by_key(|a| (ordem(a.gravidade), a.tipo, a.op, a.codigo));
    alertas
}

pub fn calcular(ops: &[Op], produtos: &HashMap<i64, Produto>) -> Resultado {
    let mut mapa: HashMap<i64, Necessidade> = HashMap::new();
    let mut detalhe = Vec::new();

    for op in ops {
        for item in &op.itens {
            let peso_un = produtos.get(&item.cod_produto).map(|p| p.peso_liquido).unwrap_or(0.0);
            for ins in &item.insumos {
                let qtd = item.quant * ins.qtd_por_un;
                let cad = produtos.get(&ins.cod_insumo);
                let unidade = cad.map(|p| p.unidade.clone()).unwrap_or_default();

                let n = mapa.entry(ins.cod_insumo).or_insert_with(|| Necessidade {
                    cod_insumo: ins.cod_insumo,
                    codigo_proprio: ins.codigo_proprio.clone(),
                    descricao: cad.map(|p| p.descricao.clone()).unwrap_or_else(|| ins.descricao.clone()),
                    categoria: ins.categoria.clone(),
                    unidade: unidade.clone(),
                    necessidade: 0.0,
                    estoque: cad.map(|p| p.estoque).unwrap_or(0.0),
                    estoque_minimo: cad.map(|p| p.estoque_minimo).unwrap_or(0.0),
                    controla_estoque: cad.map(|p| p.controlar_estoque).unwrap_or(false),
                    custo_unit: cad.map(|p| p.preco_custo).filter(|c| *c > 0.0).unwrap_or(ins.custo_unit),
                    ops: BTreeSet::new(),
                    produtos: BTreeSet::new(),
                    mao_de_obra: eh_mao_de_obra(&ins.categoria, &ins.descricao),
                    tags: cad.map(|p| p.tags.clone()).unwrap_or_default(),
                });
                n.necessidade += qtd;
                n.ops.insert(op.codigo);
                n.produtos.insert(item.cod_produto);

                detalhe.push(LinhaDetalhe {
                    op: op.codigo,
                    op_tags: op.tags.join(", "),
                    dt_inicio: op.dt_inicio.clone(),
                    cod_produto: item.cod_produto,
                    produto_cod_proprio: item.codigo_proprio.clone(),
                    produto: item.descricao.clone(),
                    quant: item.quant,
                    peso_un,
                    cod_insumo: ins.cod_insumo,
                    insumo_cod_proprio: ins.codigo_proprio.clone(),
                    insumo: ins.descricao.clone(),
                    categoria: ins.categoria.clone(),
                    unidade,
                    consumo_un: ins.qtd_por_un,
                    perc_perda: ins.perc_perda,
                    necessidade: qtd,
                });
            }
        }
    }

    let mut necessidades: Vec<Necessidade> = mapa.into_values().collect();
    // Faltas primeiro (maior valor de falta no topo), depois maior necessidade.
    necessidades.sort_by(|a, b| {
        (b.falta() * b.custo_unit.max(1e-9))
            .total_cmp(&(a.falta() * a.custo_unit.max(1e-9)))
            .then(b.necessidade.total_cmp(&a.necessidade))
    });
    detalhe.sort_by(|a, b| a.op.cmp(&b.op).then(a.cod_produto.cmp(&b.cod_produto)));

    let alertas = verificar(ops, produtos, &necessidades);
    Resultado { necessidades, detalhe, alertas }
}
