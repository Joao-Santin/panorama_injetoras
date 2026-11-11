//! Um mesmo item cadastrado em mais de um lugar. A cópia guardada em outro local
//! tem na descrição "(local: X)", onde X é o código próprio do item de produção,
//! ex.: "0144 - EMBALAGEM SUPORTE SM 20X50 1099P - (almoxarifado: 6100116)"
//! (às vezes sem os dois-pontos: "(almoxarifado 3000025)").
//! O item de produção é o que não tem essa marca.

use std::collections::{HashMap, HashSet};

use crate::{calculo::Necessidade, modelo::Produto};

/// Nome do local do item de produção.
pub const PRODUCAO: &str = "Produção";

#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    /// Ex.: "Almoxarifado".
    pub nome: String,
    /// Código (eGestor) da cópia neste local.
    pub codigo: i64,
    pub estoque: f64,
}

/// Cópias em outros locais, por item de produção.
#[derive(Debug, Clone, Default)]
pub struct Locais {
    /// Código do item de produção → cópias dele em outros locais.
    pub outros: HashMap<i64, Vec<Local>>,
    /// Códigos das cópias (somem da lista quando os locais são juntados).
    pub copias: HashSet<i64>,
}

/// Marcas "(nome: código)" da descrição, ex.: ("almoxarifado", "6100116"). Sem os
/// dois-pontos, só "(almoxarifado código)": outros textos assim não são local, ex.:
/// "(COD 01)" ou "(CÓDIGO UTILIZADO PARA BAIXAR NAS OPS AS CAIXA 6100606)".
fn marcas(descricao: &str) -> impl Iterator<Item = (&str, &str)> {
    descricao.split('(').skip(1).filter_map(|resto| {
        let dentro = resto.split_once(')')?.0.trim();
        let sem_dois_pontos =
            || dentro.split_once(char::is_whitespace).filter(|(nome, _)| nome.eq_ignore_ascii_case("almoxarifado"));
        let (nome, codigo) = dentro.split_once(':').or_else(sem_dois_pontos)?;
        let (nome, codigo) = (nome.trim(), codigo.trim());
        let nome_ok = !nome.is_empty() && nome.chars().all(|c| c.is_alphanumeric() || c == ' ');
        let codigo_ok = !codigo.is_empty() && !codigo.contains(char::is_whitespace);
        (nome_ok && codigo_ok).then_some((nome, codigo))
    })
}

/// "almoxarifado" → "Almoxarifado".
fn nome_do_local(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|p| p.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect()).unwrap_or_default()
}

/// Junta as cópias aos itens de produção. Só conta a marca cujo código é o código
/// próprio de outro produto (ex.: "(INJETORA: 7)" na mão de obra não é local).
pub fn agrupar(produtos: &HashMap<i64, Produto>) -> Locais {
    let por_codigo_proprio: HashMap<&str, i64> = produtos
        .values()
        .filter(|p| !p.codigo_proprio.trim().is_empty())
        .map(|p| (p.codigo_proprio.trim(), p.codigo))
        .collect();
    let mut l = Locais::default();
    let mut copias: Vec<&Produto> = produtos.values().collect();
    copias.sort_by_key(|p| p.codigo);
    for p in copias {
        let alvo = marcas(&p.descricao).find_map(|(nome, cod)| {
            let alvo = *por_codigo_proprio.get(cod)?;
            (alvo != p.codigo).then_some((nome, alvo))
        });
        if let Some((nome, alvo)) = alvo {
            l.outros.entry(alvo).or_default().push(Local { nome: nome_do_local(nome), codigo: p.codigo, estoque: p.estoque });
            l.copias.insert(p.codigo);
        }
    }
    // Uma cópia que também é alvo de outra (cadeia) não some: fica como item próprio.
    let alvos: HashSet<i64> = l.outros.keys().copied().collect();
    l.copias.retain(|c| !alvos.contains(c));
    l.outros.retain(|alvo, _| !l.copias.contains(alvo));
    for v in l.outros.values_mut() {
        v.retain(|x| l.copias.contains(&x.codigo));
    }
    l.outros.retain(|_, v| !v.is_empty());
    l
}

impl Locais {
    /// Estoque somado de todos os locais do item (só o dele, se não tiver cópias).
    pub fn total(&self, p: &Produto) -> f64 {
        p.estoque + self.outros.get(&p.codigo).map_or(0.0, |v| v.iter().map(|x| x.estoque).sum())
    }

    /// O item e as cópias, para a busca achar o item de produção pelo código ou
    /// descrição de qualquer local.
    pub fn codigos(&self, p: &Produto) -> impl Iterator<Item = i64> + '_ {
        self.codigos_de(p.codigo)
    }

    pub fn codigos_de(&self, codigo: i64) -> impl Iterator<Item = i64> + '_ {
        std::iter::once(codigo).chain(self.outros.get(&codigo).into_iter().flatten().map(|x| x.codigo))
    }

    /// Estoque guardado nos outros locais do item (0 se não tiver cópias).
    pub fn estoque_fora(&self, codigo: i64) -> f64 {
        self.outros.get(&codigo).map_or(0.0, |v| v.iter().map(|x| x.estoque).sum())
    }
}

/// Junta as linhas de necessidade das cópias na linha do item de produção: soma
/// necessidade, simulação, consumo parcial e OPs, e o estoque de todos os locais.
/// Se só a cópia é usada nas OPs, a linha passa a ser a do item de produção.
/// `categoria` dá o nome da categoria de um produto.
pub fn juntar_necessidades(
    linhas: &mut Vec<Necessidade>,
    l: &Locais,
    produtos: &HashMap<i64, Produto>,
    categoria: impl Fn(&Produto) -> String,
) {
    if l.outros.is_empty() {
        return;
    }
    let copia_de: HashMap<i64, i64> =
        l.outros.iter().flat_map(|(alvo, v)| v.iter().map(move |x| (x.codigo, *alvo))).collect();
    let (copias, resto): (Vec<_>, Vec<_>) = std::mem::take(linhas).into_iter().partition(|n| copia_de.contains_key(&n.cod_insumo));
    *linhas = resto;
    for c in copias {
        let alvo = copia_de[&c.cod_insumo];
        if let Some(n) = linhas.iter_mut().find(|n| n.cod_insumo == alvo) {
            n.necessidade += c.necessidade;
            n.simulada += c.simulada;
            n.consumo_parcial += c.consumo_parcial;
            n.ops.extend(c.ops);
            n.produtos.extend(c.produtos);
            continue;
        }
        let Some(p) = produtos.get(&alvo) else {
            linhas.push(c);
            continue;
        };
        linhas.push(Necessidade {
            cod_insumo: alvo,
            codigo_proprio: p.codigo_proprio.clone(),
            descricao: p.descricao.clone(),
            categoria: categoria(p),
            unidade: p.unidade.clone(),
            estoque: p.estoque,
            estoque_minimo: p.estoque_minimo,
            controla_estoque: p.controlar_estoque,
            custo_unit: p.preco_custo,
            tags: p.tags.clone(),
            ..c
        });
    }
    for n in linhas.iter_mut() {
        n.estoque += l.estoque_fora(n.cod_insumo);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn produto(codigo: i64, codigo_proprio: &str, descricao: &str, estoque: f64) -> Produto {
        Produto {
            codigo,
            codigo_proprio: codigo_proprio.into(),
            descricao: descricao.into(),
            unidade: "UN".into(),
            estoque,
            estoque_minimo: 0.0,
            controlar_estoque: true,
            peso_liquido: 0.0,
            peso_bruto: 0.0,
            preco_custo: 0.0,
            preco_venda: 0.0,
            tipo: String::new(),
            tags: Vec::new(),
            cod_categoria: 0,
        }
    }

    #[test]
    fn junta_a_copia_do_almoxarifado_ao_item_de_producao() {
        let lista = [
            produto(116, "6100116", "0144 - EMBALAGEM SUPORTE SM 20X50 1099P", 114.0),
            produto(633, "5100633", "0144 - EMBALAGEM SUPORTE SM 20X50 1099P - (almoxarifado: 6100116)", 3000.0),
            produto(84, "5000084", "MO - HORA MAQUINA (INJETORAS: 8-9-10)", 5.0),
            produto(10, "3100110", "0615 - MOLA KIT FIX FF358 LS2", 1.0),
            produto(11, "5100611", "0615 - MOLA KIT FIX FF358 LS2 - (Almoxarifado: 3100110) P/ LS2", 2.0),
            produto(25, "3000025", "PARAFUSO PHS AA 4,8X13 (BICO FILTRO)", -2096.0),
            produto(620, "5000620", "PARAFUSO PHS AA 4,8X13 (BICO FILTRO) (almoxarifado 3000025)", 19000.0),
            produto(7, "01", "SKIMMER F-300", 1.0),
            produto(8, "5000008", "KIT DISCO P/ CADERNO INTELIGENTE PQ CRISTAL(COD 01)", 1.0),
        ];
        let produtos: HashMap<i64, Produto> = lista.into_iter().map(|p| (p.codigo, p)).collect();
        let l = agrupar(&produtos);
        assert_eq!(l.copias, HashSet::from([633, 11, 620]), "também sem os dois-pontos");
        assert_eq!(l.total(&produtos[&25]), 16904.0);
        assert_eq!(l.outros[&116], [Local { nome: "Almoxarifado".into(), codigo: 633, estoque: 3000.0 }]);
        assert_eq!(l.total(&produtos[&116]), 3114.0);
        assert_eq!(l.total(&produtos[&84]), 5.0, "injetora não é local de outro produto");
        assert_eq!(l.codigos(&produtos[&10]).collect::<Vec<_>>(), [10, 11]);
    }

    fn necessidade(p: &Produto, necessidade: f64, op: i64) -> Necessidade {
        Necessidade {
            cod_insumo: p.codigo,
            codigo_proprio: p.codigo_proprio.clone(),
            descricao: p.descricao.clone(),
            categoria: "Embalagem".into(),
            unidade: p.unidade.clone(),
            necessidade,
            simulada: 0.0,
            consumo_parcial: 0.0,
            estoque: p.estoque,
            estoque_minimo: 0.0,
            controla_estoque: true,
            custo_unit: 1.0,
            ops: [op].into(),
            produtos: Default::default(),
            tags: Vec::new(),
            mao_de_obra: false,
        }
    }

    #[test]
    fn junta_as_necessidades_e_o_estoque_dos_locais() {
        let lista = [
            produto(25, "3000025", "MOLA", 100.0),
            produto(26, "5000026", "MOLA - (almoxarifado: 3000025)", 900.0),
            produto(30, "3000030", "CAIXA", 0.0),
            produto(31, "5000031", "CAIXA - (almoxarifado: 3000030)", 50.0),
        ];
        let produtos: HashMap<i64, Produto> = lista.into_iter().map(|p| (p.codigo, p)).collect();
        let l = agrupar(&produtos);

        // Só o item de produção nas OPs: saldo 100 - 500 = -400 vira 1000 - 500 = 500.
        let mut linhas = vec![necessidade(&produtos[&25], 500.0, 1), necessidade(&produtos[&31], 20.0, 2)];
        juntar_necessidades(&mut linhas, &l, &produtos, |_| "Embalagem".into());
        assert_eq!(linhas.len(), 2);
        let mola = linhas.iter().find(|n| n.cod_insumo == 25).unwrap();
        assert_eq!((mola.estoque, mola.saldo()), (1000.0, 500.0));

        // Só a cópia nas OPs: a linha passa a ser a do item de produção.
        let caixa = linhas.iter().find(|n| n.cod_insumo == 30).unwrap();
        assert_eq!((caixa.codigo_proprio.as_str(), caixa.necessidade, caixa.estoque), ("3000030", 20.0, 50.0));
        assert!(!linhas.iter().any(|n| n.cod_insumo == 31));

        // Os dois nas OPs: soma as necessidades e as OPs.
        let mut linhas = vec![necessidade(&produtos[&25], 500.0, 1), necessidade(&produtos[&26], 300.0, 2)];
        juntar_necessidades(&mut linhas, &l, &produtos, |_| String::new());
        assert_eq!(linhas.len(), 1);
        assert_eq!((linhas[0].necessidade, linhas[0].estoque, linhas[0].ops.len()), (800.0, 1000.0, 2));
    }
}
