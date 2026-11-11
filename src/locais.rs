//! Um mesmo item cadastrado em mais de um lugar. A cópia guardada em outro local
//! tem na descrição "(local: X)", onde X é o código próprio do item de produção,
//! ex.: "0144 - EMBALAGEM SUPORTE SM 20X50 1099P - (almoxarifado: 6100116)".
//! O item de produção é o que não tem essa marca.

use std::collections::{HashMap, HashSet};

use crate::modelo::Produto;

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

/// Marcas "(nome: código)" da descrição, ex.: ("almoxarifado", "6100116").
fn marcas(descricao: &str) -> impl Iterator<Item = (&str, &str)> {
    descricao.split('(').skip(1).filter_map(|resto| {
        let dentro = resto.split_once(')')?.0;
        let (nome, codigo) = dentro.split_once(':')?;
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
        std::iter::once(p.codigo).chain(self.outros.get(&p.codigo).into_iter().flatten().map(|x| x.codigo))
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
        ];
        let produtos: HashMap<i64, Produto> = lista.into_iter().map(|p| (p.codigo, p)).collect();
        let l = agrupar(&produtos);
        assert_eq!(l.copias, HashSet::from([633, 11]));
        assert_eq!(l.outros[&116], [Local { nome: "Almoxarifado".into(), codigo: 633, estoque: 3000.0 }]);
        assert_eq!(l.total(&produtos[&116]), 3114.0);
        assert_eq!(l.total(&produtos[&84]), 5.0, "injetora não é local de outro produto");
        assert_eq!(l.codigos(&produtos[&10]).collect::<Vec<_>>(), [10, 11]);
    }
}
