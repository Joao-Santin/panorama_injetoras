//! Levantamento de necessidades de materiais (explosão de 1 nível da
//! composição de cada produto das OPs em aberto).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
    pub fn custo_necessidade(&self) -> f64 {
        self.necessidade * self.custo_unit
    }
    pub fn custo_falta(&self) -> f64 {
        self.falta() * self.custo_unit
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

/// Horas de máquina/mão de obra previstas para uma OP.
#[derive(Debug, Clone, PartialEq)]
pub struct HorasOp {
    pub op: i64,
    pub tags: String,
    pub horas: f64,
    /// A OP tem alerta de consumo de mão de obra suspeito.
    pub suspeita: bool,
}

impl Resultado {
    /// Horas de mão de obra de cada OP, da maior para a menor.
    pub fn horas_por_op(&self) -> Vec<HorasOp> {
        let suspeitas: HashSet<i64> =
            self.alertas.iter().filter(|a| a.tipo == TIPO_MO_SUSPEITO).filter_map(|a| a.op).collect();
        let mut por_op: BTreeMap<i64, HorasOp> = BTreeMap::new();
        for d in self.detalhe.iter().filter(|d| eh_mao_de_obra(&d.categoria, &d.insumo)) {
            let e = por_op.entry(d.op).or_insert_with(|| HorasOp {
                op: d.op,
                tags: d.op_tags.clone(),
                horas: 0.0,
                suspeita: suspeitas.contains(&d.op),
            });
            e.horas += d.necessidade;
        }
        let mut lista: Vec<HorasOp> = por_op.into_values().collect();
        lista.sort_by(|x, y| y.horas.total_cmp(&x.horas));
        lista
    }

    /// OPs que usam algum material cujo estoque não cobre a necessidade total.
    pub fn ops_com_falta(&self) -> HashSet<i64> {
        self.necessidades.iter().filter(|n| !n.mao_de_obra && n.falta() > 0.0).flat_map(|n| n.ops.iter().copied()).collect()
    }
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
const TIPO_MO_SUSPEITO: &str = "Consumo de MO suspeito";
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
                            tipo: TIPO_MO_SUSPEITO,
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
            let peso_un = item.peso_un(produtos);
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

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;

    /// `insumos`: (código, categoria, consumo por peça).
    fn op(codigo: i64, quant: f64, insumos: &[(i64, &str, f64)]) -> Op {
        let insumos: Vec<_> = insumos
            .iter()
            .map(|(c, cat, q)| json!({"codInsumo": c, "descricao": format!("I{c}"), "categoria": cat, "quant": q}))
            .collect();
        Op::from_json(&json!({"codigo": codigo, "tags": [format!("T{codigo}")], "produtos": [
            {"codProduto": 1, "descricao": "P", "quant": quant, "insumos": insumos}
        ]}))
        .unwrap()
    }

    /// `produtos`: (código, estoque, custo).
    fn cadastro(produtos: &[(i64, f64, f64)]) -> HashMap<i64, Produto> {
        produtos
            .iter()
            .map(|(c, e, custo)| {
                let p = json!({"codigo": c, "estoque": e, "precoCusto": custo, "controlarEstoque": true, "unidadeTributada": "UN"});
                (*c, Produto::from_json(&p).unwrap())
            })
            .collect()
    }

    #[test]
    fn custos_da_necessidade_e_da_falta() {
        // Precisa de 10, tem 4 a R$ 2: a falta é 6.
        let r = calcular(&[op(1, 10.0, &[(2, "Compras", 1.0)])], &cadastro(&[(1, 0.0, 0.0), (2, 4.0, 2.0)]));
        let n = &r.necessidades[0];
        assert_eq!(n.custo_necessidade(), 20.0);
        assert_eq!(n.custo_falta(), 12.0);
    }

    #[test]
    fn custo_da_falta_e_zero_se_o_estoque_cobre() {
        let r = calcular(&[op(1, 10.0, &[(2, "Compras", 1.0)])], &cadastro(&[(1, 0.0, 0.0), (2, 50.0, 2.0)]));
        assert_eq!(r.necessidades[0].custo_falta(), 0.0);
    }

    #[test]
    fn horas_por_op_somam_so_mao_de_obra_e_marcam_suspeitas() {
        // OP 1: 0,1 h × 10 = 1 h + 0,2 h × 10 = 2 h. OP 2: 1 h × 5 = 5 h, acima do limite por peça.
        let ops = [
            op(1, 10.0, &[(5, "Mão-de-Obra", 0.1), (6, "Mão-de-Obra", 0.2), (2, "Compras", 7.0)]),
            op(2, 5.0, &[(5, "Mão-de-Obra", 1.0)]),
            op(3, 5.0, &[(2, "Compras", 1.0)]),
        ];
        let r = calcular(&ops, &cadastro(&[(1, 0.0, 0.0), (2, 0.0, 1.0), (5, 0.0, 1.0), (6, 0.0, 1.0)]));

        let h = r.horas_por_op();
        assert_eq!(h.iter().map(|x| x.op).collect::<Vec<_>>(), [2, 1], "maior primeiro; OP 3 não tem MO");
        assert!((h[0].horas - 5.0).abs() < 1e-9);
        assert!((h[1].horas - 3.0).abs() < 1e-9);
        assert!(h[0].suspeita);
        assert!(!h[1].suspeita);
        assert_eq!(h[0].tags, "T2");
    }

    #[test]
    fn ops_com_falta_ignoram_mao_de_obra_e_material_com_estoque() {
        let ops = [
            op(1, 10.0, &[(2, "Compras", 1.0)]),            // material 2: precisa de 15, tem 12
            op(2, 5.0, &[(2, "Compras", 1.0)]),             // mesmo material em falta
            op(3, 1.0, &[(3, "Compras", 1.0)]),             // material 3 tem estoque
            op(4, 1.0, &[(5, "Mão-de-Obra", 0.1)]),         // MO sem "estoque" não é falta
        ];
        let r = calcular(&ops, &cadastro(&[(1, 0.0, 0.0), (2, 12.0, 1.0), (3, 9.0, 1.0), (5, 0.0, 1.0)]));
        assert_eq!(r.ops_com_falta(), HashSet::from([1, 2]));
    }
}
