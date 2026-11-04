//! Colunas configuráveis da aba Necessidades (quais aparecem e em que ordem).
//! A configuração é salva em `relatorios/config_colunas_necessidades.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::coleta;

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

impl ColunaNec {
    /// Ordem padrão, com a visibilidade padrão de cada coluna.
    pub const PADRAO: [(ColunaNec, bool); 19] = [
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

    pub fn rotulo(self) -> &'static str {
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

    /// A coluna Material é a que abre o detalhe do material — não pode ser ocultada.
    pub fn obrigatoria(self) -> bool {
        self == Self::Material
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigColunas {
    pub colunas: Vec<(ColunaNec, bool)>,
}

impl Default for ConfigColunas {
    fn default() -> Self {
        Self { colunas: ColunaNec::PADRAO.to_vec() }
    }
}

fn caminho() -> PathBuf {
    coleta::pasta_saida().join("config_colunas_necessidades.json")
}

impl ConfigColunas {
    /// Carrega do disco; se não existir/for inválido, usa o padrão.
    /// Colunas novas (de versões futuras) entram no fim, ocultas.
    pub fn carregar() -> Self {
        let mut cfg: Self = std::fs::read(caminho())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut vistas = Vec::new();
        cfg.colunas.retain(|(c, _)| {
            let nova = !vistas.contains(c);
            vistas.push(*c);
            nova
        });
        for (c, _) in ColunaNec::PADRAO {
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
        std::fs::write(caminho(), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn visiveis(&self) -> impl Iterator<Item = ColunaNec> + '_ {
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
        let mut c = ConfigColunas::default();
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
}
