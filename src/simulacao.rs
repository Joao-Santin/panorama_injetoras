//! Simulações: produtos e quantidades que ainda não viraram OP, para prever o
//! consumo futuro de materiais. Nada aqui altera o levantamento real das OPs.
//!
//! O produto simulado é sempre aberto inteiro, pois será fabricado. Um subproduto
//! usa primeiro o estoque que sobra depois das OPs: só desce de nível a parte que
//! deixaria o saldo negativo. Todas as simulações dividem o mesmo estoque.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    calculo::{Necessidade, eh_mao_de_obra},
    coleta::{self, Analise},
    modelo::{ItemComposicao, Produto},
};

const ARQUIVO: &str = "simulacoes.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Simulacao {
    pub id: u64,
    pub cod_produto: i64,
    pub quantidade: f64,
}

/// Carrega do disco; se não existir ou for inválido, começa sem simulações.
pub fn carregar() -> Vec<Simulacao> {
    std::fs::read(coleta::pasta_saida().join(ARQUIVO)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn salvar(simulacoes: &[Simulacao]) -> anyhow::Result<()> {
    std::fs::create_dir_all(coleta::pasta_saida())?;
    std::fs::write(coleta::pasta_saida().join(ARQUIVO), serde_json::to_vec_pretty(simulacoes)?)?;
    Ok(())
}

pub fn proximo_id(simulacoes: &[Simulacao]) -> u64 {
    simulacoes.iter().map(|s| s.id + 1).max().unwrap_or(1)
}

/// Itens que podem ser simulados (têm composição), por código ou nome.
/// Código exato vem primeiro; depois, ordem alfabética.
pub fn buscar_itens<'a>(a: &'a Analise, busca: &str, limite: usize) -> Vec<&'a Produto> {
    let busca = busca.trim().to_lowercase();
    let Some(composicoes) = &a.composicoes else { return Vec::new() };
    if busca.is_empty() {
        return Vec::new();
    }
    let codigo_exato = |p: &Produto| p.codigo.to_string() == busca || p.codigo_proprio.to_lowercase() == busca;
    let mut itens: Vec<&Produto> = a
        .produtos
        .values()
        .filter(|p| composicoes.contains_key(&p.codigo))
        .filter(|p| {
            codigo_exato(p) || p.descricao.to_lowercase().contains(&busca) || p.codigo_proprio.to_lowercase().contains(&busca)
        })
        .collect();
    itens.sort_by(|x, y| {
        codigo_exato(y).cmp(&codigo_exato(x)).then(x.descricao.cmp(&y.descricao)).then(x.codigo.cmp(&y.codigo))
    });
    itens.truncate(limite);
    itens
}

/// O que uma simulação consome de um item.
#[derive(Debug, Clone, PartialEq)]
pub struct Consumo {
    pub codigo: i64,
    pub descricao: String,
    pub unidade: String,
    pub qtd: f64,
    /// Subproduto: tem composição própria.
    pub composto: bool,
    /// Parte de um subproduto que não coube no estoque e desceu de nível.
    pub fabricar: f64,
}

#[derive(Debug, Default)]
pub struct Simulado {
    /// Necessidades recebidas, com `simulada` preenchida e os itens que só as simulações usam.
    pub linhas: Vec<Necessidade>,
    /// O que cada simulação consome. O estoque é usado na ordem da lista: a soma é sempre o
    /// total geral, mas excluir uma simulação pode mudar o que as seguintes precisam fabricar.
    pub por_simulacao: HashMap<u64, Vec<Consumo>>,
}

#[derive(Default)]
struct Demanda {
    total: f64,
    fabricar: f64,
}

struct Explosao<'a> {
    analise: &'a Analise,
    necessidade_ops: HashMap<i64, f64>,
    demandas: HashMap<i64, Demanda>,
    /// Item de composição que originou cada código, para montar as linhas novas.
    totais: BTreeMap<i64, (f64, &'a ItemComposicao)>,
    atual: BTreeMap<i64, Consumo>,
}

impl<'a> Explosao<'a> {
    /// Estoque que sobra depois das OPs.
    fn disponivel(&self, codigo: i64) -> f64 {
        let estoque = self.analise.produtos.get(&codigo).map_or(0.0, |p| p.estoque);
        let ops = self.necessidade_ops.get(&codigo).copied().unwrap_or(0.0);
        // Estoque negativo conta como zero, igual ao cálculo da falta.
        (estoque.max(0.0) - ops).max(0.0)
    }

    fn registrar(&mut self, item: &'a ItemComposicao, qtd: f64, composto: bool, fabricar: f64) {
        self.totais.entry(item.cod_insumo).or_insert((0.0, item)).0 += qtd;
        let c = self.atual.entry(item.cod_insumo).or_insert_with(|| Consumo {
            codigo: item.cod_insumo,
            descricao: item.descricao.clone(),
            unidade: item.unidade.clone(),
            qtd: 0.0,
            composto,
            fabricar: 0.0,
        });
        c.qtd += qtd;
        c.fabricar += fabricar;
    }

    /// Consome os itens de `qtd` unidades de `codigo`. `caminho` evita laços em cadastro circular.
    fn abrir(&mut self, codigo: i64, qtd: f64, caminho: &mut Vec<i64>) {
        let analise = self.analise;
        let Some(composicoes) = &analise.composicoes else { return };
        let Some(itens) = composicoes.get(&codigo) else { return };
        for item in itens {
            let cod = item.cod_insumo;
            if caminho.contains(&cod) {
                continue;
            }
            let demanda = qtd * item.qtd_com_perda();
            if composicoes.contains_key(&cod) {
                let disponivel = self.disponivel(cod);
                let d = self.demandas.entry(cod).or_default();
                d.total += demanda;
                let falta = (d.total - disponivel).max(0.0);
                let novo = (falta - d.fabricar).max(0.0);
                d.fabricar += novo;
                self.registrar(item, demanda, true, novo);
                if novo > 0.0 {
                    caminho.push(cod);
                    self.abrir(cod, novo, caminho);
                    caminho.pop();
                }
            } else {
                let categoria =
                    analise.produtos.get(&cod).map_or("—", |p| analise.categoria_de(p.cod_categoria));
                if !eh_mao_de_obra(categoria, &item.descricao) {
                    self.registrar(item, demanda, false, 0.0);
                }
            }
        }
    }
}

/// Linha de necessidade para um item que nenhuma OP usa.
fn linha_nova(a: &Analise, item: &ItemComposicao) -> Necessidade {
    let cad = a.produtos.get(&item.cod_insumo);
    Necessidade {
        cod_insumo: item.cod_insumo,
        codigo_proprio: item.codigo_proprio.clone(),
        descricao: cad.map_or_else(|| item.descricao.clone(), |p| p.descricao.clone()),
        categoria: cad.map_or("—", |p| a.categoria_de(p.cod_categoria)).to_string(),
        unidade: cad.map_or_else(|| item.unidade.clone(), |p| p.unidade.clone()),
        necessidade: 0.0,
        simulada: 0.0,
        estoque: cad.map_or(0.0, |p| p.estoque),
        estoque_minimo: cad.map_or(0.0, |p| p.estoque_minimo),
        controla_estoque: cad.is_some_and(|p| p.controlar_estoque),
        custo_unit: cad.map_or(0.0, |p| p.preco_custo),
        ops: Default::default(),
        produtos: Default::default(),
        tags: cad.map(|p| p.tags.clone()).unwrap_or_default(),
        mao_de_obra: false,
    }
}

/// Soma as simulações às necessidades das OPs em `base` (o geral ou o grupo de OPs em uso).
pub fn calcular(a: &Analise, base: &[Necessidade], simulacoes: &[Simulacao]) -> Simulado {
    let mut e = Explosao {
        analise: a,
        necessidade_ops: base.iter().map(|n| (n.cod_insumo, n.necessidade)).collect(),
        demandas: HashMap::new(),
        totais: BTreeMap::new(),
        atual: BTreeMap::new(),
    };
    let mut por_simulacao = HashMap::new();
    for s in simulacoes {
        e.abrir(s.cod_produto, s.quantidade, &mut vec![s.cod_produto]);
        let mut consumos: Vec<Consumo> = std::mem::take(&mut e.atual).into_values().collect();
        consumos.sort_by(|x, y| x.descricao.cmp(&y.descricao));
        por_simulacao.insert(s.id, consumos);
    }

    let mut linhas = base.to_vec();
    let posicao: HashMap<i64, usize> = linhas.iter().enumerate().map(|(i, n)| (n.cod_insumo, i)).collect();
    for (cod, (qtd, item)) in e.totais {
        match posicao.get(&cod) {
            Some(&i) => linhas[i].simulada = qtd,
            None => linhas.push(Necessidade { simulada: qtd, ..linha_nova(a, item) }),
        }
    }
    Simulado { linhas, por_simulacao }
}

#[cfg(test)]
mod testes {
    use serde_json::{Value, json};

    use super::*;
    use crate::coleta::DadosBrutos;

    /// (código, descrição, estoque, cód. categoria). Categoria 14 = mão de obra.
    type Prod<'x> = (i64, &'x str, f64, i64);
    /// (pai, [(filho, qtd, perda)])
    type Comp<'x> = (i64, &'x [(i64, f64, f64)]);

    /// `ops`: (nº da OP, quantidade, item consumido por peça da OP).
    fn analise(produtos: &[Prod], composicoes: &[Comp], ops: &[(i64, f64, i64)]) -> Analise {
        let nome = |c: i64| produtos.iter().find(|p| p.0 == c).map_or("", |p| p.1);
        let composicoes: Vec<Value> = composicoes
            .iter()
            .map(|(pai, itens)| {
                let insumos: Vec<Value> = itens
                    .iter()
                    .map(|(f, q, p)| {
                        json!({"codInsumo": f.to_string(), "insumo": nome(*f), "codProprio": format!("P{f}"), "unidade": "UN",
                               "quant": q.to_string(), "pPerda": p.to_string()})
                    })
                    .collect();
                json!({"cod": pai.to_string(), "produto": nome(*pai), "insumos": insumos})
            })
            .collect();
        Analise::de(&DadosBrutos {
            gerado_em: String::new(),
            ops: ops
                .iter()
                .map(|(op, q, insumo)| {
                    json!({"codigo": op, "produtos": [{"codProduto": 999, "quant": q, "insumos": [
                        {"codInsumo": insumo, "descricao": nome(*insumo), "categoria": "Compras", "quant": 1}
                    ]}]})
                })
                .collect(),
            produtos: produtos
                .iter()
                .map(|(c, d, est, cat)| {
                    json!({"codigo": c, "descricao": d, "estoque": est, "codCategoria": cat, "codigoProprio": format!("P{c}"),
                           "unidadeTributada": "UN", "precoCusto": 2.0, "controlarEstoque": true})
                })
                .collect(),
            composicoes: Some(composicoes),
            categorias: vec![json!({"codigo": 14, "nome": "Mão-de-Obra"}), json!({"codigo": 13, "nome": "Compras"})],
        })
    }

    fn sim(id: u64, cod_produto: i64, quantidade: f64) -> Simulacao {
        Simulacao { id, cod_produto, quantidade }
    }

    fn rodar(a: &Analise, sims: &[Simulacao]) -> Simulado {
        calcular(a, &a.res.necessidades, sims)
    }

    fn simulada(s: &Simulado, cod: i64) -> Option<f64> {
        s.linhas.iter().find(|n| n.cod_insumo == cod).map(|n| n.simulada)
    }

    fn perto(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    /// 1 = produto, 2 = subproduto, 3 = matéria-prima. Cada 1 usa 1 do 2; cada 2 usa 0,5 do 3.
    fn com_subproduto(estoque_sub: f64, ops: &[(i64, f64, i64)]) -> Analise {
        analise(
            &[(1, "PRODUTO", 0.0, 13), (2, "SUB", estoque_sub, 13), (3, "MP", 0.0, 13)],
            &[(1, &[(2, 1.0, 0.0)]), (2, &[(3, 0.5, 0.0)])],
            ops,
        )
    }

    #[test]
    fn produto_simulado_e_aberto_com_perda_e_sem_mao_de_obra() {
        // O produto tem 500 em estoque, mas será fabricado: o estoque dele não abate nada.
        let a = analise(
            &[(1, "PRODUTO", 500.0, 13), (3, "MP", 0.0, 13), (4, "MO - HORA", 0.0, 14)],
            &[(1, &[(3, 2.0, 10.0), (4, 0.1, 0.0)])],
            &[],
        );
        let s = rodar(&a, &[sim(1, 1, 100.0)]);

        perto(simulada(&s, 3).unwrap(), 220.0);
        assert_eq!(simulada(&s, 4), None, "mão de obra não é material");
        assert_eq!(simulada(&s, 1), None, "o próprio produto não vira necessidade");
    }

    #[test]
    fn subproduto_com_estoque_nao_desce_de_nivel() {
        let a = com_subproduto(100.0, &[]);
        let s = rodar(&a, &[sim(1, 1, 100.0)]);

        perto(simulada(&s, 2).unwrap(), 100.0);
        assert_eq!(simulada(&s, 3), None, "saldo do subproduto ficou em zero, não negativo");
    }

    #[test]
    fn so_o_que_falta_do_subproduto_desce_de_nivel() {
        let a = com_subproduto(30.0, &[]);
        let s = rodar(&a, &[sim(1, 1, 100.0)]);

        perto(simulada(&s, 2).unwrap(), 100.0);
        perto(simulada(&s, 3).unwrap(), 35.0); // 70 a fabricar × 0,5
        perto(s.por_simulacao[&1].iter().find(|c| c.codigo == 2).unwrap().fabricar, 70.0);
    }

    #[test]
    fn estoque_comprometido_com_ops_nao_serve_para_a_simulacao() {
        // Estoque 100, OPs pedem 80: sobram 20. Simulando 50, faltam 30.
        let a = com_subproduto(100.0, &[(10, 80.0, 2)]);
        let s = rodar(&a, &[sim(1, 1, 50.0)]);

        perto(simulada(&s, 3).unwrap(), 15.0);
        let sub = s.linhas.iter().find(|n| n.cod_insumo == 2).unwrap();
        perto(sub.necessidade, 80.0);
        perto(sub.simulada, 50.0);
        perto(sub.saldo_apos_simulacao(), 50.0);
        perto(sub.saldo_apos_ops_e_simulacao(), -30.0);
    }

    #[test]
    fn falta_que_ja_vem_das_ops_nao_e_cobrada_da_simulacao() {
        // OPs já estouram o estoque em 50, mas a simulação só fabrica o que ela mesma pede.
        let a = com_subproduto(100.0, &[(10, 150.0, 2)]);
        let s = rodar(&a, &[sim(1, 1, 10.0)]);
        perto(simulada(&s, 3).unwrap(), 5.0);
    }

    #[test]
    fn estoque_negativo_conta_como_zero() {
        let a = com_subproduto(-40.0, &[]);
        let s = rodar(&a, &[sim(1, 1, 10.0)]);
        perto(simulada(&s, 3).unwrap(), 5.0);
    }

    #[test]
    fn simulacoes_dividem_o_mesmo_estoque() {
        // Estoque 100: a 1ª usa 60 do estoque; a 2ª usa os 40 restantes e fabrica 20.
        let a = com_subproduto(100.0, &[]);
        let s = rodar(&a, &[sim(1, 1, 60.0), sim(2, 1, 60.0)]);

        perto(simulada(&s, 2).unwrap(), 120.0);
        perto(simulada(&s, 3).unwrap(), 10.0);
        assert_eq!(s.por_simulacao[&1].len(), 1, "a 1ª não chega à matéria-prima");
        let segunda = &s.por_simulacao[&2];
        perto(segunda.iter().find(|c| c.codigo == 2).unwrap().fabricar, 20.0);
        perto(segunda.iter().find(|c| c.codigo == 3).unwrap().qtd, 10.0);
    }

    #[test]
    fn falta_em_um_nivel_so_abre_o_seguinte_pelo_que_falta_nele() {
        // 1 → 2 (sem estoque) → 5 (estoque 10) → 3.
        let a = analise(
            &[(1, "P", 0.0, 13), (2, "S1", 0.0, 13), (5, "S2", 10.0, 13), (3, "MP", 0.0, 13)],
            &[(1, &[(2, 1.0, 0.0)]), (2, &[(5, 1.0, 0.0)]), (5, &[(3, 2.0, 0.0)])],
            &[],
        );
        let s = rodar(&a, &[sim(1, 1, 30.0)]);
        perto(simulada(&s, 5).unwrap(), 30.0);
        perto(simulada(&s, 3).unwrap(), 40.0); // (30 − 10) × 2
    }

    #[test]
    fn mesmo_subproduto_em_dois_ramos_soma_a_demanda_antes_de_abater() {
        // 1 → 2 e 5; 2 e 5 → 6 (estoque 10, cada ramo pede 8) → 3.
        let a = analise(
            &[(1, "P", 0.0, 13), (2, "S1", 0.0, 13), (5, "S2", 0.0, 13), (6, "S3", 10.0, 13), (3, "MP", 0.0, 13)],
            &[(1, &[(2, 1.0, 0.0), (5, 1.0, 0.0)]), (2, &[(6, 8.0, 0.0)]), (5, &[(6, 8.0, 0.0)]), (6, &[(3, 1.0, 0.0)])],
            &[],
        );
        let s = rodar(&a, &[sim(1, 1, 1.0)]);
        perto(simulada(&s, 6).unwrap(), 16.0);
        perto(simulada(&s, 3).unwrap(), 6.0);
    }

    #[test]
    fn linha_de_item_nas_ops_e_mantida_e_item_novo_e_acrescentado() {
        let a = analise(
            &[(1, "P", 0.0, 13), (3, "MP", 7.0, 13), (4, "NOVO", 9.0, 13)],
            &[(1, &[(3, 1.0, 0.0), (4, 1.0, 0.0)])],
            &[(10, 5.0, 3)],
        );
        let s = rodar(&a, &[sim(1, 1, 2.0)]);

        assert_eq!(s.linhas.len(), 2);
        let mp = s.linhas.iter().find(|n| n.cod_insumo == 3).unwrap();
        perto(mp.necessidade, 5.0);
        perto(mp.simulada, 2.0);
        assert!(mp.ops.contains(&10));

        let novo = s.linhas.iter().find(|n| n.cod_insumo == 4).unwrap();
        assert_eq!(novo.descricao, "NOVO");
        perto(novo.necessidade, 0.0);
        perto(novo.estoque, 9.0);
        perto(novo.saldo_apos_ops_e_simulacao(), 7.0);
        perto(novo.falta(), 0.0);
    }

    #[test]
    fn sem_simulacoes_as_linhas_nao_mudam() {
        let a = com_subproduto(1.0, &[(10, 5.0, 2)]);
        let s = rodar(&a, &[]);
        assert_eq!(s.linhas.len(), a.res.necessidades.len());
        assert!(s.linhas.iter().all(|n| n.simulada == 0.0));
        assert!(a.res.necessidades.iter().all(|n| n.simulada == 0.0), "o levantamento real fica intacto");
    }

    #[test]
    fn cadastro_circular_nao_trava() {
        let a = analise(
            &[(1, "A", 0.0, 13), (2, "B", 0.0, 13), (3, "MP", 0.0, 13)],
            &[(1, &[(2, 1.0, 0.0)]), (2, &[(1, 1.0, 0.0), (3, 1.0, 0.0)])],
            &[],
        );
        let s = rodar(&a, &[sim(1, 1, 4.0)]);
        perto(simulada(&s, 3).unwrap(), 4.0);
    }

    #[test]
    fn produto_sem_composicao_nao_consome_nada() {
        let a = com_subproduto(0.0, &[]);
        let s = rodar(&a, &[sim(1, 3, 10.0)]);
        assert!(s.por_simulacao[&1].is_empty());
        assert_eq!(simulada(&s, 3), None);
    }

    #[test]
    fn busca_por_codigo_e_nome_so_com_composicao() {
        let a = analise(
            &[(12, "BOMBA GRANDE", 0.0, 13), (2, "BOMBA PEQUENA", 0.0, 13), (120, "CODIGO 12 NO NOME", 0.0, 13), (3, "BOMBA SEM COMPOSICAO", 0.0, 13)],
            &[(12, &[(3, 1.0, 0.0)]), (2, &[(3, 1.0, 0.0)]), (120, &[(3, 1.0, 0.0)])],
            &[],
        );
        let nomes = |b: &str, lim| buscar_itens(&a, b, lim).iter().map(|p| p.descricao.clone()).collect::<Vec<_>>();

        assert_eq!(nomes("bomba", 10), ["BOMBA GRANDE", "BOMBA PEQUENA"]);
        assert_eq!(nomes("bomba", 1), ["BOMBA GRANDE"]);
        assert_eq!(nomes(" 12 ", 10), ["BOMBA GRANDE", "CODIGO 12 NO NOME"], "código exato primeiro");
        assert_eq!(nomes("p2", 10), ["BOMBA PEQUENA"], "código próprio");
        assert!(nomes("   ", 10).is_empty());
    }

    #[test]
    fn ids_nao_se_repetem_e_arquivo_mantem_o_formato() {
        assert_eq!(proximo_id(&[]), 1);
        assert_eq!(proximo_id(&[sim(7, 1, 1.0), sim(3, 1, 1.0)]), 8);

        let salvo = r#"[{"id":7,"cod_produto":1,"quantidade":2.5}]"#;
        let lista: Vec<Simulacao> = serde_json::from_str(salvo).unwrap();
        assert_eq!(lista, [sim(7, 1, 2.5)]);
    }
}
