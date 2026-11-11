//! Colunas configuráveis das tabelas (quais aparecem, em que ordem e com que
//! cores). A configuração de cada tabela fica no banco do servidor, separada
//! por usuário.

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
/// Descreve a mudança de configuração de colunas guardada no histórico
/// (`antes`/`depois` = configuração inteira como o servidor guarda; registros
/// antigos têm só a lista `colunas`; `null` = padrão).
pub fn descrever_mudanca(tabela: &str, antes: &serde_json::Value, depois: &serde_json::Value) -> String {
    match tabela {
        ColunaNec::TABELA => descrever::<ColunaNec>("Necessidades", antes, depois),
        ColunaProd::TABELA => descrever::<ColunaProd>("Produtos", antes, depois),
        outra => format!("Alterou as colunas de {outra}"),
    }
}

fn descrever<C: Coluna>(nome: &str, antes: &serde_json::Value, depois: &serde_json::Value) -> String {
    let ler = |v: &serde_json::Value| {
        let v = if v.is_object() { v.clone() } else { serde_json::json!({ "colunas": v }) };
        ConfigColunas::<C>::do_servidor(v)
    };
    let (a, d) = (ler(antes), ler(depois));
    let visivel = |cfg: &ConfigColunas<C>, c: C| cfg.colunas.iter().any(|(x, v)| *x == c && *v);
    let nomes = |lista: Vec<C>| lista.into_iter().map(Coluna::rotulo).collect::<Vec<_>>().join(", ");

    let mostrou: Vec<C> = d.visiveis().filter(|&c| !visivel(&a, c)).collect();
    let ocultou: Vec<C> = a.visiveis().filter(|&c| !visivel(&d, c)).collect();
    let ordem = |cfg: &ConfigColunas<C>| cfg.colunas.iter().map(|(c, _)| *c).collect::<Vec<_>>();
    let mut partes = Vec::new();
    if !mostrou.is_empty() {
        partes.push(format!("mostrou {}", nomes(mostrou)));
    }
    if !ocultou.is_empty() {
        partes.push(format!("ocultou {}", nomes(ocultou)));
    }
    if ordem(&a) != ordem(&d) {
        partes.push("mudou a ordem".to_string());
    }
    let mudou_cor: Vec<C> = d.colunas.iter().map(|(c, _)| *c).filter(|&c| a.cor_de(c) != d.cor_de(c)).collect();
    if !mudou_cor.is_empty() {
        partes.push(format!("mudou as cores de {}", nomes(mudou_cor)));
    }
    if partes.is_empty() {
        partes.push("sem mudança visível".to_string());
    }
    format!("Colunas de {nome}: {}", partes.join("; "))
}

/// Cor guardada como "#RRGGBB".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Lê "#RRGGBB" ou "RRGGBB".
    pub fn do_hex(s: &str) -> Option<Self> {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Self(byte(0)?, byte(2)?, byte(4)?))
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}

impl From<Rgb> for String {
    fn from(c: Rgb) -> Self {
        c.hex()
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        Self::do_hex(&s).ok_or_else(|| format!("cor inválida: {s}"))
    }
}

impl From<Rgb> for iced::Color {
    fn from(c: Rgb) -> Self {
        iced::Color::from_rgb8(c.0, c.1, c.2)
    }
}

/// Cores escolhidas para uma coluna; `None` = a cor padrão do tema.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorColuna {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fundo: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texto: Option<Rgb>,
}

impl CorColuna {
    pub fn padrao(self) -> bool {
        self == Self::default()
    }

    pub fn de(self, alvo: Alvo) -> Option<Rgb> {
        match alvo {
            Alvo::Fundo => self.fundo,
            Alvo::Texto => self.texto,
        }
    }
}

/// Qual cor da coluna se escolhe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alvo {
    Fundo,
    Texto,
}

#[derive(Debug, Clone, Copy)]
pub enum AcaoColuna {
    Visivel(usize, bool),
    Mover(usize, usize),
    MostrarTodas,
    /// Cor da coluna `usize` (posição em `colunas`); `None` = padrão.
    Cor(usize, Alvo, Option<Rgb>),
    Padrao,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "C: Coluna")]
pub struct ConfigColunas<C> {
    pub colunas: Vec<(C, bool)>,
    /// Só as colunas com alguma cor escolhida.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cores: Vec<(C, CorColuna)>,
}

impl<C: Coluna> Default for ConfigColunas<C> {
    fn default() -> Self {
        Self { colunas: C::PADRAO.to_vec(), cores: Vec::new() }
    }
}

impl<C: Coluna> ConfigColunas<C> {
    /// A partir do que o servidor guardou (`null` se nunca foi configurada);
    /// se não houver ou for inválido, usa o padrão.
    pub fn do_servidor(salva: serde_json::Value) -> Self {
        // Cores inválidas não fazem perder a ordem e a visibilidade.
        let cfg: Self = serde_json::from_value(salva.clone())
            .or_else(|_| serde_json::from_value(serde_json::json!({ "colunas": salva["colunas"] })))
            .unwrap_or_default();
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
        let mut com_cor = Vec::new();
        self.cores.retain(|(c, cor)| {
            let fica = !cor.padrao() && !com_cor.contains(c);
            com_cor.push(*c);
            fica
        });
        self
    }

    /// Cores da coluna (padrão se nenhuma foi escolhida).
    pub fn cor_de(&self, c: C) -> CorColuna {
        self.cores.iter().find(|(x, _)| *x == c).map(|(_, cor)| *cor).unwrap_or_default()
    }

    pub fn definir_cor(&mut self, c: C, alvo: Alvo, cor: Option<Rgb>) {
        let mut nova = self.cor_de(c);
        match alvo {
            Alvo::Fundo => nova.fundo = cor,
            Alvo::Texto => nova.texto = cor,
        }
        self.definir_cores(c, nova);
    }

    /// Troca as duas cores da coluna de uma vez (ex.: para desfazer uma edição).
    pub fn definir_cores(&mut self, c: C, cor: CorColuna) {
        self.cores.retain(|(x, _)| *x != c);
        if !cor.padrao() {
            self.cores.push((c, cor));
        }
    }

    /// Aplica a mudança (quem chama envia a nova configuração ao servidor).
    pub fn aplicar(&mut self, acao: AcaoColuna) {
        match acao {
            AcaoColuna::Visivel(i, v) => self.definir_visivel(i, v),
            AcaoColuna::Mover(i, destino) => self.mover(i, destino),
            AcaoColuna::MostrarTodas => self.mostrar_todas(),
            AcaoColuna::Cor(i, alvo, cor) => {
                if let Some(&(c, _)) = self.colunas.get(i) {
                    self.definir_cor(c, alvo, cor);
                }
            }
            // Volta a ordem e a visibilidade; as cores ficam (cada uma tem o seu "Padrão").
            AcaoColuna::Padrao => *self = Self { cores: std::mem::take(&mut self.cores), ..Self::default() },
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
        let salvo = ConfigColunas { colunas: vec![(ColunaNec::Tags, true), (ColunaNec::Material, true), (ColunaNec::Saldo, false)], cores: Vec::new() };
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
    fn cores_por_coluna() {
        let mut c = ConfigColunas::<ColunaProd>::default();
        let i = c.colunas.iter().position(|(c, _)| *c == ColunaProd::Estoque).unwrap();
        c.aplicar(AcaoColuna::Cor(i, Alvo::Fundo, Some(Rgb(255, 238, 170))));
        c.aplicar(AcaoColuna::Cor(i, Alvo::Texto, Some(Rgb(0, 0, 128))));
        let salvo = serde_json::to_value(&c).unwrap();
        assert_eq!(salvo["cores"], serde_json::json!([["Estoque", {"fundo": "#FFEEAA", "texto": "#000080"}]]));
        let lida = ConfigColunas::<ColunaProd>::do_servidor(salvo);
        assert_eq!(lida.cor_de(ColunaProd::Estoque).fundo, Some(Rgb(255, 238, 170)));

        // "Restaurar padrão" não apaga as cores; "Padrão" de cada cor, sim.
        c.aplicar(AcaoColuna::Padrao);
        assert!(!c.cor_de(ColunaProd::Estoque).padrao());
        let i = c.colunas.iter().position(|(c, _)| *c == ColunaProd::Estoque).unwrap();
        c.aplicar(AcaoColuna::Cor(i, Alvo::Fundo, None));
        c.aplicar(AcaoColuna::Cor(i, Alvo::Texto, None));
        assert!(c.cores.is_empty());
        assert!(serde_json::to_value(&c).unwrap().get("cores").is_none());
    }

    #[test]
    fn cor_invalida_nao_perde_a_ordem() {
        let salvo = serde_json::json!({"colunas": [["Tags", true]], "cores": [["Tags", {"fundo": "azul"}]]});
        let c = ConfigColunas::<ColunaProd>::do_servidor(salvo);
        assert_eq!(c.colunas[0], (ColunaProd::Tags, true));
        assert!(c.cores.is_empty());
        assert_eq!(Rgb::do_hex("#0a0B0c"), Some(Rgb(10, 11, 12)));
        assert_eq!(Rgb::do_hex("#12345"), None);
    }

    #[test]
    fn descreve_mudanca_de_cor() {
        let antes = serde_json::json!({"colunas": [["Tags", true]]});
        let depois = serde_json::json!({"colunas": [["Tags", true]], "cores": [["Tags", {"fundo": "#FFEEAA"}]]});
        assert_eq!(descrever_mudanca("produtos", &antes, &depois), "Colunas de Produtos: mudou as cores de Tags");
    }

    #[test]
    fn sem_configuracao_no_servidor_usa_o_padrao() {
        assert_eq!(ConfigColunas::<ColunaProd>::do_servidor(serde_json::Value::Null), ConfigColunas::default());
        let invalida = serde_json::json!({"colunas": [["NaoExiste", true]]});
        assert_eq!(ConfigColunas::<ColunaProd>::do_servidor(invalida), ConfigColunas::default());
    }
}
