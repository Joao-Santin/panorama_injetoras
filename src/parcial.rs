//! Lançamento parcial do realizado das OPs em aberto. O eGestor só baixa o estoque
//! quando a OP é encerrada; até lá, o que já foi produzido consumiu material que o
//! estoque do sistema ainda mostra. Nada aqui altera o levantamento das necessidades.
//!
//! Uma OP pode ter o mesmo produto em várias linhas, com quantidades e composições
//! diferentes, e o eGestor não dá um código às linhas. Por isso o lançamento guarda a
//! posição da linha e o produto: se a linha mudar de produto, ele deixa de valer.
//!
//! Os lançamentos ficam no servidor (`/api/panorama/parciais`), que também apaga os
//! que deixaram de valer quando sincroniza o eGestor.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{calculo::Necessidade, fmt, modelo::Op};

/// (nº da OP, posição da linha na OP, código do produto)
pub type Chave = (i64, usize, i64);

/// Um tipo de volume da produção: quantos volumes e quantas peças vão em cada um.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Grupo {
    pub volumes: f64,
    pub qtd_volume: f64,
}

impl Grupo {
    pub fn total(self) -> f64 {
        self.volumes * self.qtd_volume
    }
}

/// Soma dos tipos de volume: é o realizado da linha.
pub fn total(grupos: &[Grupo]) -> f64 {
    grupos.iter().map(|g| g.total()).sum()
}

/// Ex.: "1V×2.000 + 3V×1.000".
pub fn composicao(grupos: &[Grupo]) -> String {
    grupos.iter().map(|g| format!("{}V×{}", fmt::qtd(g.volumes), fmt::qtd(g.qtd_volume))).collect::<Vec<_>>().join(" + ")
}

/// Volumes somados por quantidade por volume, da maior quantidade para a menor
/// (dois tipos "de 500" contam como um só).
fn por_quantidade(grupos: &[Grupo]) -> Vec<(f64, f64)> {
    let mut m: Vec<(f64, f64)> = Vec::new();
    for g in grupos {
        match m.iter_mut().find(|(q, _)| *q == g.qtd_volume) {
            Some((_, v)) => *v += g.volumes,
            None => m.push((g.qtd_volume, g.volumes)),
        }
    }
    m.sort_by(|a, b| b.0.total_cmp(&a.0));
    m
}

/// O realizado é sempre o total da linha: a soma de volumes × quantidade por volume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lancamento {
    pub op: i64,
    pub linha: usize,
    pub cod_produto: i64,
    pub realizado: f64,
    /// Preenchidos só com um tipo de volume (e nos lançamentos de antes dos tipos).
    #[serde(default)]
    pub volumes: Option<f64>,
    #[serde(default)]
    pub qtd_volume: Option<f64>,
    /// Lançamentos de antes dos tipos de volume não têm.
    #[serde(default)]
    pub grupos: Option<Vec<Grupo>>,
}

/// Tipos de volume salvos, aceitando o formato de antes (um tipo só em `volumes`/`qtd_volume`).
fn grupos_salvos(grupos: &Option<Vec<Grupo>>, volumes: Option<f64>, qtd_volume: Option<f64>) -> Vec<Grupo> {
    match (grupos, volumes, qtd_volume) {
        (Some(g), _, _) => g.clone(),
        (None, Some(volumes), Some(qtd_volume)) if volumes > 0.0 => vec![Grupo { volumes, qtd_volume }],
        _ => Vec::new(),
    }
}

impl Lancamento {
    fn chave(&self) -> Chave {
        (self.op, self.linha, self.cod_produto)
    }

    /// Monta o lançamento como o servidor o guarda (com um tipo só, também em `volumes`/`qtd_volume`).
    pub fn novo((op, linha, cod_produto): Chave, grupos: Vec<Grupo>) -> Self {
        let (volumes, qtd_volume) = match grupos.as_slice() {
            [g] => (Some(g.volumes), Some(g.qtd_volume)),
            _ => (None, None),
        };
        Self { op, linha, cod_produto, realizado: total(&grupos), volumes, qtd_volume, grupos: Some(grupos) }
    }

    pub fn grupos(&self) -> Vec<Grupo> {
        grupos_salvos(&self.grupos, self.volumes, self.qtd_volume)
    }
}

/// Caminho do lançamento no servidor, para apagar.
pub fn caminho((op, linha, cod_produto): Chave) -> String {
    format!("/api/panorama/parciais/{op}/{linha}/{cod_produto}")
}

/// Grava o realizado de uma linha (sem tipos de volume = apaga). Devolve se algo mudou.
pub fn definir(lancamentos: &mut Vec<Lancamento>, chave: Chave, grupos: Vec<Grupo>) -> bool {
    let posicao = lancamentos.iter().position(|l| l.chave() == chave);
    let novo = (!grupos.is_empty()).then(|| Lancamento::novo(chave, grupos));
    match (posicao, novo) {
        (Some(i), Some(l)) if lancamentos[i] == l => false,
        (Some(i), Some(l)) => {
            lancamentos[i] = l;
            true
        }
        (Some(i), None) => {
            lancamentos.remove(i);
            true
        }
        (None, Some(l)) => {
            lancamentos.push(l);
            true
        }
        (None, None) => false,
    }
}

/// Texto de um tipo de volume, como digitado.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CampoGrupo {
    pub volumes: String,
    pub qtd_volume: String,
}

/// Campos de volumes de uma linha, como digitados: um por tipo de volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campos {
    pub grupos: Vec<CampoGrupo>,
}

/// Número para um campo de edição: sem separador de milhares nem arredondamento,
/// lido de volta dá exatamente o mesmo número.
fn texto_numero(v: f64) -> String {
    v.to_string().replace('.', ",")
}

impl Campos {
    /// Campos a partir do que está salvo; sem lançamento (ou de antes dos volumes), um tipo vazio.
    pub fn do_lancamento(l: Option<&Lancamento>) -> Self {
        let mut grupos: Vec<CampoGrupo> = l
            .map(Lancamento::grupos)
            .unwrap_or_default()
            .into_iter()
            .map(|g| CampoGrupo { volumes: texto_numero(g.volumes), qtd_volume: texto_numero(g.qtd_volume) })
            .collect();
        if grupos.is_empty() {
            grupos.push(CampoGrupo::default());
        }
        Self { grupos }
    }

    pub fn adicionar(&mut self) {
        self.grupos.push(CampoGrupo::default());
    }

    /// Tira o tipo `i`, mas sempre fica ao menos um (vazio).
    pub fn remover(&mut self, i: usize) {
        if i < self.grupos.len() {
            self.grupos.remove(i);
        }
        if self.grupos.is_empty() {
            self.grupos.push(CampoGrupo::default());
        }
    }

    /// Tipos de volume válidos. Tipo em branco ou com 0 volumes não conta; nenhum = apaga o lançamento.
    pub fn ler(&self) -> Result<Vec<Grupo>, String> {
        // `ler_qtd` só aceita mais que zero; aqui o zero vale (não conta).
        let so_zeros = |s: &str| !s.trim().is_empty() && s.trim().chars().all(|c| matches!(c, '0' | ',' | '.'));
        let mut grupos = Vec::new();
        for (i, c) in self.grupos.iter().enumerate() {
            let em_branco = c.volumes.trim().is_empty() && c.qtd_volume.trim().is_empty();
            if em_branco || so_zeros(&c.volumes) {
                continue;
            }
            let n = i + 1;
            let volumes = fmt::ler_qtd(&c.volumes).ok_or(format!("tipo {n}: informe os volumes (0 tira o tipo)"))?;
            let qtd_volume = fmt::ler_qtd(&c.qtd_volume).ok_or(format!("tipo {n}: informe a quantidade por volume"))?;
            grupos.push(Grupo { volumes, qtd_volume });
        }
        Ok(grupos)
    }
}

/// Uma confirmação de realizado guardada no servidor (`/api/panorama/parciais/lancamentos`).
#[derive(Debug, Clone, Deserialize)]
pub struct Confirmacao {
    pub linha: usize,
    pub cod_produto: i64,
    pub em: String,
    #[serde(default)]
    pub volumes: Option<f64>,
    #[serde(default)]
    pub qtd_volume: Option<f64>,
    #[serde(default)]
    pub grupos: Option<Vec<Grupo>>,
    /// Total depois da confirmação (0 = apagado) e antes dela.
    pub realizado: f64,
    pub anterior: f64,
    pub usuario: Option<String>,
}

impl Confirmacao {
    pub fn grupos(&self) -> Vec<Grupo> {
        grupos_salvos(&self.grupos, self.volumes, self.qtd_volume)
    }

    /// Quanto entrou em relação à confirmação anterior da mesma linha (`antes`), ex.:
    /// "+4.500 (9v de 500)" ou "+3.000 (3v de 1.000, −1v de 500)". Se a anterior não é
    /// conhecida (lançamento de antes deste registro), mostra a composição do total.
    pub fn delta(&self, antes: Option<&Confirmacao>) -> String {
        let delta = self.realizado - self.anterior;
        let sinal = if delta >= 0.0 { "+" } else { "−" };
        let base = format!("{sinal}{}", fmt::qtd(delta.abs()));
        if self.realizado == 0.0 {
            return format!("{base} (lançamento apagado)");
        }
        let agora = self.grupos();
        if agora.is_empty() {
            return base;
        }
        let anteriores = match antes {
            _ if self.anterior == 0.0 => Some(Vec::new()),
            Some(a) if (a.realizado - self.anterior).abs() < 1e-9 => Some(a.grupos()),
            _ => None,
        };
        let Some(anteriores) = anteriores else {
            return format!("{base} (total {})", composicao(&agora));
        };
        let (agora, anteriores) = (por_quantidade(&agora), por_quantidade(&anteriores));
        let mut quantidades: Vec<f64> = agora.iter().chain(&anteriores).map(|(q, _)| *q).collect();
        quantidades.sort_by(|a, b| b.total_cmp(a));
        quantidades.dedup();
        let volumes = |m: &[(f64, f64)], q: f64| m.iter().find(|(x, _)| *x == q).map_or(0.0, |(_, v)| *v);
        let partes: Vec<String> = quantidades
            .into_iter()
            .filter_map(|q| {
                let dv = volumes(&agora, q) - volumes(&anteriores, q);
                let sinal = if dv < 0.0 { "−" } else { "" };
                (dv != 0.0).then(|| format!("{sinal}{}v de {}", fmt::qtd(dv.abs()), fmt::qtd(q)))
            })
            .collect();
        if partes.is_empty() { base } else { format!("{base} ({})", partes.join(", ")) }
    }
}

fn linhas_abertas(ops: &[Op]) -> impl Iterator<Item = (Chave, &crate::modelo::ItemOp)> {
    ops.iter().flat_map(|o| o.itens.iter().enumerate().map(move |(i, item)| ((o.codigo, i, item.cod_produto), item)))
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

/// Total já produzido de cada produto, somando as linhas de `ops` (as OPs em aberto).
pub fn produzidos(ops: &[Op], lancamentos: &[Lancamento]) -> HashMap<i64, f64> {
    let realizado: HashMap<Chave, f64> = lancamentos.iter().map(|l| (l.chave(), l.realizado)).collect();
    let mut total = HashMap::new();
    for (chave, item) in linhas_abertas(ops) {
        if let Some(r) = realizado.get(&chave) {
            *total.entry(item.cod_produto).or_insert(0.0) += r;
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

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;
    use crate::{calculo, modelo::Produto};

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
        Lancamento { op, linha, cod_produto, realizado, volumes: None, qtd_volume: None, grupos: None }
    }

    fn vol(volumes: f64, qtd_volume: f64) -> Grupo {
        Grupo { volumes, qtd_volume }
    }

    fn campos(grupos: &[(&str, &str)]) -> Campos {
        Campos {
            grupos: grupos.iter().map(|(v, q)| CampoGrupo { volumes: v.to_string(), qtd_volume: q.to_string() }).collect(),
        }
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
    fn produzido_soma_as_linhas_e_as_ops_do_mesmo_produto() {
        let ops = [op(1, &[(10, 100.0, &[]), (10, 100.0, &[]), (11, 100.0, &[])]), op(2, &[(10, 100.0, &[])])];
        let p = produzidos(&ops, &[lanc(1, 0, 10, 5.0), lanc(1, 1, 10, 2.5), lanc(2, 0, 10, 1.0), lanc(1, 2, 11, 4.0)]);

        perto(p[&10], 8.5);
        perto(p[&11], 4.0);
    }

    #[test]
    fn produzido_ignora_op_fechada_e_linha_que_mudou_de_produto() {
        let ops = [op(1, &[(10, 100.0, &[])])];
        assert!(produzidos(&ops, &[lanc(2, 0, 10, 5.0), lanc(1, 0, 77, 5.0)]).is_empty());
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
    fn definir_cria_troca_e_apaga_com_a_soma_dos_tipos() {
        let mut lista = Vec::new();
        assert!(!definir(&mut lista, (1, 0, 10), vec![]));
        assert!(definir(&mut lista, (1, 0, 10), vec![vol(1.0, 2000.0), vol(3.0, 1000.0)]));
        assert!(!definir(&mut lista, (1, 0, 10), vec![vol(1.0, 2000.0), vol(3.0, 1000.0)]), "mesmo valor não é mudança");
        perto(lista[0].realizado, 5000.0);
        assert_eq!((lista[0].volumes, lista[0].qtd_volume), (None, None), "vários tipos");
        assert!(definir(&mut lista, (1, 0, 10), vec![vol(17.0, 500.0)]));
        assert_eq!((lista[0].volumes, lista[0].qtd_volume), (Some(17.0), Some(500.0)), "um tipo só, como o servidor guarda");
        assert!(definir(&mut lista, (1, 0, 10), vec![vol(34.0, 250.0)]), "mesmo total, volumes diferentes");
        assert!(definir(&mut lista, (1, 0, 10), vec![]));
        assert!(lista.is_empty());
    }

    #[test]
    fn campos_leem_os_tipos_e_ignoram_os_vazios_e_os_zerados() {
        assert_eq!(campos(&[("1", "2000"), ("3", "1000")]).ler(), Ok(vec![vol(1.0, 2000.0), vol(3.0, 1000.0)]));
        assert_eq!(campos(&[("2", "12,5")]).ler().map(|g| total(&g)), Ok(25.0));
        assert_eq!(campos(&[("8", "500"), ("", ""), ("0", "")]).ler(), Ok(vec![vol(8.0, 500.0)]));
        assert_eq!(campos(&[("0", "")]).ler(), Ok(vec![]), "nada = apaga");
        assert!(campos(&[("8", "500"), ("", "500")]).ler().unwrap_err().starts_with("tipo 2:"));
        assert!(campos(&[("8", "")]).ler().is_err());
        assert!(campos(&[("8", "0")]).ler().is_err());
        assert!(campos(&[("-1", "500")]).ler().is_err());

        let salvo = Lancamento::novo((1, 0, 10), vec![vol(17.0, 12.5), vol(2.0, 100.0)]);
        let c = Campos::do_lancamento(Some(&salvo));
        assert_eq!(c, campos(&[("17", "12,5"), ("2", "100")]));
        assert_eq!(c.ler(), Ok(salvo.grupos()), "volta ao mesmo número");
        assert_eq!(Campos::do_lancamento(Some(&lanc(1, 0, 10, 5.0))), campos(&[("", "")]), "lançamento antigo, sem volumes");
        let antigo = Lancamento { volumes: Some(8.0), qtd_volume: Some(500.0), ..lanc(1, 0, 10, 4000.0) };
        assert_eq!(antigo.grupos(), [vol(8.0, 500.0)], "um tipo só, de antes dos tipos");
    }

    #[test]
    fn remover_sempre_deixa_um_tipo() {
        let mut c = campos(&[("1", "2000"), ("3", "1000")]);
        c.remover(0);
        assert_eq!(c, campos(&[("3", "1000")]));
        c.remover(0);
        assert_eq!(c, campos(&[("", "")]));
        c.adicionar();
        assert_eq!(c.grupos.len(), 2);
    }

    #[test]
    fn composicao_do_total() {
        assert_eq!(composicao(&[vol(1.0, 2000.0), vol(3.0, 1000.0)]), "1V×2.000 + 3V×1.000");
    }

    #[test]
    fn delta_compara_com_a_confirmacao_anterior_por_tipo_de_volume() {
        let conf = |anterior: f64, grupos: Vec<Grupo>| Confirmacao {
            linha: 0,
            cod_produto: 10,
            em: String::new(),
            volumes: None,
            qtd_volume: None,
            realizado: total(&grupos),
            grupos: Some(grupos),
            anterior,
            usuario: None,
        };
        let c1 = conf(0.0, vec![vol(8.0, 500.0)]);
        assert_eq!(c1.delta(None), "+4.000 (8v de 500)");
        let c2 = conf(4000.0, vec![vol(17.0, 500.0)]);
        assert_eq!(c2.delta(Some(&c1)), "+4.500 (9v de 500)");
        let c3 = conf(8500.0, vec![vol(17.0, 500.0), vol(3.0, 1000.0)]);
        assert_eq!(c3.delta(Some(&c2)), "+3.000 (3v de 1.000)", "novo tipo de volume");
        let c4 = conf(11500.0, vec![vol(15.0, 500.0), vol(4.0, 1000.0)]);
        assert_eq!(c4.delta(Some(&c3)), "+0 (1v de 1.000, −2v de 500)");
        assert_eq!(c2.delta(None), "+4.500 (total 17V×500)", "anterior desconhecida: mostra a composição");

        let apagado = Confirmacao { grupos: Some(vec![]), realizado: 0.0, ..conf(4000.0, vec![]) };
        assert_eq!(apagado.delta(Some(&c1)), "−4.000 (lançamento apagado)");
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
            let t = texto_numero(v);
            assert_eq!(fmt::ler_qtd(&t), Some(v), "{t}");
        }
    }

    #[test]
    fn formato_do_servidor() {
        let antigo = r#"[{"op":888,"linha":2,"cod_produto":1543,"realizado":10.5}]"#;
        let lista: Vec<Lancamento> = serde_json::from_str(antigo).unwrap();
        assert_eq!(lista, [lanc(888, 2, 1543, 10.5)]);
        let um_tipo = r#"[{"op":1,"linha":0,"cod_produto":5,"realizado":4000,"volumes":8,"qtd_volume":500,"grupos":null}]"#;
        let lista: Vec<Lancamento> = serde_json::from_str(um_tipo).unwrap();
        assert_eq!(lista[0].grupos(), [vol(8.0, 500.0)]);
        let tipos = r#"[{"op":1,"linha":0,"cod_produto":5,"realizado":5000,"volumes":null,"qtd_volume":null,
            "grupos":[{"volumes":1,"qtd_volume":2000},{"volumes":3,"qtd_volume":1000}]}]"#;
        let lista: Vec<Lancamento> = serde_json::from_str(tipos).unwrap();
        assert_eq!(lista[0].grupos(), [vol(1.0, 2000.0), vol(3.0, 1000.0)]);
    }
}
