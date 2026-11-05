//! Colunas configuráveis das tabelas (quais aparecem e em que ordem).
//! Cada tabela salva a própria configuração em `relatorios/`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::coleta;

/// Uma coluna de uma tabela configurável.
pub trait Coluna: Copy + PartialEq + Serialize + DeserializeOwned + 'static {
    /// Ordem padrão, com a visibilidade padrão de cada coluna.
    const PADRAO: &'static [(Self, bool)];
    /// Nome do arquivo onde a configuração é salva.
    const ARQUIVO: &'static str;

    fn rotulo(self) -> &'static str;

    /// Coluna que não pode ser ocultada.
    fn obrigatoria(self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColunaNec {
    Material,
    Codigo,
    CodigoProprio,
    Categoria,
    Unidade,
    Necessidade,
    Estoque,
    EstoqueMinimo,
    Saldo,
    Falta,
    ComprarComMinimo,
    CustoUnit,
    CustoNecessidade,
    CustoFalta,
    ControlaEstoque,
    NumOps,
    ListaOps,
    NumProdutos,
    Tags,
}

impl Coluna for ColunaNec {
    const PADRAO: &'static [(Self, bool)] = &[
        (Self::Material, true),
        (Self::CodigoProprio, true),
        (Self::Categoria, true),
        (Self::Unidade, true),
        (Self::Necessidade, true),
        (Self::Estoque, true),
        (Self::Saldo, true),
        (Self::Falta, true),
        (Self::ComprarComMinimo, true),
        (Self::CustoUnit, true),
        (Self::CustoFalta, true),
        (Self::NumOps, true),
        (Self::Codigo, false),
        (Self::EstoqueMinimo, false),
        (Self::CustoNecessidade, false),
        (Self::ControlaEstoque, false),
        (Self::ListaOps, false),
        (Self::NumProdutos, false),
        (Self::Tags, false),
    ];
    const ARQUIVO: &'static str = "config_colunas_necessidades.json";

    fn rotulo(self) -> &'static str {
        match self {
            Self::Material => "Material",
            Self::Codigo => "Cód. eGestor",
            Self::CodigoProprio => "Cód. próprio",
            Self::Categoria => "Categoria",
            Self::Unidade => "Un.",
            Self::Necessidade => "Necessidade",
            Self::Estoque => "Estoque",
            Self::EstoqueMinimo => "Estoque mínimo",
            Self::Saldo => "Saldo após OPs",
            Self::Falta => "Falta",
            Self::ComprarComMinimo => "Comprar c/ mín.",
            Self::CustoUnit => "Custo un.",
            Self::CustoNecessidade => "Custo necessidade",
            Self::CustoFalta => "Custo falta",
            Self::ControlaEstoque => "Controla estoque",
            Self::NumOps => "Nº OPs",
            Self::ListaOps => "OPs (lista)",
            Self::NumProdutos => "Nº produtos",
            Self::Tags => "Tags (palavras-chave)",
        }
    }

    /// A coluna Material é a que abre o detalhe do material.
    fn obrigatoria(self) -> bool {
        self == Self::Material
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColunaProd {
    Codigo,
    CodigoProprio,
    Descricao,
    Categoria,
    Composicao,
    Unidade,
    Estoque,
    EstoqueMinimo,
    PesoLiquido,
    PrecoCusto,
    PrecoVenda,
    Tags,
}

impl Coluna for ColunaProd {
    const PADRAO: &'static [(Self, bool)] = &[
        (Self::Codigo, true),
        (Self::CodigoProprio, true),
        (Self::Descricao, true),
        (Self::Categoria, true),
        (Self::Composicao, true),
        (Self::Unidade, true),
        (Self::Estoque, true),
        (Self::EstoqueMinimo, true),
        (Self::PesoLiquido, true),
        (Self::PrecoCusto, true),
        (Self::PrecoVenda, true),
        (Self::Tags, true),
    ];
    const ARQUIVO: &'static str = "config_colunas_produtos.json";

    fn rotulo(self) -> &'static str {
        match self {
            Self::Codigo => "Cód.",
            Self::CodigoProprio => "Cód. próprio",
            Self::Descricao => "Descrição",
            Self::Categoria => "Categoria",
            Self::Composicao => "Composição",
            Self::Unidade => "Un.",
            Self::Estoque => "Estoque",
            Self::EstoqueMinimo => "Est. mín.",
            Self::PesoLiquido => "Peso líq. (kg)",
            Self::PrecoCusto => "Preço custo",
            Self::PrecoVenda => "Preço venda",
            Self::Tags => "Tags",
        }
    }

    /// A coluna Descrição é a que abre a composição do produto.
    fn obrigatoria(self) -> bool {
        self == Self::Descricao
    }
}

/// Mudança pedida no painel "Configurar colunas".
#[derive(Debug, Clone, Copy)]
pub enum AcaoColuna {
    Visivel(usize, bool),
    Mover(usize, usize),
    MostrarTodas,
    Padrao,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "C: Coluna")]
pub struct ConfigColunas<C> {
    pub colunas: Vec<(C, bool)>,
}

impl<C: Coluna> Default for ConfigColunas<C> {
    fn default() -> Self {
        Self { colunas: C::PADRAO.to_vec() }
    }
}

fn caminho<C: Coluna>() -> PathBuf {
    coleta::pasta_saida().join(C::ARQUIVO)
}

impl<C: Coluna> ConfigColunas<C> {
    /// Carrega do disco; se não existir/for inválido, usa o padrão.
    /// Colunas novas (de versões futuras) entram no fim, ocultas.
    pub fn carregar() -> Self {
        let mut cfg: Self = std::fs::read(caminho::<C>())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut vistas = Vec::new();
        cfg.colunas.retain(|(c, _)| {
            let nova = !vistas.contains(c);
            vistas.push(*c);
            nova
        });
        for &(c, _) in C::PADRAO {
            if !vistas.contains(&c) {
                cfg.colunas.push((c, false));
            }
        }
        for (c, v) in cfg.colunas.iter_mut() {
            if c.obrigatoria() {
                *v = true;
            }
        }
        cfg
    }

    pub fn salvar(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(coleta::pasta_saida())?;
        std::fs::write(caminho::<C>(), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    /// Aplica a mudança e salva no disco.
    pub fn aplicar(&mut self, acao: AcaoColuna) -> anyhow::Result<()> {
        match acao {
            AcaoColuna::Visivel(i, v) => self.definir_visivel(i, v),
            AcaoColuna::Mover(i, destino) => self.mover(i, destino),
            AcaoColuna::MostrarTodas => self.mostrar_todas(),
            AcaoColuna::Padrao => *self = Self::default(),
        }
        self.salvar()
    }

    pub fn visiveis(&self) -> impl Iterator<Item = C> + '_ {
        self.colunas.iter().filter(|(_, v)| *v).map(|(c, _)| *c)
    }

    pub fn definir_visivel(&mut self, i: usize, visivel: bool) {
        if let Some((c, v)) = self.colunas.get_mut(i) {
            *v = visivel || c.obrigatoria();
        }
    }

    /// Move a coluna `i` para a posição `destino` (limitada aos extremos).
    pub fn mover(&mut self, i: usize, destino: usize) {
        if i < self.colunas.len() {
            let destino = destino.min(self.colunas.len() - 1);
            let item = self.colunas.remove(i);
            self.colunas.insert(destino, item);
        }
    }

    pub fn mostrar_todas(&mut self) {
        for (_, v) in self.colunas.iter_mut() {
            *v = true;
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn mover_e_ocultar() {
        let mut c = ConfigColunas::<ColunaNec>::default();
        c.mover(0, 3);
        assert_eq!(c.colunas[3].0, ColunaNec::Material);
        c.mover(3, 99);
        assert_eq!(c.colunas.last().unwrap().0, ColunaNec::Material);
        let ultima = c.colunas.len() - 1;
        c.definir_visivel(ultima, false);
        assert!(c.colunas[ultima].1, "Material não pode ser ocultada");
        c.definir_visivel(0, false);
        assert!(!c.colunas[0].1);
    }

    #[test]
    fn descricao_do_produto_nao_pode_ser_ocultada() {
        let mut c = ConfigColunas::<ColunaProd>::default();
        let i = c.colunas.iter().position(|(c, _)| *c == ColunaProd::Descricao).unwrap();
        c.definir_visivel(i, false);
        assert!(c.colunas[i].1);
    }

    #[test]
    fn formato_do_arquivo_salvo_nao_mudou() {
        let salvo = r#"{"colunas":[["Material",true],["Tags",false]]}"#;
        let c: ConfigColunas<ColunaNec> = serde_json::from_str(salvo).unwrap();
        assert_eq!(c.colunas, [(ColunaNec::Material, true), (ColunaNec::Tags, false)]);
    }
}
