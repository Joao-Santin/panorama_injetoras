//! Lançamento parcial do realizado das OPs em aberto. O eGestor só baixa o estoque
//! quando a OP é encerrada; até lá, o que já foi produzido consumiu material que o
//! estoque do sistema ainda mostra. Nada aqui altera o levantamento das necessidades.
//!
//! Uma OP pode ter o mesmo produto em várias linhas, com quantidades e composições
//! diferentes, e o eGestor não dá um código às linhas. Por isso o lançamento guarda a
//! posição da linha e o produto: se a linha mudar de produto, ele deixa de valer.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{calculo::Necessidade, coleta, modelo::Op};

const ARQUIVO: &str = "lancamentos_parciais.json";

/// (nº da OP, posição da linha na OP, código do produto)
pub type Chave = (i64, usize, i64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lancamento {
    pub op: i64,
    pub linha: usize,
    pub cod_produto: i64,
    pub realizado: f64,
}

impl Lancamento {
    fn chave(&self) -> Chave {
        (self.op, self.linha, self.cod_produto)
    }
}

/// Carrega do disco; se não existir ou for inválido, começa sem lançamentos.
pub fn carregar() -> Vec<Lancamento> {
    std::fs::read(coleta::pasta_saida().join(ARQUIVO)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn salvar(lancamentos: &[Lancamento]) -> anyhow::Result<()> {
    std::fs::create_dir_all(coleta::pasta_saida())?;
    std::fs::write(coleta::pasta_saida().join(ARQUIVO), serde_json::to_vec_pretty(lancamentos)?)?;
    Ok(())
}

/// Grava o realizado de uma linha (`None` apaga). Devolve se algo mudou.
pub fn definir(lancamentos: &mut Vec<Lancamento>, chave: Chave, realizado: Option<f64>) -> bool {
    let posicao = lancamentos.iter().position(|l| l.chave() == chave);
    match (posicao, realizado) {
        (Some(i), Some(r)) if lancamentos[i].realizado == r => false,
        (Some(i), Some(r)) => {
            lancamentos[i].realizado = r;
            true
        }
        (Some(i), None) => {
            lancamentos.remove(i);
            true
        }
        (None, Some(realizado)) => {
            let (op, linha, cod_produto) = chave;
            lancamentos.push(Lancamento { op, linha, cod_produto, realizado });
            true
        }
        (None, None) => false,
    }
}

fn linhas_abertas(ops: &[Op]) -> impl Iterator<Item = (Chave, &crate::modelo::ItemOp)> {
    ops.iter().flat_map(|o| o.itens.iter().enumerate().map(move |(i, item)| ((o.codigo, i, item.cod_produto), item)))
}

/// Descarta os lançamentos de OPs que não estão mais abertas (ou de linhas que mudaram).
/// Devolve quantos foram descartados.
pub fn manter_validos(lancamentos: &mut Vec<Lancamento>, ops: &[Op]) -> usize {
    let validas: HashSet<Chave> = linhas_abertas(ops).map(|(chave, _)| chave).collect();
    let antes = lancamentos.len();
    lancamentos.retain(|l| validas.contains(&l.chave()));
    antes - lancamentos.len()
}

/// Consumo já realizado de cada insumo: realizado da linha × consumo por peça com perda.
/// Só contam as linhas de `ops`, que são as OPs em aberto.
pub fn consumos(ops: &[Op], lancamentos: &[Lancamento]) -> HashMap<i64, f64> {
    let realizado: HashMap<Chave, f64> = lancamentos.iter().map(|l| (l.chave(), l.realizado)).collect();
    let mut total = HashMap::new();
    for (chave, item) in linhas_abertas(ops) {
        if let Some(r) = realizado.get(&chave) {
            for ins in &item.insumos {
                *total.entry(ins.cod_insumo).or_insert(0.0) += r * ins.qtd_por_un;
            }
        }
    }
    total
}

/// Preenche o consumo parcial das linhas da aba Necessidades.
pub fn aplicar(linhas: &mut [Necessidade], consumos: &HashMap<i64, f64>) {
    for n in linhas {
        n.consumo_parcial = consumos.get(&n.cod_insumo).copied().unwrap_or(0.0);
    }
}

/// Texto dos campos de realizado, a partir do que está salvo. Sem separador de
/// milhares nem arredondamento: lido de volta, dá exatamente o mesmo número.
pub fn textos(lancamentos: &[Lancamento]) -> HashMap<Chave, String> {
    lancamentos.iter().map(|l| (l.chave(), l.realizado.to_string().replace('.', ","))).collect()
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;
    use crate::{calculo, fmt, modelo::Produto};

    /// `linhas`: (produto, quantidade, [(insumo, consumo por peça)]).
    fn op(codigo: i64, linhas: &[(i64, f64, &[(i64, f64)])]) -> Op {
        let produtos: Vec<_> = linhas
            .iter()
            .map(|(p, q, insumos)| {
                let insumos: Vec<_> =
                    insumos.iter().map(|(c, qtd)| json!({"codInsumo": c, "descricao": "I", "categoria": "Compras", "quant": qtd})).collect();
                json!({"codProduto": p, "quant": q, "insumos": insumos})
            })
            .collect();
        Op::from_json(&json!({"codigo": codigo, "produtos": produtos})).unwrap()
    }

    fn lanc(op: i64, linha: usize, cod_produto: i64, realizado: f64) -> Lancamento {
        Lancamento { op, linha, cod_produto, realizado }
    }

    fn perto(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn consumo_e_realizado_vezes_consumo_por_peca_somando_as_ops() {
        let ops = [op(1, &[(10, 100.0, &[(50, 0.5), (51, 2.0)])]), op(2, &[(11, 100.0, &[(50, 0.1)])])];
        let c = consumos(&ops, &[lanc(1, 0, 10, 40.0), lanc(2, 0, 11, 30.0)]);

        perto(c[&50], 40.0 * 0.5 + 30.0 * 0.1);
        perto(c[&51], 80.0);
    }

    #[test]
    fn mesmo_produto_em_varias_linhas_usa_a_composicao_de_cada_linha() {
        // Como a OP 888 real: o mesmo produto com composições diferentes por linha.
        let ops = [op(888, &[(1543, 100.0, &[(49, 0.01)]), (1543, 100.0, &[(54, 0.04)]), (1543, 100.0, &[(54, 0.05)])])];
        let c = consumos(&ops, &[lanc(888, 1, 1543, 10.0)]);

        perto(c[&54], 0.4);
        assert!(!c.contains_key(&49), "a linha 0 não teve lançamento");
    }

    #[test]
    fn op_que_nao_esta_aberta_nao_conta() {
        let ops = [op(1, &[(10, 100.0, &[(50, 1.0)])])];
        assert!(consumos(&ops, &[lanc(2, 0, 10, 40.0)]).is_empty());
    }

    #[test]
    fn linha_que_mudou_de_produto_nao_conta() {
        let ops = [op(1, &[(99, 100.0, &[(50, 1.0)])])];
        assert!(consumos(&ops, &[lanc(1, 0, 10, 40.0)]).is_empty());
    }

    #[test]
    fn descarta_so_lancamentos_de_ops_que_sairam_das_abertas() {
        let ops = [op(1, &[(10, 100.0, &[]), (11, 100.0, &[])])];
        let mut lista = vec![lanc(1, 1, 11, 5.0), lanc(2, 0, 10, 5.0), lanc(1, 0, 77, 5.0)];

        assert_eq!(manter_validos(&mut lista, &ops), 2);
        assert_eq!(lista, [lanc(1, 1, 11, 5.0)]);
    }

    #[test]
    fn definir_cria_troca_e_apaga() {
        let mut lista = Vec::new();
        assert!(!definir(&mut lista, (1, 0, 10), None));
        assert!(definir(&mut lista, (1, 0, 10), Some(5.0)));
        assert!(!definir(&mut lista, (1, 0, 10), Some(5.0)), "mesmo valor não é mudança");
        assert!(definir(&mut lista, (1, 0, 10), Some(7.0)));
        assert_eq!(lista, [lanc(1, 0, 10, 7.0)]);
        assert!(definir(&mut lista, (1, 0, 10), None));
        assert!(lista.is_empty());
    }

    #[test]
    fn estoque_fisico_real_desconta_o_consumo_e_nao_mexe_na_necessidade() {
        let ops = [op(1, &[(10, 100.0, &[(50, 2.0)])])];
        let cadastro: HashMap<i64, Produto> = [Produto::from_json(&json!({"codigo": 50, "estoque": 150.0})).unwrap()]
            .into_iter()
            .map(|p| (p.codigo, p))
            .collect();
        let mut linhas = calculo::calcular(&ops, &cadastro).necessidades;

        aplicar(&mut linhas, &consumos(&ops, &[lanc(1, 0, 10, 40.0)]));

        let n = &linhas[0];
        perto(n.consumo_parcial, 80.0);
        perto(n.estoque_fisico_real(), 70.0);
        perto(n.necessidade, 200.0);
        perto(n.saldo(), -50.0);
    }

    #[test]
    fn texto_do_campo_volta_ao_mesmo_numero() {
        for v in [1.0, 1000.0, 2.5, 1234.5678] {
            let t = textos(&[lanc(1, 0, 10, v)]).remove(&(1, 0, 10)).unwrap();
            assert_eq!(fmt::ler_qtd(&t), Some(v), "{t}");
        }
    }

    #[test]
    fn formato_do_arquivo_salvo() {
        let salvo = r#"[{"op":888,"linha":2,"cod_produto":1543,"realizado":10.5}]"#;
        let lista: Vec<Lancamento> = serde_json::from_str(salvo).unwrap();
        assert_eq!(lista, [lanc(888, 2, 1543, 10.5)]);
    }
}
