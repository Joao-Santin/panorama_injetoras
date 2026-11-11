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

/// Etiqueta do cartão no Trello (ex.: "Urgente", vermelha). O nome pode ser vazio.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Etiqueta {
    pub nome: String,
    /// Cor do Trello: "red", "green_dark", "sky_light"... `None` = sem cor.
    pub cor: Option<String>,
}

impl Etiqueta {
    /// Cor RGB da etiqueta, a mesma paleta do Trello. Sem cor ou desconhecida = cinza.
    pub fn rgb(&self) -> (u8, u8, u8) {
        match self.cor.as_deref().unwrap_or("") {
            "green_light" => (0xBA, 0xF3, 0xDB),
            "green" => (0x4B, 0xCE, 0x97),
            "green_dark" => (0x1F, 0x84, 0x5A),
            "yellow_light" => (0xF8, 0xE6, 0xA0),
            "yellow" => (0xF5, 0xCD, 0x47),
            "yellow_dark" => (0x94, 0x6F, 0x00),
            "orange_light" => (0xFE, 0xDE, 0xC8),
            "orange" => (0xFE, 0xA3, 0x62),
            "orange_dark" => (0xC2, 0x51, 0x00),
            "red_light" => (0xFF, 0xD5, 0xD2),
            "red" => (0xF8, 0x71, 0x68),
            "red_dark" => (0xC9, 0x37, 0x2C),
            "purple_light" => (0xDF, 0xD8, 0xFD),
            "purple" => (0x9F, 0x8F, 0xEF),
            "purple_dark" => (0x6E, 0x5D, 0xC6),
            "blue_light" => (0xCC, 0xE0, 0xFF),
            "blue" => (0x57, 0x9D, 0xFF),
            "blue_dark" => (0x0C, 0x66, 0xE4),
            "sky_light" => (0xC6, 0xED, 0xFB),
            "sky" => (0x6C, 0xC3, 0xE0),
            "sky_dark" => (0x22, 0x7D, 0x9B),
            "lime_light" => (0xD3, 0xF1, 0xA7),
            "lime" => (0x94, 0xC7, 0x48),
            "lime_dark" => (0x5B, 0x7F, 0x24),
            "pink_light" => (0xFD, 0xD0, 0xEC),
            "pink" => (0xE7, 0x74, 0xBB),
            "pink_dark" => (0xAE, 0x47, 0x87),
            "black_light" => (0xDC, 0xDF, 0xE4),
            "black" => (0x8C, 0x9B, 0xAB),
            "black_dark" => (0x62, 0x6F, 0x86),
            _ => (0xB3, 0xB9, 0xC4),
        }
    }

    /// Letra escura nas cores claras e clara nas escuras.
    pub fn letra_clara(&self) -> bool {
        let (r, g, b) = self.rgb();
        0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) < 140.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Situacao {
    pub etapa: Etapa,
    /// Só para cartões em andamento: separador "MAQUINA N" acima do cartão.
    pub maquina: Option<String>,
    pub etiquetas: Vec<Etiqueta>,
    /// Link do cartão no Trello (vazio se o servidor não mandou).
    pub url: String,
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

/// Etiquetas de todos os cartões da OP, sem repetir, na ordem em que aparecem.
pub fn etiquetas(situacoes: &[Situacao]) -> Vec<&Etiqueta> {
    let mut out: Vec<&Etiqueta> = Vec::new();
    for e in situacoes.iter().flat_map(|s| &s.etiquetas) {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

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
        // Servidor antigo não manda `etiquetas`: fica sem.
        let etiquetas: Vec<Etiqueta> = c["etiquetas"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| Etiqueta {
                nome: e["nome"].as_str().unwrap_or("").trim().to_string(),
                cor: e["cor"].as_str().filter(|c| !c.is_empty()).map(String::from),
            })
            .filter(|e| !e.nome.is_empty() || e.cor.is_some())
            .collect();
        let url = c["url"].as_str().unwrap_or("").trim().to_string();
        for op in c["ops"].as_array().into_iter().flatten().filter_map(Value::as_i64) {
            let lista = mapa.entry(op).or_default();
            let s = Situacao { etapa, maquina: maquina.clone(), etiquetas: etiquetas.clone(), url: url.clone() };
            if !lista.contains(&s) {
                lista.push(s);
            }
        }
    }
    for lista in mapa.values_mut() {
        lista.sort_by(|a, b| a.etapa.cmp(&b.etapa).then_with(|| a.maquina.cmp(&b.maquina)).then_with(|| a.url.cmp(&b.url)));
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

        assert_eq!(s[&1323], [sit(Etapa::AguardandoMaquina, None)]);
        assert_eq!(s[&1046][0].texto(), "Em máquina · MAQUINA 9");
        assert_eq!(s[&981], s[&1046]);
        assert_eq!(s[&700][0].texto(), "Aguardando baixa", "máquina não vale fora de andamento");
        assert!(!s.contains_key(&5));
        assert_eq!(s.len(), 4);
    }

    fn sit(etapa: Etapa, maquina: Option<&str>) -> Situacao {
        Situacao { etapa, maquina: maquina.map(String::from), etiquetas: Vec::new(), url: String::new() }
    }

    #[test]
    fn le_etiquetas_e_link_e_junta_sem_repetir() {
        let mut c1 = cartao(&[10], "aguardando-maquina", None);
        c1["url"] = json!("https://trello.com/c/abc");
        c1["etiquetas"] = json!([{"nome": "Urgente", "cor": "red"}, {"nome": "", "cor": "green"}, {"nome": "", "cor": null}]);
        let mut c2 = cartao(&[10], "em-andamento", Some("MAQUINA 2"));
        c2["etiquetas"] = json!([{"nome": "Urgente", "cor": "red"}, {"nome": "Cliente X", "cor": null}]);
        let s = de_json(&[c1, c2, cartao(&[20], "concluido", None)]);

        let aguardando = s[&10].iter().find(|x| x.etapa == Etapa::AguardandoMaquina).unwrap();
        assert_eq!(aguardando.url, "https://trello.com/c/abc");
        assert_eq!(aguardando.etiquetas.len(), 2, "etiqueta sem nome e sem cor é descartada");
        let nomes: Vec<&str> = etiquetas(&s[&10]).iter().map(|e| e.nome.as_str()).collect();
        assert_eq!(nomes, ["Urgente", "Cliente X", ""], "na ordem dos cartões: em máquina antes de aguardando");
        assert!(s[&20][0].etiquetas.is_empty(), "servidor antigo, sem o campo");
    }

    #[test]
    fn cor_da_letra_acompanha_o_fundo() {
        let e = |c: &str| Etiqueta { nome: "x".into(), cor: Some(c.into()) };
        assert!(e("red_dark").letra_clara());
        assert!(!e("yellow_light").letra_clara());
        assert_eq!(Etiqueta { nome: "x".into(), cor: None }.rgb(), e("inexistente").rgb());
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
