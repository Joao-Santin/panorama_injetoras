//! Colunas configuráveis das tabelas (quais aparecem e em que ordem).
//! A configuração de cada tabela fica no banco do servidor e vale para todos
//! os computadores.

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Uma coluna de uma tabela configurável.
pub trait Coluna: Copy + PartialEq + Serialize + DeserializeOwned + 'static {
    /// Ordem padrão, com a visibilidade padrão de cada coluna.
    const PADRAO: &'static [(Self, bool)];
    /// Nome da tabela no servidor (`/api/panorama/colunas/{TABELA}`).
    const TABELA: &'static str;

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
    NecessidadeSimulacao,
    SaldoAposSimulacao,
    SaldoAposOpsESimulacao,
    ConsumoParcial,
    EstoqueFisicoReal,
}

impl Coluna for ColunaNec {
    const PADRAO: &'static [(Self, bool)] = &[
        (Self::Material, true),
        (Self::CodigoProprio, true),
        (Self::Categoria, true),
        (Self::Unidade, true),
        (Self::Necessidade, true),
        (Self::Estoque, true),
        (Self::ConsumoParcial, true),
        (Self::EstoqueFisicoReal, true),
        (Self::Saldo, true),
        (Self::NecessidadeSimulacao, true),
        (Self::SaldoAposSimulacao, true),
        (Self::SaldoAposOpsESimulacao, true),
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
    const TABELA: &'static str = "necessidades";

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
            Self::NecessidadeSimulacao => "Necessidade em Simulação",
            Self::SaldoAposSimulacao => "Saldo após Simulação",
            Self::SaldoAposOpsESimulacao => "Saldo após OPs e Simulação",
            Self::ConsumoParcial => "Consumo Parcial",
            Self::EstoqueFisicoReal => "Estoque Físico Real",
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
    ProduzidoParcial,
    EstoqueReal,
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
        (Self::ProduzidoParcial, true),
        (Self::EstoqueReal, true),
        (Self::EstoqueMinimo, true),
        (Self::PesoLiquido, true),
        (Self::PrecoCusto, true),
        (Self::PrecoVenda, true),
        (Self::Tags, true),
    ];
    const TABELA: &'static str = "produtos";

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
            Self::ProduzidoParcial => "Produzido Parcial",
            Self::EstoqueReal => "Estoque real",
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

impl<C: Coluna> ConfigColunas<C> {
    /// A partir do que o servidor guardou (`null` se nunca foi configurada);
    /// se não houver ou for inválido, usa o padrão.
    pub fn do_servidor(salva: serde_json::Value) -> Self {
        let cfg: Self = serde_json::from_value(salva).unwrap_or_default();
        cfg.normalizada()
    }

    /// Tira repetições, garante as colunas obrigatórias e acrescenta as colunas que não
    /// existiam quando a configuração foi salva: no fim, com a visibilidade padrão delas.
    fn normalizada(mut self) -> Self {
        let mut vistas = Vec::new();
        self.colunas.retain(|(c, _)| {
            let nova = !vistas.contains(c);
            vistas.push(*c);
            nova
        });
        for &(c, visivel) in C::PADRAO {
            if !vistas.contains(&c) {
                self.colunas.push((c, visivel));
            }
        }
        for (c, v) in self.colunas.iter_mut() {
            if c.obrigatoria() {
                *v = true;
            }
        }
        self
    }

    /// Aplica a mudança (quem chama envia a nova configuração ao servidor).
    pub fn aplicar(&mut self, acao: AcaoColuna) {
        match acao {
            AcaoColuna::Visivel(i, v) => self.definir_visivel(i, v),
            AcaoColuna::Mover(i, destino) => self.mover(i, destino),
            AcaoColuna::MostrarTodas => self.mostrar_todas(),
            AcaoColuna::Padrao => *self = Self::default(),
        }
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
    fn colunas_novas_entram_no_fim_com_a_visibilidade_padrao() {
        let salvo = ConfigColunas { colunas: vec![(ColunaNec::Tags, true), (ColunaNec::Material, true), (ColunaNec::Saldo, false)] };
        let c = salvo.normalizada();

        // A escolha salva é mantida.
        assert_eq!(c.colunas[..3], [(ColunaNec::Tags, true), (ColunaNec::Material, true), (ColunaNec::Saldo, false)]);
        let visivel = |alvo| c.colunas.iter().find(|(x, _)| *x == alvo).unwrap().1;
        assert!(visivel(ColunaNec::SaldoAposOpsESimulacao), "padrão visível");
        assert!(!visivel(ColunaNec::ListaOps), "padrão oculta");
        assert_eq!(c.colunas.len(), ColunaNec::PADRAO.len());
    }

    #[test]
    fn formato_guardado_no_servidor_nao_mudou() {
        let salvo = serde_json::json!({"colunas": [["Material", true], ["Tags", false]]});
        let c: ConfigColunas<ColunaNec> = serde_json::from_value(salvo.clone()).unwrap();
        assert_eq!(c.colunas, [(ColunaNec::Material, true), (ColunaNec::Tags, false)]);
        assert_eq!(serde_json::to_value(&c).unwrap(), salvo);
    }

    #[test]
    fn sem_configuracao_no_servidor_usa_o_padrao() {
        assert_eq!(ConfigColunas::<ColunaProd>::do_servidor(serde_json::Value::Null), ConfigColunas::default());
        let invalida = serde_json::json!({"colunas": [["NaoExiste", true]]});
        assert_eq!(ConfigColunas::<ColunaProd>::do_servidor(invalida), ConfigColunas::default());
    }
}
