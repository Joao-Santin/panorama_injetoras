//! Filtro de busca das abas Necessidades e Produtos e a busca por aproximação
//! (estilo fuzzy finder) da barra de busca.

use std::collections::BTreeSet;

use crate::trello::normalizar;

/// Para que o item é usado. O eGestor não tem esse campo: vem do 1º dígito do
/// código próprio (2 = produto final, 3 = MP, 4 = em processo, 5 = uso e consumo,
/// 6 = embalagem; o resto, outros insumos).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoUso {
    MateriaPrima,
    ProdutoEmProcesso,
    Embalagem,
    ProdutoFinal,
    OutrosInsumos,
    UsoEConsumo,
}

impl TipoUso {
    pub const TODOS: [TipoUso; 6] = [
        Self::MateriaPrima,
        Self::ProdutoEmProcesso,
        Self::Embalagem,
        Self::ProdutoFinal,
        Self::OutrosInsumos,
        Self::UsoEConsumo,
    ];

    pub fn do_codigo(codigo_proprio: &str) -> Self {
        match codigo_proprio.trim().chars().next() {
            Some('2') => Self::ProdutoFinal,
            Some('3') => Self::MateriaPrima,
            Some('4') => Self::ProdutoEmProcesso,
            Some('5') => Self::UsoEConsumo,
            Some('6') => Self::Embalagem,
            _ => Self::OutrosInsumos,
        }
    }
}

impl std::fmt::Display for TipoUso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MateriaPrima => "Matéria-prima",
            Self::ProdutoEmProcesso => "Produto em processo",
            Self::Embalagem => "Embalagem",
            Self::ProdutoFinal => "Produto final",
            Self::OutrosInsumos => "Outros insumos",
            Self::UsoEConsumo => "Uso e consumo",
        })
    }
}

/// Opção de uma lista de escolha que pode ficar sem nada escolhido ("Qualquer").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opcao<T>(pub Option<T>);

impl<T: std::fmt::Display> std::fmt::Display for Opcao<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(v) => v.fmt(f),
            None => f.write_str("Qualquer"),
        }
    }
}

/// Lista de opções para um `pick_list`, começando por "Qualquer".
pub fn opcoes<T: Clone>(itens: impl IntoIterator<Item = T>) -> Vec<Opcao<T>> {
    std::iter::once(Opcao(None)).chain(itens.into_iter().map(|v| Opcao(Some(v)))).collect()
}

/// Mudanças no filtro, vindas do painel.
#[derive(Debug, Clone)]
pub enum AcaoFiltro {
    Categoria(Option<String>),
    Prefixo(String),
    AdicionarTag(String),
    RemoverTag(String),
    Tipo(Option<TipoUso>),
    Limpar,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filtro {
    pub categoria: Option<String>,
    /// Começo do código próprio (normalmente os 2 primeiros dígitos).
    pub prefixo: String,
    /// O item precisa ter todas.
    pub tags: BTreeSet<String>,
    pub tipo: Option<TipoUso>,
}

impl Filtro {
    pub fn aplicar(&mut self, acao: AcaoFiltro) {
        match acao {
            AcaoFiltro::Categoria(c) => self.categoria = c,
            // Só dígitos e no máximo 2: é o grupo do código próprio.
            AcaoFiltro::Prefixo(s) => self.prefixo = s.chars().filter(|c| !c.is_whitespace()).take(2).collect(),
            AcaoFiltro::AdicionarTag(t) => {
                self.tags.insert(t);
            }
            AcaoFiltro::RemoverTag(t) => {
                self.tags.remove(&t);
            }
            AcaoFiltro::Tipo(t) => self.tipo = t,
            AcaoFiltro::Limpar => *self = Self::default(),
        }
    }

    /// Quantos campos do filtro estão preenchidos (para o botão "Filtros (n)").
    pub fn ativos(&self) -> usize {
        usize::from(self.categoria.is_some())
            + usize::from(!self.prefixo.is_empty())
            + usize::from(!self.tags.is_empty())
            + usize::from(self.tipo.is_some())
    }

    pub fn aceita(&self, codigo_proprio: &str, categoria: &str, tags: &[String]) -> bool {
        let cod = codigo_proprio.trim();
        self.categoria.as_deref().is_none_or(|c| c == categoria)
            && cod.starts_with(self.prefixo.as_str())
            && self.tipo.is_none_or(|t| t == TipoUso::do_codigo(cod))
            && self.tags.iter().all(|t| tags.iter().any(|x| x.trim().eq_ignore_ascii_case(t.trim())))
    }
}

// ========================================================== busca fuzzy ===

/// Texto da barra de busca, já preparado. Cada palavra precisa ser achada em algum
/// campo (em qualquer ordem); dentro do campo vale achar as letras na ordem, mesmo
/// com outras no meio ("tmp prt" acha "TAMPA PRETA"). Quanto mais junto e no começo
/// das palavras, maior a nota.
pub struct Busca {
    palavras: Vec<Vec<char>>,
}

impl Busca {
    pub fn nova(texto: &str) -> Self {
        Self { palavras: normalizar(texto).split(' ').filter(|p| !p.is_empty()).map(|p| p.chars().collect()).collect() }
    }

    pub fn vazia(&self) -> bool {
        self.palavras.is_empty()
    }

    /// Nota do item com estes campos, ou `None` se alguma palavra não foi achada.
    /// Busca vazia aceita tudo com nota 0.
    pub fn nota<'a>(&self, campos: impl IntoIterator<Item = &'a str>) -> Option<i64> {
        if self.vazia() {
            return Some(0);
        }
        let campos: Vec<Vec<char>> = campos.into_iter().map(|c| normalizar(c).chars().collect()).collect();
        self.palavras.iter().map(|p| campos.iter().filter_map(|c| nota_palavra(c, p)).max()).sum()
    }
}

/// Começo de palavra: início do texto ou depois de algo que não é letra/dígito.
fn inicio_de_palavra(alvo: &[char], i: usize) -> bool {
    i == 0 || !alvo[i - 1].is_alphanumeric()
}

fn nota_palavra(alvo: &[char], p: &[char]) -> Option<i64> {
    let n = p.len();
    if n == 0 || alvo.len() < n {
        return None;
    }
    if alvo == p {
        return Some(10_000);
    }
    // Trecho igual: o melhor é no começo de uma palavra e o mais cedo possível.
    let trechos = (0..=alvo.len() - n).filter(|&i| alvo[i..i + n] == *p);
    if let Some(nota) = trechos
        .map(|i| 1_000 + if inicio_de_palavra(alvo, i) { 500 } else { 0 } - i.min(100) as i64)
        .max()
    {
        return Some(nota + 10 * n as i64);
    }
    letras_em_ordem(alvo, p)
}

/// As letras de `p` na ordem dentro de `alvo`, na janela mais curta que começa em
/// cada ocorrência da 1ª letra. Recusa quando ficam espalhadas demais.
fn letras_em_ordem(alvo: &[char], p: &[char]) -> Option<i64> {
    let n = p.len();
    let mut melhor: Option<i64> = None;
    for inicio in (0..alvo.len()).filter(|&i| alvo[i] == p[0]) {
        let mut pos = Vec::with_capacity(n);
        let mut j = 0;
        for (i, &c) in alvo.iter().enumerate().skip(inicio) {
            if c == p[j] {
                pos.push(i);
                j += 1;
                if j == n {
                    break;
                }
            }
        }
        if j < n {
            break; // daqui para a frente também não acha
        }
        let espaco = pos[n - 1] - pos[0] + 1 - n;
        if espaco > n + 3 {
            continue;
        }
        let mut nota = 100 + 10 * n as i64;
        for (k, &i) in pos.iter().enumerate() {
            if inicio_de_palavra(alvo, i) {
                nota += 15;
            }
            if k > 0 && pos[k - 1] + 1 == i {
                nota += 8;
            }
        }
        nota -= 3 * espaco as i64;
        melhor = melhor.max(Some(nota));
    }
    melhor
}

#[cfg(test)]
mod testes {
    use super::*;

    fn nota(busca: &str, campos: &[&str]) -> Option<i64> {
        Busca::nova(busca).nota(campos.iter().copied())
    }

    #[test]
    fn tipo_de_uso_pelo_primeiro_digito() {
        assert_eq!(TipoUso::do_codigo("3000050"), TipoUso::MateriaPrima);
        assert_eq!(TipoUso::do_codigo(" 2100016"), TipoUso::ProdutoFinal);
        assert_eq!(TipoUso::do_codigo("4100001"), TipoUso::ProdutoEmProcesso);
        assert_eq!(TipoUso::do_codigo("5000001"), TipoUso::UsoEConsumo);
        assert_eq!(TipoUso::do_codigo("6100606"), TipoUso::Embalagem);
        assert_eq!(TipoUso::do_codigo("7100001"), TipoUso::OutrosInsumos);
        assert_eq!(TipoUso::do_codigo(""), TipoUso::OutrosInsumos);
    }

    #[test]
    fn filtro_aceita() {
        let tags = vec!["Biplas".to_string(), "Prime".to_string()];
        let mut f = Filtro::default();
        assert!(f.aceita("2100016", "Injetados", &tags));
        f.aplicar(AcaoFiltro::Prefixo("21x".into()));
        assert_eq!(f.prefixo, "21");
        assert!(f.aceita("2100016", "Injetados", &tags));
        assert!(!f.aceita("2000016", "Injetados", &tags));
        f.aplicar(AcaoFiltro::AdicionarTag("prime".into()));
        assert!(f.aceita("2100016", "Injetados", &tags));
        f.aplicar(AcaoFiltro::AdicionarTag("Outra".into()));
        assert!(!f.aceita("2100016", "Injetados", &tags));
        f.aplicar(AcaoFiltro::Limpar);
        f.aplicar(AcaoFiltro::Categoria(Some("Compras".into())));
        f.aplicar(AcaoFiltro::Tipo(Some(TipoUso::ProdutoFinal)));
        assert_eq!(f.ativos(), 2);
        assert!(!f.aceita("2100016", "Injetados", &tags));
        assert!(f.aceita("2100016", "Compras", &tags));
        assert!(!f.aceita("3100016", "Compras", &tags));
    }

    #[test]
    fn busca_por_aproximacao() {
        let tampa = ["TAMPA PRETA DO CORPO", "2100016"];
        assert!(nota("", &tampa).is_some());
        assert!(nota("tampa", &tampa).is_some());
        assert!(nota("tmp prt", &tampa).is_some(), "letras na ordem, com falhas");
        assert!(nota("preta tampa", &tampa).is_some(), "palavras em qualquer ordem");
        assert!(nota("2100", &tampa).is_some());
        assert!(nota("tampa azul", &tampa).is_none(), "toda palavra precisa ser achada");
        assert!(nota("tpo", &["TAMPA PRETA DO CORPO"]).is_none(), "letras espalhadas demais");
        // Sem acento e sem diferenciar maiúsculas.
        assert!(nota("succao", &["BOMBA DE SUCÇÃO"]).is_some());
    }

    #[test]
    fn mais_parecido_tem_nota_maior() {
        let n = |b, c| nota(b, &[c]).unwrap();
        assert!(n("tampa", "TAMPA") > n("tampa", "TAMPA PRETA"));
        assert!(n("tampa", "TAMPA PRETA") > n("tampa", "SUBTAMPA"));
        assert!(n("tampa", "SUBTAMPA") > n("tampa", "TAMPINHA"));
        assert!(n("tmpa", "TAMPA") > n("tmpa", "TEMPO DA ALCA"));
    }
}
