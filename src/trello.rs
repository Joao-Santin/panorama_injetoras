//! Situação das OPs no quadro "Programação Máquinas" do Trello, lida do
//! servidor `biplas_server_claude` (rota `/api/trello/ops`, servida do banco: quem
//! busca no Trello é o servidor). Só informa: nada aqui altera o levantamento.

use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, bail};
use serde_json::Value;

const TEMPO_LIMITE: Duration = Duration::from_secs(30);

/// Em que lista do quadro o cartão da OP está. A ordem é a de exibição.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Etapa {
    EmMaquina,
    AguardandoMaquina,
    /// Cartão em "Concluído", mas a OP ainda está aberta no eGestor.
    AguardandoBaixa,
}

impl Etapa {
    fn de_slug(slug: &str) -> Option<Self> {
        match slug {
            "em-andamento" => Some(Self::EmMaquina),
            "aguardando-maquina" => Some(Self::AguardandoMaquina),
            "concluido" => Some(Self::AguardandoBaixa),
            _ => None,
        }
    }

    pub fn rotulo(self) -> &'static str {
        match self {
            Self::EmMaquina => "Em máquina",
            Self::AguardandoMaquina => "Aguardando máquina",
            Self::AguardandoBaixa => "Aguardando baixa",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Situacao {
    pub etapa: Etapa,
    /// Só para cartões em andamento: separador "MAQUINA N" acima do cartão.
    pub maquina: Option<String>,
}

impl Situacao {
    /// Ex.: "Em máquina · MAQUINA 4".
    pub fn texto(&self) -> String {
        match &self.maquina {
            Some(m) => format!("{} · {m}", self.etapa.rotulo()),
            None => self.etapa.rotulo().to_string(),
        }
    }
}

/// Situações de cada OP. Uma OP pode ter vários cartões (ex.: em duas máquinas).
pub type Situacoes = HashMap<i64, Vec<Situacao>>;

/// Posição da OP na ordem "aguardando máquina → em máquina → aguardando baixa → sem cartão".
/// Com vários cartões, vale a etapa mais cedo nessa ordem.
pub fn posicao_por_situacao(situacoes: &[Situacao]) -> u8 {
    situacoes
        .iter()
        .map(|s| match s.etapa {
            Etapa::AguardandoMaquina => 0,
            Etapa::EmMaquina => 1,
            Etapa::AguardandoBaixa => 2,
        })
        .min()
        .unwrap_or(3)
}

/// Chave para ordenar pela máquina: "MAQUINA 2" vem antes de "MAQUINA 10". Com a OP em
/// várias máquinas, vale a primeira. `None` = não está em máquina.
pub fn chave_maquina(situacoes: &[Situacao]) -> Option<(u64, String)> {
    situacoes
        .iter()
        .filter(|s| s.etapa == Etapa::EmMaquina)
        .filter_map(|s| s.maquina.as_deref())
        .map(|m| (numero_da_maquina(m).unwrap_or(u64::MAX), m.to_string()))
        .min()
}

/// Minúsculas, sem acentos e com um espaço só entre as palavras: "Aguardando  MÁQUINA" → "aguardando maquina".
pub fn normalizar(s: &str) -> String {
    let sem_acento: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            outro => outro,
        })
        .collect();
    sem_acento.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn numero_da_maquina(nome: &str) -> Option<u64> {
    nome.chars().filter(char::is_ascii_digit).collect::<String>().parse().ok()
}

/// Busca que, digitada por inteiro, filtra pela situação no Trello em vez de procurar texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiltroSituacao {
    Etapa(Etapa),
    /// Número da máquina: "maquina 1" não pega a 10 nem a 11.
    Maquina(u64),
}

impl FiltroSituacao {
    /// Reconhece "aguardando máquina", "em máquina", "aguardando baixa" e "máquina N".
    pub fn da_busca(busca: &str) -> Option<Self> {
        let b = normalizar(busca);
        match b.as_str() {
            "aguardando maquina" => Some(Self::Etapa(Etapa::AguardandoMaquina)),
            "em maquina" => Some(Self::Etapa(Etapa::EmMaquina)),
            "aguardando baixa" => Some(Self::Etapa(Etapa::AguardandoBaixa)),
            _ => {
                let resto = b.strip_prefix("maquina")?.trim();
                if resto.is_empty() || !resto.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                resto.parse().ok().map(Self::Maquina)
            }
        }
    }

    pub fn aceita(self, situacoes: &[Situacao]) -> bool {
        situacoes.iter().any(|s| match self {
            Self::Etapa(e) => s.etapa == e,
            Self::Maquina(n) => {
                s.etapa == Etapa::EmMaquina && s.maquina.as_deref().and_then(numero_da_maquina) == Some(n)
            }
        })
    }

    pub fn rotulo(self) -> String {
        match self {
            Self::Etapa(e) => e.rotulo().to_string(),
            Self::Maquina(n) => format!("MAQUINA {n}"),
        }
    }
}

/// `raiz` é o endereço do servidor (ex.: `http://localhost:3000`).
pub fn buscar(raiz: &str) -> Result<Situacoes> {
    let url = format!("{}/api/trello/ops", raiz.trim_end_matches('/'));
    let http = crate::api::cliente_http(Some(TEMPO_LIMITE))?;
    let resp = http
        .get(&url)
        .send()
        .with_context(|| format!("Não consegui conectar ao servidor em {url}. Ele está rodando?"))?;
    let status = resp.status();
    if !status.is_success() {
        let corpo = resp.text().unwrap_or_default();
        bail!("Servidor respondeu {status} em {url}: {corpo}");
    }
    let v: Value = resp.json().context("Resposta do servidor do Trello não é JSON")?;
    let Value::Array(cartoes) = v else { bail!("O servidor do Trello não devolveu uma lista de cartões") };
    Ok(de_json(&cartoes))
}

pub fn de_json(cartoes: &[Value]) -> Situacoes {
    let mut mapa: Situacoes = HashMap::new();
    for c in cartoes {
        let Some(etapa) = c["slug"].as_str().and_then(Etapa::de_slug) else { continue };
        let maquina = match etapa {
            Etapa::EmMaquina => c["maquina"].as_str().map(str::trim).filter(|m| !m.is_empty()).map(String::from),
            _ => None,
        };
        for op in c["ops"].as_array().into_iter().flatten().filter_map(Value::as_i64) {
            let lista = mapa.entry(op).or_default();
            let s = Situacao { etapa, maquina: maquina.clone() };
            if !lista.contains(&s) {
                lista.push(s);
            }
        }
    }
    for lista in mapa.values_mut() {
        lista.sort_by(|a, b| a.etapa.cmp(&b.etapa).then_with(|| a.maquina.cmp(&b.maquina)));
    }
    mapa
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;

    fn cartao(ops: &[i64], slug: &str, maquina: Option<&str>) -> Value {
        json!({"ops": ops, "slug": slug, "maquina": maquina, "cartao": "X", "url": ""})
    }

    #[test]
    fn le_as_tres_listas_e_a_maquina_so_de_quem_esta_em_andamento() {
        let s = de_json(&[
            cartao(&[1323], "aguardando-maquina", None),
            cartao(&[981, 1046], "em-andamento", Some("MAQUINA 9")),
            cartao(&[700], "concluido", Some("MAQUINA 1")),
            cartao(&[], "em-andamento", Some("MAQUINA 2")),
            cartao(&[5], "outra-lista", None),
        ]);

        assert_eq!(s[&1323], [Situacao { etapa: Etapa::AguardandoMaquina, maquina: None }]);
        assert_eq!(s[&1046][0].texto(), "Em máquina · MAQUINA 9");
        assert_eq!(s[&981], s[&1046]);
        assert_eq!(s[&700][0].texto(), "Aguardando baixa", "máquina não vale fora de andamento");
        assert!(!s.contains_key(&5));
        assert_eq!(s.len(), 4);
    }

    fn sit(etapa: Etapa, maquina: Option<&str>) -> Situacao {
        Situacao { etapa, maquina: maquina.map(String::from) }
    }

    #[test]
    fn posicao_segue_aguardando_em_maquina_baixa_e_sem_cartao() {
        use Etapa::*;
        assert_eq!(posicao_por_situacao(&[sit(AguardandoMaquina, None)]), 0);
        assert_eq!(posicao_por_situacao(&[sit(EmMaquina, Some("MAQUINA 1"))]), 1);
        assert_eq!(posicao_por_situacao(&[sit(AguardandoBaixa, None)]), 2);
        assert_eq!(posicao_por_situacao(&[]), 3);
        assert_eq!(posicao_por_situacao(&[sit(AguardandoBaixa, None), sit(EmMaquina, Some("M"))]), 1, "vale a mais cedo");
    }

    #[test]
    fn maquina_ordena_pelo_numero_e_usa_a_primeira() {
        use Etapa::*;
        let chave = |m: &str| chave_maquina(&[sit(EmMaquina, Some(m))]);
        assert!(chave("MAQUINA 2") < chave("MAQUINA 10"));
        assert!(chave("MAQUINA 10") < chave("INJETORA X"), "sem número vai para o fim");
        assert_eq!(chave_maquina(&[sit(EmMaquina, Some("MAQUINA 6")), sit(EmMaquina, Some("MAQUINA 4"))]), chave("MAQUINA 4"));
        assert_eq!(chave_maquina(&[sit(AguardandoMaquina, None)]), None);
        assert_eq!(chave_maquina(&[sit(EmMaquina, None)]), None, "em andamento sem separador");
    }

    #[test]
    fn busca_digitada_vira_filtro_sem_ligar_para_acento_e_maiuscula() {
        use Etapa::*;
        let f = FiltroSituacao::da_busca;
        assert_eq!(f("aguardando máquina"), Some(FiltroSituacao::Etapa(AguardandoMaquina)));
        assert_eq!(f("  AGUARDANDO   maquina "), Some(FiltroSituacao::Etapa(AguardandoMaquina)));
        assert_eq!(f("Em Máquina"), Some(FiltroSituacao::Etapa(EmMaquina)));
        assert_eq!(f("aguardando baixa"), Some(FiltroSituacao::Etapa(AguardandoBaixa)));
        assert_eq!(f("Máquina 4"), Some(FiltroSituacao::Maquina(4)));
        assert_eq!(f("maquina10"), Some(FiltroSituacao::Maquina(10)));
        for texto_livre in ["maquina", "aguardando", "aguardando maq", "maquina x", "1323", "maquina de lavar"] {
            assert_eq!(f(texto_livre), None, "{texto_livre}");
        }
    }

    #[test]
    fn filtro_de_maquina_e_pelo_numero_exato() {
        use Etapa::*;
        let m1 = FiltroSituacao::Maquina(1);
        assert!(m1.aceita(&[sit(EmMaquina, Some("MAQUINA 1"))]));
        assert!(!m1.aceita(&[sit(EmMaquina, Some("MAQUINA 10"))]));
        assert!(!m1.aceita(&[sit(EmMaquina, Some("MAQUINA 11"))]));
        assert!(m1.aceita(&[sit(EmMaquina, Some("MAQUINA 6")), sit(EmMaquina, Some("MAQUINA 1"))]), "OP em duas máquinas");
        assert!(!m1.aceita(&[]));

        let baixa = FiltroSituacao::Etapa(AguardandoBaixa);
        assert!(baixa.aceita(&[sit(AguardandoBaixa, None)]));
        assert!(!baixa.aceita(&[sit(AguardandoMaquina, None)]));
    }

    #[test]
    fn op_em_varios_cartoes_mostra_todos_sem_repetir_em_ordem() {
        let s = de_json(&[
            cartao(&[1339], "concluido", None),
            cartao(&[1339], "em-andamento", Some("MAQUINA 6")),
            cartao(&[1339], "em-andamento", Some("MAQUINA 4")),
            cartao(&[1339], "concluido", None),
        ]);
        let textos: Vec<String> = s[&1339].iter().map(Situacao::texto).collect();
        assert_eq!(textos, ["Em máquina · MAQUINA 4", "Em máquina · MAQUINA 6", "Aguardando baixa"]);
    }
}
