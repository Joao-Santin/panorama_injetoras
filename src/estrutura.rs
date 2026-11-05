//! Explosão multinível da composição cadastrada de um produto: BOM e custeio.
//!
//! Cada item composto é aberto até os insumos finais. O custo é sempre calculado
//! de baixo para cima a partir do custo cadastrado desses insumos finais: o custo
//! cadastrado de um item composto serve só de comparação, nunca entra na soma.

use std::collections::BTreeMap;

use crate::{calculo::eh_mao_de_obra, coleta::Analise};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoNo {
    /// Tem composição própria, que aparece nas linhas seguintes.
    Composto,
    Material,
    MaoDeObra,
    /// Já aparece acima no próprio ramo (cadastro circular): não é aberto de novo.
    Ciclo,
}

impl TipoNo {
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::Composto => "Composto",
            Self::Material => "Matéria-prima / insumo",
            Self::MaoDeObra => "Mão de obra",
            Self::Ciclo => "Cadastro circular",
        }
    }
}

/// Uma linha da árvore. As quantidades e custos totais são pela quantidade pedida do produto raiz.
#[derive(Debug, Clone, Copy)]
pub struct No<'a> {
    /// 0 = o próprio produto.
    pub nivel: usize,
    pub codigo: i64,
    pub codigo_proprio: &'a str,
    pub descricao: &'a str,
    pub categoria: &'a str,
    pub unidade: &'a str,
    pub tipo: TipoNo,
    /// Consumo por unidade do item de cima, já com perda.
    pub qtd_por_pai: f64,
    pub perc_perda: f64,
    pub qtd_total: f64,
    /// Insumo final: custo cadastrado. Composto: soma calculada dos filhos.
    pub custo_unit: f64,
    pub custo_total: f64,
    pub custo_cadastrado: f64,
    /// Este item, ou algo abaixo dele, ficou sem custo: o valor está subestimado.
    pub incompleto: bool,
}

pub struct Estrutura<'a> {
    /// A árvore em ordem de leitura (cada item seguido do que há dentro dele).
    pub nos: Vec<No<'a>>,
    pub niveis: usize,
    pub custo_materiais: f64,
    pub custo_mao_de_obra: f64,
    /// Insumos finais sem custo cadastrado, sem repetição: (descrição, categoria).
    pub sem_custo: Vec<(&'a str, &'a str)>,
    /// Itens que aparecem dentro deles mesmos, sem repetição.
    pub ciclos: Vec<&'a str>,
}

/// Um material somado em todos os pontos da estrutura onde aparece, pela quantidade pedida do produto raiz.
#[derive(Debug, Clone, Copy)]
pub struct MaterialSomado<'a> {
    pub codigo_proprio: &'a str,
    pub descricao: &'a str,
    pub categoria: &'a str,
    pub unidade: &'a str,
    /// Em quantas linhas da árvore o material aparece.
    pub usos: usize,
    pub qtd_total: f64,
    pub custo_unit: f64,
    pub custo_total: f64,
}

impl<'a> Estrutura<'a> {
    /// Custo calculado de 1 unidade do produto raiz.
    pub fn custo_produto(&self) -> f64 {
        self.nos[0].custo_unit
    }

    /// Materiais da árvore somados por código, do maior para o menor custo.
    /// Compostos não entram (já estão abertos nos seus materiais), nem mão de obra.
    pub fn materiais_somados(&self) -> Vec<MaterialSomado<'a>> {
        let mut por_codigo: BTreeMap<i64, MaterialSomado<'a>> = BTreeMap::new();
        for no in self.nos.iter().filter(|n| n.tipo == TipoNo::Material) {
            let m = por_codigo.entry(no.codigo).or_insert(MaterialSomado {
                codigo_proprio: no.codigo_proprio,
                descricao: no.descricao,
                categoria: no.categoria,
                unidade: no.unidade,
                usos: 0,
                qtd_total: 0.0,
                custo_unit: no.custo_unit,
                custo_total: 0.0,
            });
            m.usos += 1;
            m.qtd_total += no.qtd_total;
            m.custo_total += no.custo_total;
        }
        let mut lista: Vec<_> = por_codigo.into_values().collect();
        lista.sort_by(|x, y| y.custo_total.total_cmp(&x.custo_total).then(x.descricao.cmp(y.descricao)));
        lista
    }
}

struct Explosao<'a> {
    analise: &'a Analise,
    nos: Vec<No<'a>>,
    custo_materiais: f64,
    custo_mao_de_obra: f64,
    sem_custo: BTreeMap<i64, (&'a str, &'a str)>,
    ciclos: BTreeMap<i64, &'a str>,
}

impl<'a> Explosao<'a> {
    fn categoria(&self, codigo: i64) -> &'a str {
        self.analise.produtos.get(&codigo).map_or("—", |p| self.analise.categoria_de(p.cod_categoria))
    }

    /// Acrescenta os filhos de `codigo` e devolve o custo de 1 unidade dele e se ficou incompleto.
    /// `caminho` tem os itens do ramo atual: um mesmo insumo pode aparecer em ramos
    /// diferentes, só é ciclo se reaparecer dentro de si mesmo.
    fn descer(&mut self, codigo: i64, nivel: usize, qtd_pai: f64, caminho: &mut Vec<i64>) -> (f64, bool) {
        let analise = self.analise;
        let Some(itens) = analise.composicoes.as_ref().and_then(|m| m.get(&codigo)) else {
            return (0.0, false);
        };
        let (mut soma, mut incompleto) = (0.0, false);
        for item in itens {
            let cod = item.cod_insumo;
            let categoria = self.categoria(cod);
            let custo_cadastrado = analise.produtos.get(&cod).map_or(0.0, |p| p.preco_custo);
            let qtd_por_pai = item.qtd_com_perda();
            let qtd_total = qtd_pai * qtd_por_pai;

            let eh_composto = analise.composicoes.as_ref().is_some_and(|m| m.contains_key(&cod));
            let tipo = if caminho.contains(&cod) {
                TipoNo::Ciclo
            } else if eh_composto {
                TipoNo::Composto
            } else if eh_mao_de_obra(categoria, &item.descricao) {
                TipoNo::MaoDeObra
            } else {
                TipoNo::Material
            };

            let indice = self.nos.len();
            self.nos.push(No {
                nivel,
                codigo: cod,
                codigo_proprio: &item.codigo_proprio,
                descricao: &item.descricao,
                categoria,
                unidade: &item.unidade,
                tipo,
                qtd_por_pai,
                perc_perda: item.perc_perda,
                qtd_total,
                custo_unit: 0.0,
                custo_total: 0.0,
                custo_cadastrado,
                incompleto: false,
            });

            let (custo_unit, falta_custo) = match tipo {
                TipoNo::Ciclo => {
                    self.ciclos.insert(cod, &item.descricao);
                    (0.0, true)
                }
                TipoNo::Composto => {
                    caminho.push(cod);
                    let r = self.descer(cod, nivel + 1, qtd_total, caminho);
                    caminho.pop();
                    r
                }
                TipoNo::Material | TipoNo::MaoDeObra => {
                    let custo = custo_cadastrado.max(0.0);
                    if tipo == TipoNo::MaoDeObra {
                        self.custo_mao_de_obra += qtd_total * custo;
                    } else {
                        self.custo_materiais += qtd_total * custo;
                    }
                    if custo <= 0.0 {
                        self.sem_custo.insert(cod, (&item.descricao, categoria));
                    }
                    (custo, custo <= 0.0)
                }
            };

            let no = &mut self.nos[indice];
            no.custo_unit = custo_unit;
            no.custo_total = qtd_total * custo_unit;
            no.incompleto = falta_custo;
            soma += qtd_por_pai * custo_unit;
            incompleto |= falta_custo;
        }
        (soma, incompleto)
    }
}

/// Explode a composição de `qtd` unidades de um produto. `None` se ele não tiver composição cadastrada.
pub fn explodir(analise: &Analise, codigo: i64, qtd: f64) -> Option<Estrutura<'_>> {
    analise.composicoes.as_ref()?.get(&codigo)?;
    let produto = analise.produtos.get(&codigo)?;

    let mut e = Explosao {
        analise,
        nos: vec![No {
            nivel: 0,
            codigo,
            codigo_proprio: &produto.codigo_proprio,
            descricao: &produto.descricao,
            categoria: analise.categoria_de(produto.cod_categoria),
            unidade: &produto.unidade,
            tipo: TipoNo::Composto,
            qtd_por_pai: 1.0,
            perc_perda: 0.0,
            qtd_total: qtd,
            custo_unit: 0.0,
            custo_total: 0.0,
            custo_cadastrado: produto.preco_custo,
            incompleto: false,
        }],
        custo_materiais: 0.0,
        custo_mao_de_obra: 0.0,
        sem_custo: BTreeMap::new(),
        ciclos: BTreeMap::new(),
    };
    let (custo, incompleto) = e.descer(codigo, 1, qtd, &mut vec![codigo]);
    let raiz = &mut e.nos[0];
    raiz.custo_unit = custo;
    raiz.custo_total = qtd * custo;
    raiz.incompleto = incompleto;

    Some(Estrutura {
        niveis: e.nos.iter().map(|n| n.nivel).max().unwrap_or(0),
        nos: e.nos,
        custo_materiais: e.custo_materiais,
        custo_mao_de_obra: e.custo_mao_de_obra,
        sem_custo: e.sem_custo.into_values().collect(),
        ciclos: e.ciclos.into_values().collect(),
    })
}

#[cfg(test)]
mod testes {
    use serde_json::{Value, json};

    use super::*;
    use crate::coleta::DadosBrutos;

    /// (pai, [(filho, qtd, perda)])
    type Comp<'x> = (i64, &'x [(i64, f64, f64)]);

    /// `produtos`: (código, descrição, custo, cód. categoria).
    fn analise(produtos: &[(i64, &str, f64, i64)], composicoes: &[Comp]) -> Analise {
        let nome = |c: i64| produtos.iter().find(|p| p.0 == c).map_or("", |p| p.1);
        let composicoes: Vec<Value> = composicoes
            .iter()
            .map(|(pai, itens)| {
                let insumos: Vec<Value> = itens
                    .iter()
                    .map(|(f, q, p)| {
                        json!({"codInsumo": f.to_string(), "insumo": nome(*f), "codProprio": "", "unidade": "UN",
                               "quant": q.to_string(), "pPerda": p.to_string()})
                    })
                    .collect();
                json!({"cod": pai.to_string(), "produto": nome(*pai), "insumos": insumos})
            })
            .collect();
        Analise::de(&DadosBrutos {
            gerado_em: String::new(),
            ops: vec![],
            produtos: produtos
                .iter()
                .map(|(c, d, custo, cat)| json!({"codigo": c, "descricao": d, "precoCusto": custo, "codCategoria": cat}))
                .collect(),
            composicoes: Some(composicoes),
            categorias: vec![json!({"codigo": 14, "nome": "Mão-de-Obra"}), json!({"codigo": 13, "nome": "Compras"})],
        })
    }

    fn perto(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn custo_sobe_por_todos_os_niveis_com_perda() {
        // A = 2 B + 0,5 h de MO a R$ 10. B = 3 C com 10% de perda. C custa R$ 1.
        // O custo cadastrado de B (R$ 99) é só comparação e não entra na soma.
        let a = analise(
            &[(1, "A", 50.0, 13), (2, "B", 99.0, 13), (3, "C", 1.0, 13), (4, "MO - HORA", 10.0, 14)],
            &[(1, &[(2, 2.0, 0.0), (4, 0.5, 0.0)]), (2, &[(3, 3.0, 10.0)])],
        );
        let e = explodir(&a, 1, 1.0).unwrap();

        assert_eq!(
            e.nos.iter().map(|n| (n.nivel, n.descricao)).collect::<Vec<_>>(),
            [(0, "A"), (1, "B"), (2, "C"), (1, "MO - HORA")]
        );
        assert_eq!(e.niveis, 2);
        assert_eq!(e.nos[1].tipo, TipoNo::Composto);
        assert_eq!(e.nos[2].tipo, TipoNo::Material);
        assert_eq!(e.nos[3].tipo, TipoNo::MaoDeObra);

        perto(e.nos[2].qtd_por_pai, 3.3);
        perto(e.nos[2].qtd_total, 6.6);
        perto(e.nos[1].custo_unit, 3.3);
        perto(e.nos[1].custo_total, 6.6);
        perto(e.custo_produto(), 11.6);
        perto(e.custo_materiais, 6.6);
        perto(e.custo_mao_de_obra, 5.0);
        perto(e.nos[0].custo_cadastrado, 50.0);
        assert!(!e.nos[0].incompleto);
    }

    #[test]
    fn quantidade_multiplica_totais_e_mantem_valores_por_unidade() {
        // Mesma estrutura do teste acima, para 10 unidades de A.
        let a = analise(
            &[(1, "A", 50.0, 13), (2, "B", 99.0, 13), (3, "C", 1.0, 13), (4, "MO - HORA", 10.0, 14)],
            &[(1, &[(2, 2.0, 0.0), (4, 0.5, 0.0)]), (2, &[(3, 3.0, 10.0)])],
        );
        let e = explodir(&a, 1, 10.0).unwrap();

        perto(e.nos[0].qtd_total, 10.0);
        perto(e.nos[2].qtd_total, 66.0);
        perto(e.nos[0].custo_total, 116.0);
        perto(e.custo_materiais, 66.0);
        perto(e.custo_mao_de_obra, 50.0);
        let m = e.materiais_somados();
        perto(m[0].qtd_total, 66.0);
        perto(m[0].custo_total, 66.0);

        // Continuam por unidade.
        perto(e.custo_produto(), 11.6);
        perto(e.nos[2].qtd_por_pai, 3.3);
        perto(e.nos[1].custo_unit, 3.3);
        perto(m[0].custo_unit, 1.0);
    }

    #[test]
    fn insumo_sem_custo_marca_os_itens_acima_como_incompletos() {
        // A = B + D. B = C (sem custo). C também entra direto em A: é listado uma vez só.
        let a = analise(
            &[(1, "A", 0.0, 13), (2, "B", 0.0, 13), (3, "C", 0.0, 13), (4, "D", 2.0, 13)],
            &[(1, &[(2, 1.0, 0.0), (3, 1.0, 0.0), (4, 1.0, 0.0)]), (2, &[(3, 1.0, 0.0)])],
        );
        let e = explodir(&a, 1, 1.0).unwrap();

        assert!(e.nos[0].incompleto);
        assert!(e.nos[1].incompleto, "B contém C");
        assert!(!e.nos.last().unwrap().incompleto, "D tem custo");
        assert_eq!(e.sem_custo, [("C", "Compras")]);
        perto(e.custo_produto(), 2.0);
    }

    #[test]
    fn cadastro_circular_nao_trava_e_e_sinalizado() {
        // 5 contém ele mesmo; 1 → 2 → 1 é um ciclo indireto.
        let a = analise(
            &[(1, "A", 0.0, 13), (2, "B", 0.0, 13), (3, "C", 4.0, 13), (5, "E", 0.0, 13)],
            &[(1, &[(2, 1.0, 0.0), (3, 1.0, 0.0)]), (2, &[(1, 1.0, 0.0)]), (5, &[(5, 1.0, 0.0)])],
        );

        let direto = explodir(&a, 5, 1.0).unwrap();
        assert_eq!(direto.nos.len(), 2);
        assert_eq!(direto.nos[1].tipo, TipoNo::Ciclo);
        assert_eq!(direto.ciclos, ["E"]);
        assert!(direto.nos[0].incompleto);

        let indireto = explodir(&a, 1, 1.0).unwrap();
        assert_eq!(indireto.nos.iter().map(|n| n.descricao).collect::<Vec<_>>(), ["A", "B", "A", "C"]);
        assert_eq!(indireto.nos[2].tipo, TipoNo::Ciclo);
        assert_eq!(indireto.ciclos, ["A"]);
        perto(indireto.custo_produto(), 4.0);
    }

    #[test]
    fn mesmo_insumo_em_ramos_diferentes_nao_e_ciclo() {
        let a = analise(
            &[(1, "A", 0.0, 13), (2, "B", 0.0, 13), (3, "C", 0.0, 13), (4, "D", 1.0, 13)],
            &[(1, &[(2, 1.0, 0.0), (3, 1.0, 0.0)]), (2, &[(4, 2.0, 0.0)]), (3, &[(4, 3.0, 0.0)])],
        );
        let e = explodir(&a, 1, 1.0).unwrap();
        assert!(e.ciclos.is_empty());
        assert!(e.nos.iter().all(|n| n.tipo != TipoNo::Ciclo));
        perto(e.custo_produto(), 5.0);
    }

    #[test]
    fn material_em_varios_subprodutos_e_somado() {
        // A = 2 B + 3 C + 1 E + 1 h de MO. B = 2 D. C = 1 D com 10% de perda + 1 E. D custa R$ 2; E custa R$ 100.
        // D total = 2×2 + 3×1,1 = 7,3 em 2 linhas. E total = 1 + 3 = 4 em 2 linhas.
        let a = analise(
            &[
                (1, "A", 0.0, 13),
                (2, "B", 0.0, 13),
                (3, "C", 0.0, 13),
                (4, "D", 2.0, 13),
                (5, "E", 100.0, 13),
                (6, "MO - HORA", 10.0, 14),
            ],
            &[
                (1, &[(2, 2.0, 0.0), (3, 3.0, 0.0), (5, 1.0, 0.0), (6, 1.0, 0.0)]),
                (2, &[(4, 2.0, 0.0)]),
                (3, &[(4, 1.0, 10.0), (5, 1.0, 0.0)]),
            ],
        );
        let e = explodir(&a, 1, 1.0).unwrap();
        let m = e.materiais_somados();

        // Só materiais: sem os compostos B e C e sem a mão de obra. Maior custo primeiro.
        assert_eq!(m.iter().map(|x| x.descricao).collect::<Vec<_>>(), ["E", "D"]);
        assert_eq!(m[0].usos, 2);
        perto(m[0].qtd_total, 4.0);
        perto(m[0].custo_total, 400.0);
        assert_eq!(m[1].usos, 2);
        perto(m[1].qtd_total, 7.3);
        perto(m[1].custo_unit, 2.0);
        perto(m[1].custo_total, 14.6);
        perto(m.iter().map(|x| x.custo_total).sum(), e.custo_materiais);
    }

    #[test]
    fn soma_mantem_material_sem_custo_e_ignora_ciclo() {
        // A = 2 C (sem custo) + A (circular).
        let a = analise(&[(1, "A", 0.0, 13), (3, "C", 0.0, 13)], &[(1, &[(3, 2.0, 0.0), (1, 1.0, 0.0)])]);
        let m = explodir(&a, 1, 1.0).unwrap().materiais_somados();

        assert_eq!(m.iter().map(|x| x.descricao).collect::<Vec<_>>(), ["C"]);
        perto(m[0].qtd_total, 2.0);
        perto(m[0].custo_total, 0.0);
    }

    #[test]
    fn item_sem_composicao_nao_tem_estrutura() {
        let a = analise(&[(1, "A", 1.0, 13)], &[]);
        assert!(explodir(&a, 1, 1.0).is_none());
    }
}
