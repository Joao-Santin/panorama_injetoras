//! Interface gráfica (iced) do panorama de necessidades de materiais.

use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
    sync::Arc,
};

use iced::{
    Alignment::Center,
    Element, Font,
    Length::{Fill, FillPortion},
    Task, Theme, font,
    alignment::Horizontal::Right,
    widget::{
        Column, button, center, checkbox, column, container, operation, pick_list, progress_bar, row, rule, scrollable,
        space, table, text, text_input, toggler,
    },
};

use crate::{
    calculo::{self, Alerta, Necessidade, Resultado},
    coleta::{self, Analise, DadosBrutos, Progresso},
    colunas::{AcaoColuna, Coluna, ColunaNec, ColunaProd, ConfigColunas},
    estrutura::{self, Estrutura, MaterialSomado, No, TipoNo},
    fmt,
    modelo::{ItemComposicao, ItemOp, Op, Produto},
};

const NEGRITO: Font = Font { weight: font::Weight::Bold, ..Font::DEFAULT };
const ITENS_POR_PAGINA: usize = 300;
const ID_LISTA_PRODUTOS: &str = "lista_produtos";
const QTD_PECAS_PADRAO: &str = "1";
const AVISO_SEM_COMPOSICOES: &str =
    "Composições e categorias ainda não coletadas: clique em \"Atualizar do eGestor\" para buscá-las.";

type EstiloTexto = fn(&Theme) -> text::Style;
type EstiloBotao = fn(&Theme, button::Status) -> button::Style;

pub fn executar() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title(App::titulo)
        .theme(App::tema)
        .window_size((1400.0, 820.0))
        .run()
}

// ============================================================== estado ===

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aba {
    Resumo,
    Necessidades,
    MaoDeObra,
    Ops,
    Alertas,
    Produtos,
}

/// O que o banner da aba Produtos mostra do item selecionado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisaoComposicao {
    /// Só o primeiro nível, como está cadastrado.
    Direta,
    Bom,
    /// Materiais da BOM somados: quanto de cada um o produto usa no geral.
    SomaMateriais,
    Custeio,
}

/// Tabelas com colunas configuráveis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tabela {
    Necessidades,
    Produtos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdemNec {
    CustoFalta,
    Falta,
    Necessidade,
    Saldo,
    Nome,
}

impl OrdemNec {
    const TODAS: [OrdemNec; 5] = [Self::CustoFalta, Self::Falta, Self::Necessidade, Self::Saldo, Self::Nome];
}

impl std::fmt::Display for OrdemNec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CustoFalta => "Ordenar: maior custo da falta",
            Self::Falta => "Ordenar: maior falta (qtd)",
            Self::Necessidade => "Ordenar: maior necessidade",
            Self::Saldo => "Ordenar: menor saldo",
            Self::Nome => "Ordenar: nome (A–Z)",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiltroGrav {
    Todas,
    Alta,
    Media,
    Baixa,
}

impl FiltroGrav {
    const TODAS: [FiltroGrav; 4] = [Self::Todas, Self::Alta, Self::Media, Self::Baixa];
    fn aceita(self, g: &str) -> bool {
        match self {
            Self::Todas => true,
            Self::Alta => g == "ALTA",
            Self::Media => g == "MÉDIA",
            Self::Baixa => g == "BAIXA",
        }
    }
}

impl std::fmt::Display for FiltroGrav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Todas => "Todas as gravidades",
            Self::Alta => "Somente ALTA",
            Self::Media => "Somente MÉDIA",
            Self::Baixa => "Somente BAIXA",
        })
    }
}

/// Item da lista "Adicionar OP..." do grupo.
#[derive(Debug, Clone, PartialEq)]
pub struct OpcaoOp {
    codigo: i64,
    rotulo: String,
}

impl std::fmt::Display for OpcaoOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.rotulo)
    }
}

#[derive(Debug, Clone)]
pub enum Evento {
    Progresso(Progresso),
    Fim(Result<Arc<DadosBrutos>, String>),
}

#[derive(Debug, Clone)]
pub enum Message {
    Atualizar,
    Coleta(Evento),
    Exportar,
    AbrirArquivo(PathBuf),
    AlternarTema,
    Aba(Aba),
    BuscaNec(String),
    SoFaltas(bool),
    OrdemNec(OrdemNec),
    ModoConfigNec(bool),
    GrupoEntrada(String),
    GrupoAdicionarEntrada,
    GrupoAdicionarOp(OpcaoOp),
    GrupoAdicionarTag(String),
    GrupoAlternar(i64),
    GrupoRemover(i64),
    GrupoLimpar,
    Colunas(Tabela, AcaoColuna),
    SelMaterial(i64),
    FecharMaterial,
    VerMaterial(i64),
    BuscaOp(String),
    SoOpsComFalta(bool),
    SelOp(i64),
    VerOp(i64),
    FiltroGrav(FiltroGrav),
    BuscaProd(String),
    SoUsados(bool),
    ModoConfigProd(bool),
    SelProduto(i64),
    FecharProduto,
    PaginaProd(usize),
    VisaoComposicao(VisaoComposicao),
    QtdPecas(String),
}

/// Recorte de uma lista em páginas de `ITENS_POR_PAGINA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Paginacao {
    pagina: usize,
    ultima: usize,
    inicio: usize,
    fim: usize,
    total: usize,
}

impl Paginacao {
    /// A página pedida é limitada à última: a lista pode ter encolhido desde o clique.
    fn de(total: usize, pedida: usize) -> Self {
        let ultima = total.saturating_sub(1) / ITENS_POR_PAGINA;
        let pagina = pedida.min(ultima);
        let inicio = pagina * ITENS_POR_PAGINA;
        Self { pagina, ultima, inicio, fim: (inicio + ITENS_POR_PAGINA).min(total), total }
    }
}

pub struct App {
    analise: Option<Analise>,
    coletando: Option<Progresso>,
    status: Option<(bool, String)>,
    ultimo_excel: Option<PathBuf>,
    escuro: bool,
    aba: Aba,
    busca_nec: String,
    so_faltas: bool,
    ordem_nec: OrdemNec,
    config_nec: ConfigColunas<ColunaNec>,
    modo_config_nec: bool,
    /// OPs escolhidas para a pesquisa por grupo (vazio = necessidade geral).
    grupo_ops: BTreeSet<i64>,
    /// Necessidades recalculadas só para `grupo_ops` (None = usar a geral).
    res_grupo: Option<Resultado>,
    entrada_grupo: String,
    material_sel: Option<i64>,
    busca_op: String,
    so_ops_falta: bool,
    op_sel: Option<i64>,
    filtro_grav: FiltroGrav,
    busca_prod: String,
    so_usados: bool,
    config_prod: ConfigColunas<ColunaProd>,
    modo_config_prod: bool,
    produto_sel: Option<i64>,
    pagina_prod: usize,
    visao_composicao: VisaoComposicao,
    /// Texto do campo de quantidade da composição, como foi digitado.
    qtd_pecas: String,
}

fn iniciar_coleta() -> Task<Message> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    std::thread::spawn(move || {
        let tx_prog = tx.clone();
        let r = coleta::coletar(&coleta::url_api(), &mut |p| {
            let _ = tx_prog.unbounded_send(Evento::Progresso(p));
        })
        .and_then(|b| {
            coleta::salvar_cache(&b)?;
            Ok(b)
        });
        let _ = tx.unbounded_send(Evento::Fim(r.map(Arc::new).map_err(|e| format!("{e:#}"))));
    });
    Task::run(rx, Message::Coleta)
}

fn contem(alvo: &str, busca: &str) -> bool {
    busca.is_empty() || alvo.to_lowercase().contains(busca)
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let cache = coleta::carregar_cache();
        let sem_cache = cache.is_none();
        let app = Self {
            analise: cache.as_ref().map(Analise::de),
            coletando: sem_cache.then(|| Progresso { etapa: "Iniciando...".into(), fracao: 0.0 }),
            status: (!sem_cache)
                .then(|| (false, "Dados carregados do cache local. Clique em \"Atualizar do eGestor\" para buscar os atuais.".into())),
            ultimo_excel: None,
            escuro: false,
            aba: Aba::Resumo,
            busca_nec: String::new(),
            so_faltas: false,
            ordem_nec: OrdemNec::CustoFalta,
            config_nec: ConfigColunas::carregar(),
            modo_config_nec: false,
            grupo_ops: BTreeSet::new(),
            res_grupo: None,
            entrada_grupo: String::new(),
            material_sel: None,
            busca_op: String::new(),
            so_ops_falta: false,
            op_sel: None,
            filtro_grav: FiltroGrav::Todas,
            busca_prod: String::new(),
            so_usados: false,
            config_prod: ConfigColunas::carregar(),
            modo_config_prod: false,
            produto_sel: None,
            pagina_prod: 0,
            visao_composicao: VisaoComposicao::Direta,
            qtd_pecas: QTD_PECAS_PADRAO.into(),
        };
        let task = if sem_cache { iniciar_coleta() } else { Task::none() };
        (app, task)
    }

    fn titulo(&self) -> String {
        match &self.analise {
            Some(a) => format!("Panorama de Injetados — dados de {}", a.gerado_em),
            None => "Panorama de Injetados".into(),
        }
    }

    fn tema(&self) -> Theme {
        if self.escuro { Theme::Dark } else { Theme::Light }
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Atualizar => {
                if self.coletando.is_none() {
                    self.coletando = Some(Progresso { etapa: "Iniciando...".into(), fracao: 0.0 });
                    self.status = None;
                    return iniciar_coleta();
                }
            }
            Message::Coleta(Evento::Progresso(p)) => self.coletando = Some(p),
            Message::Coleta(Evento::Fim(Ok(b))) => {
                self.coletando = None;
                let a = Analise::de(&b);
                self.status = Some((
                    false,
                    format!("Dados atualizados: {} OPs em aberto, {} produtos.", a.ops.len(), a.produtos.len()),
                ));
                if self.op_sel.is_some_and(|c| !a.ops.iter().any(|o| o.codigo == c)) {
                    self.op_sel = None;
                }
                self.analise = Some(a);
                // Mantém o grupo, descartando OPs que não estão mais em aberto.
                let abertas: HashSet<i64> = self.analise.iter().flat_map(|a| &a.ops).map(|o| o.codigo).collect();
                self.grupo_ops.retain(|c| abertas.contains(c));
                self.recalcular_grupo();
            }
            Message::Coleta(Evento::Fim(Err(e))) => {
                self.coletando = None;
                self.status = Some((true, format!("Falha na coleta: {e}")));
            }
            Message::Exportar => {
                if let Some(a) = &self.analise {
                    match a.exportar_excel() {
                        Ok(p) => {
                            let abs = std::fs::canonicalize(&p).unwrap_or(p);
                            self.status = Some((false, format!("Planilha salva em {}", abs.display())));
                            self.ultimo_excel = Some(abs);
                        }
                        Err(e) => self.status = Some((true, format!("Erro ao gerar planilha: {e:#}"))),
                    }
                }
            }
            Message::AbrirArquivo(p) => {
                if let Err(e) = std::process::Command::new("explorer").arg(&p).spawn() {
                    self.status = Some((true, format!("Não consegui abrir {}: {e}", p.display())));
                }
            }
            Message::AlternarTema => self.escuro = !self.escuro,
            Message::Aba(a) => self.aba = a,
            Message::BuscaNec(s) => self.busca_nec = s,
            Message::SoFaltas(b) => self.so_faltas = b,
            Message::OrdemNec(o) => self.ordem_nec = o,
            Message::ModoConfigNec(b) => self.modo_config_nec = b,
            Message::GrupoEntrada(s) => self.entrada_grupo = s,
            Message::GrupoAdicionarEntrada => {
                let abertas: HashSet<i64> = self.analise.iter().flat_map(|a| &a.ops).map(|o| o.codigo).collect();
                let numeros: Vec<i64> = self
                    .entrada_grupo
                    .split(|c: char| !c.is_ascii_digit())
                    .filter_map(|s| s.parse().ok())
                    .collect();
                let (validas, invalidas): (Vec<i64>, Vec<i64>) = numeros.into_iter().partition(|c| abertas.contains(c));
                self.grupo_ops.extend(validas);
                self.status = (!invalidas.is_empty()).then(|| {
                    let lista = invalidas.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
                    (true, format!("OPs não encontradas entre as abertas (ignoradas): {lista}"))
                });
                self.entrada_grupo.clear();
                self.recalcular_grupo();
            }
            Message::GrupoAdicionarOp(o) => {
                self.grupo_ops.insert(o.codigo);
                self.recalcular_grupo();
            }
            Message::GrupoAdicionarTag(tag) => {
                if let Some(a) = &self.analise {
                    self.grupo_ops.extend(a.ops.iter().filter(|o| o.tags.contains(&tag)).map(|o| o.codigo));
                }
                self.recalcular_grupo();
            }
            Message::GrupoAlternar(c) => {
                if !self.grupo_ops.remove(&c) {
                    self.grupo_ops.insert(c);
                }
                self.recalcular_grupo();
            }
            Message::GrupoRemover(c) => {
                self.grupo_ops.remove(&c);
                self.recalcular_grupo();
            }
            Message::GrupoLimpar => {
                self.grupo_ops.clear();
                self.recalcular_grupo();
            }
            Message::Colunas(tabela, acao) => {
                let salvo = match tabela {
                    Tabela::Necessidades => self.config_nec.aplicar(acao),
                    Tabela::Produtos => self.config_prod.aplicar(acao),
                };
                if let Err(e) = salvo {
                    self.status = Some((true, format!("Não consegui salvar a configuração de colunas: {e:#}")));
                }
            }
            Message::SelMaterial(c) => self.material_sel = Some(c),
            Message::FecharMaterial => self.material_sel = None,
            Message::VerMaterial(c) => {
                self.material_sel = Some(c);
                self.aba = Aba::Necessidades;
            }
            Message::BuscaOp(s) => self.busca_op = s,
            Message::SoOpsComFalta(b) => self.so_ops_falta = b,
            Message::SelOp(c) => self.op_sel = Some(c),
            Message::VerOp(c) => {
                self.op_sel = Some(c);
                self.aba = Aba::Ops;
            }
            Message::FiltroGrav(f) => self.filtro_grav = f,
            Message::BuscaProd(s) => {
                self.busca_prod = s;
                return self.ir_para_pagina_prod(0);
            }
            Message::SoUsados(b) => {
                self.so_usados = b;
                return self.ir_para_pagina_prod(0);
            }
            Message::ModoConfigProd(b) => self.modo_config_prod = b,
            Message::SelProduto(c) => {
                // Cada produto começa em 1: a quantidade de um não vale para o outro.
                if self.produto_sel != Some(c) {
                    self.qtd_pecas = QTD_PECAS_PADRAO.into();
                }
                self.produto_sel = Some(c);
            }
            Message::FecharProduto => {
                self.produto_sel = None;
                self.visao_composicao = VisaoComposicao::Direta;
                self.qtd_pecas = QTD_PECAS_PADRAO.into();
            }
            Message::PaginaProd(p) => return self.ir_para_pagina_prod(p),
            Message::VisaoComposicao(v) => self.visao_composicao = v,
            Message::QtdPecas(s) => self.qtd_pecas = s,
        }
        Task::none()
    }

    /// Troca a página e volta a rolagem ao topo, para a nova página começar do primeiro item.
    fn ir_para_pagina_prod(&mut self, pagina: usize) -> Task<Message> {
        self.pagina_prod = pagina;
        operation::snap_to(ID_LISTA_PRODUTOS, scrollable::RelativeOffset::START)
    }

    /// Produtos que passam pela busca e pelo filtro da aba Produtos (sem ordem definida).
    fn produtos_filtrados<'a>(&self, a: &'a Analise) -> Vec<&'a Produto> {
        let busca = self.busca_prod.to_lowercase();
        let usados: HashSet<i64> = if self.so_usados {
            a.ops
                .iter()
                .flat_map(|o| &o.itens)
                .flat_map(|i| std::iter::once(i.cod_produto).chain(i.insumos.iter().map(|x| x.cod_insumo)))
                .collect()
        } else {
            HashSet::new()
        };
        a.produtos
            .values()
            .filter(|p| !self.so_usados || usados.contains(&p.codigo))
            .filter(|p| {
                contem(&p.descricao, &busca)
                    || contem(&p.codigo_proprio, &busca)
                    || contem(&p.tags.join(" "), &busca)
                    || contem(a.categoria_de(p.cod_categoria), &busca)
                    || p.codigo.to_string() == busca
            })
            .collect()
    }

    /// Refaz o levantamento só com as OPs do grupo (ou volta para o geral se vazio).
    fn recalcular_grupo(&mut self) {
        self.res_grupo = match &self.analise {
            Some(a) if !self.grupo_ops.is_empty() => {
                let ops: Vec<Op> = a.ops.iter().filter(|o| self.grupo_ops.contains(&o.codigo)).cloned().collect();
                Some(calculo::calcular(&ops, &a.produtos))
            }
            _ => None,
        };
    }

    /// Resultado usado na aba Necessidades: do grupo, se houver, senão o geral.
    fn res_necessidades<'a>(&'a self, a: &'a Analise) -> &'a Resultado {
        self.res_grupo.as_ref().unwrap_or(&a.res)
    }

    // ============================================================ view ===

    fn view(&self) -> Element<'_, Message> {
        let corpo: Element<'_, Message> = match &self.analise {
            None if self.coletando.is_some() => {
                center(text("Coletando dados do eGestor... (leva cerca de 1 minuto por causa do limite da API)").size(16))
                    .into()
            }
            None => center(
                column![
                    text("Nenhum dado carregado").size(20),
                    button("Atualizar do eGestor").on_press(Message::Atualizar).style(button::primary),
                ]
                .spacing(12)
                .align_x(Center),
            )
            .into(),
            Some(a) => match self.aba {
                Aba::Resumo => self.view_resumo(a),
                Aba::Necessidades => self.view_necessidades(a),
                Aba::MaoDeObra => self.view_mao_de_obra(a),
                Aba::Ops => self.view_ops(a),
                Aba::Alertas => self.view_alertas(a),
                Aba::Produtos => self.view_produtos(a),
            },
        };

        let mut tela = column![self.view_cabecalho()].spacing(10).padding(16);
        if let Some(a) = &self.analise {
            tela = tela.push(self.view_abas(a));
        }
        tela.push(rule::horizontal(1)).push(corpo).into()
    }

    fn view_cabecalho(&self) -> Element<'_, Message> {
        let sub = match &self.analise {
            Some(a) => format!("Dados coletados em {}  ·  gateway {}", a.gerado_em, coleta::url_api()),
            None => format!("Sem dados  ·  gateway {}", coleta::url_api()),
        };
        let topo = row![
            column![
                text("Panorama de Injetados").size(26).font(NEGRITO),
                text("Necessidades de materiais das OPs em aberto — eGestor").size(14),
                text(sub).size(12).style(text::secondary),
            ]
            .spacing(2),
            space::horizontal(),
            button(text(if self.escuro { "Tema claro" } else { "Tema escuro" }))
                .on_press(Message::AlternarTema)
                .style(button::text),
            button("Exportar Excel")
                .on_press_maybe(self.analise.as_ref().map(|_| Message::Exportar))
                .style(button::secondary),
            button(text(if self.coletando.is_some() { "Coletando..." } else { "Atualizar do eGestor" }))
                .on_press_maybe(self.coletando.is_none().then_some(Message::Atualizar))
                .style(button::primary),
        ]
        .spacing(8)
        .align_y(Center);

        let mut col = column![topo].spacing(8);
        if let Some(p) = &self.coletando {
            col = col.push(
                column![
                    progress_bar(0.0..=1.0, p.fracao).girth(8),
                    text(format!("{}  —  {:.0}%", p.etapa, p.fracao * 100.0)).size(12),
                ]
                .spacing(4),
            );
        }
        if let Some((erro, msg)) = &self.status {
            let estilo: EstiloTexto = if *erro { text::danger } else { text::success };
            let mut linha = row![text(msg.as_str()).size(13).style(estilo)].spacing(10).align_y(Center);
            if let (false, Some(p)) = (*erro, &self.ultimo_excel) {
                linha = linha.push(
                    button(text("Abrir planilha").size(12))
                        .padding([2, 10])
                        .on_press(Message::AbrirArquivo(p.clone()))
                        .style(button::secondary),
                );
            }
            col = col.push(linha);
        }
        col.into()
    }

    fn view_abas(&self, a: &Analise) -> Element<'_, Message> {
        let n_falta = a.tot.materiais_em_falta;
        let n_alta = a.res.alertas.iter().filter(|x| x.gravidade == "ALTA").count();
        let abas = [
            (Aba::Resumo, "Resumo".to_string()),
            (
                Aba::Necessidades,
                match &self.res_grupo {
                    Some(r) => {
                        let mat: Vec<_> = r.necessidades.iter().filter(|n| !n.mao_de_obra).collect();
                        format!(
                            "Necessidades — grupo de {} OPs ({} · {} em falta)",
                            self.grupo_ops.len(),
                            mat.len(),
                            mat.iter().filter(|n| n.falta() > 0.0).count()
                        )
                    }
                    None => format!("Necessidades ({} · {} em falta)", a.tot.materiais, n_falta),
                },
            ),
            (Aba::MaoDeObra, "Mão de obra".to_string()),
            (Aba::Ops, format!("OPs em aberto ({})", a.ops.len())),
            (Aba::Alertas, format!("Alertas ({}{})", a.res.alertas.len(), if n_alta > 0 { format!(" · {n_alta} ALTA") } else { String::new() })),
            (
                Aba::Produtos,
                match (self.produtos_filtrados(a).len(), a.produtos.len()) {
                    (filtrados, total) if filtrados == total => format!("Produtos ({total})"),
                    (filtrados, total) => format!("Produtos ({filtrados} de {total})"),
                },
            ),
        ];
        let mut r = row![].spacing(4);
        for (aba, rotulo) in abas {
            let estilo: EstiloBotao = if self.aba == aba { button::primary } else { button::text };
            r = r.push(button(text(rotulo).size(14)).padding([6, 14]).style(estilo).on_press(Message::Aba(aba)));
        }
        r.into()
    }

    // --------------------------------------------------------- Resumo ---

    fn view_resumo<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let t = &a.tot;
        let n_alertas = a.res.alertas.len();
        let estilo_falta: EstiloTexto = if t.materiais_em_falta > 0 { text::danger } else { text::success };

        let cards1 = row![
            card("OPs em aberto", fmt::num(t.num_ops as f64, 0), format!("{} itens de produto", t.num_itens), text::base),
            card("Peças a produzir", fmt::num(t.pecas, 0), "soma das quantidades das OPs".into(), text::base),
            card("Peso das peças", format!("{} kg", fmt::num(t.peso_pecas_kg, 1)), "peso líquido × quantidade".into(), text::base),
            card("Horas-máquina / MO", format!("{} h", fmt::num(t.horas_maquina, 1)), "ver aba Mão de obra".into(), text::base),
        ]
        .spacing(12);
        let cards2 = row![
            card("Materiais necessários", fmt::num(t.materiais as f64, 0), "insumos diferentes nas OPs".into(), text::base),
            card("Materiais em falta", fmt::num(t.materiais_em_falta as f64, 0), "estoque menor que a necessidade".into(), estilo_falta),
            card("Custo estimado das faltas", fmt::moeda(t.valor_faltas), "falta × preço de custo".into(), estilo_falta),
            card(
                "Alertas de cadastro",
                fmt::num(n_alertas as f64, 0),
                "confira antes de comprar".into(),
                if n_alertas > 0 { text::warning } else { text::success },
            ),
        ]
        .spacing(12);

        // Maiores faltas
        let mut faltas: Vec<&Necessidade> =
            a.res.necessidades.iter().filter(|n| !n.mao_de_obra && n.falta() > 0.0).collect();
        faltas.sort_by(|x, y| y.custo_falta().total_cmp(&x.custo_falta()));
        let max = faltas.iter().map(|n| n.custo_falta()).fold(0.0_f64, f64::max).max(1e-9);
        let mut lista = column![].spacing(10);
        for n in faltas.iter().take(15) {
            let custo = n.custo_falta();
            lista = lista.push(
                column![
                    button(text(n.descricao.as_str()).size(13))
                        .padding(0)
                        .style(button::text)
                        .on_press(Message::VerMaterial(n.cod_insumo)),
                    row![
                        progress_bar(0.0..=max as f32, custo as f32).girth(10).style(progress_bar::danger),
                        text(format!(
                            "falta {} {}  ·  {}{}",
                            fmt::qtd(n.falta()),
                            n.unidade,
                            fmt::moeda(custo),
                            if n.categoria == "Clientes" { "  ·  fornecido pelo cliente" } else { "" }
                        ))
                        .size(12)
                        .width(330),
                    ]
                    .spacing(10)
                    .align_y(Center),
                ]
                .spacing(2),
            );
        }
        if faltas.is_empty() {
            lista = lista.push(text("Nenhum material em falta.").style(text::success));
        }
        let painel_faltas = painel("Maiores faltas (por custo)", scrollable(lista).into());

        // Alertas ALTA + OPs mais pesadas
        let mut alertas = column![].spacing(8);
        for al in a.res.alertas.iter().filter(|x| x.gravidade == "ALTA") {
            alertas = alertas.push(linha_alerta(al));
        }
        if a.res.alertas.iter().all(|x| x.gravidade != "ALTA") {
            alertas = alertas.push(text("Nenhum alerta de gravidade ALTA.").size(13).style(text::success));
        }

        let mut por_peso: Vec<(&Op, f64)> = a.ops.iter().map(|o| (o, o.peso(&a.produtos))).collect();
        por_peso.sort_by(|x, y| y.1.total_cmp(&x.1));
        let max_peso = por_peso.first().map(|x| x.1).unwrap_or(0.0).max(1e-9);
        let mut pesadas = column![].spacing(6);
        for (o, p) in por_peso.iter().take(8) {
            pesadas = pesadas.push(
                row![
                    button(text(format!("OP {}", o.codigo)).size(13))
                        .padding(0)
                        .style(button::text)
                        .on_press(Message::VerOp(o.codigo))
                        .width(80),
                    progress_bar(0.0..=max_peso as f32, *p as f32).girth(10),
                    text(format!("{} kg", fmt::num(*p, 1))).size(12).width(90).align_x(Right),
                ]
                .spacing(8)
                .align_y(Center),
            );
        }

        let lado = column![
            painel("Alertas críticos", scrollable(alertas).into()),
            painel("OPs com maior peso a injetar", pesadas.into()),
        ]
        .spacing(12)
        .width(FillPortion(2));

        column![
            cards1,
            cards2,
            row![container(painel_faltas).width(FillPortion(3)), lado].spacing(12).height(Fill),
        ]
        .spacing(12)
        .into()
    }

    // --------------------------------------------------- Necessidades ---

    fn view_necessidades<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let res = self.res_necessidades(a);
        let busca = self.busca_nec.to_lowercase();
        let mut linhas: Vec<&'a Necessidade> = res
            .necessidades
            .iter()
            .filter(|n| !n.mao_de_obra)
            .filter(|n| !self.so_faltas || n.falta() > 0.0)
            .filter(|n| {
                contem(&n.descricao, &busca)
                    || contem(&n.codigo_proprio, &busca)
                    || contem(&n.categoria, &busca)
                    || n.tags.iter().any(|t| contem(t, &busca))
                    || n.cod_insumo.to_string() == busca
            })
            .collect();
        match self.ordem_nec {
            OrdemNec::CustoFalta => linhas.sort_by(|x, y| {
                y.custo_falta().total_cmp(&x.custo_falta()).then(y.falta().total_cmp(&x.falta()))
            }),
            OrdemNec::Falta => linhas.sort_by(|x, y| y.falta().total_cmp(&x.falta())),
            OrdemNec::Necessidade => linhas.sort_by(|x, y| y.necessidade.total_cmp(&x.necessidade)),
            OrdemNec::Saldo => linhas.sort_by(|x, y| x.saldo().total_cmp(&y.saldo())),
            OrdemNec::Nome => linhas.sort_by(|x, y| x.descricao.cmp(&y.descricao)),
        }
        let custo_total: f64 = linhas.iter().map(|n| n.custo_falta()).sum();

        let filtros = row![
            text_input("Buscar material, código, categoria ou tag...", &self.busca_nec)
                .on_input(Message::BuscaNec)
                .width(340),
            toggler(self.so_faltas).label("Somente com falta").on_toggle(Message::SoFaltas),
            pick_list(OrdemNec::TODAS, Some(self.ordem_nec), Message::OrdemNec),
            space::horizontal(),
            text(format!("{} materiais  ·  custo das faltas listadas: {}", linhas.len(), fmt::moeda(custo_total))).size(13),
            botao_config_colunas(self.modo_config_nec, Message::ModoConfigNec(!self.modo_config_nec)),
        ]
        .spacing(14)
        .align_y(Center);

        let sel = self.material_sel;
        let colunas: Vec<_> = self.config_nec.visiveis().map(|c| coluna_necessidade(c, sel)).collect();
        let tabela = rolagem(table::table(colunas, linhas).padding_x(8).padding_y(4).width(Fill));
        // Em modo de configuração, o painel fica à direita da tabela (prévia ao vivo).
        let tabela: Element<'a, Message> = if self.modo_config_nec {
            row![container(tabela).width(Fill), view_config_colunas(&self.config_nec, Tabela::Necessidades)].spacing(12).into()
        } else {
            tabela.into()
        };

        let mut tela = column![self.view_grupo_ops(a), filtros].spacing(10);
        match sel.and_then(|c| res.necessidades.iter().find(|n| n.cod_insumo == c)) {
            Some(n) => {
                tela = tela.push(container(tabela).height(FillPortion(3)));
                tela = tela.push(container(self.view_detalhe_material(res, n)).height(FillPortion(2)));
            }
            None => {
                tela = tela.push(container(tabela).height(Fill));
                tela = tela.push(text("Clique num material para ver em quais OPs e produtos ele é usado.").size(12).style(text::secondary));
            }
        }
        tela.into()
    }

    /// Barra "Grupo de OPs": define se a pesquisa é geral ou só de algumas OPs.
    fn view_grupo_ops<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let opcoes: Vec<OpcaoOp> = a
            .ops
            .iter()
            .rev()
            .filter(|o| !self.grupo_ops.contains(&o.codigo))
            .map(|o| {
                let prod: String = o.itens.first().map(|i| i.descricao.chars().take(45).collect()).unwrap_or_default();
                let extra = if o.itens.len() > 1 { format!(" +{}", o.itens.len() - 1) } else { String::new() };
                OpcaoOp { codigo: o.codigo, rotulo: format!("OP {} — {prod}{extra}", o.codigo) }
            })
            .collect();
        let mut tags: Vec<String> = a.ops.iter().flat_map(|o| o.tags.iter().cloned()).collect();
        tags.sort();
        tags.dedup();

        let controles = row![
            text("Grupo de OPs").size(14).font(NEGRITO),
            text_input("Nº das OPs (ex.: 1323, 1325) + Enter", &self.entrada_grupo)
                .on_input(Message::GrupoEntrada)
                .on_submit(Message::GrupoAdicionarEntrada)
                .size(13)
                .width(260),
            button(text("Adicionar").size(13))
                .style(button::secondary)
                .on_press_maybe((!self.entrada_grupo.trim().is_empty()).then_some(Message::GrupoAdicionarEntrada)),
            pick_list(opcoes, None::<OpcaoOp>, Message::GrupoAdicionarOp)
                .placeholder("Escolher OP da lista...")
                .text_size(13)
                .width(330),
            pick_list(tags, None::<String>, Message::GrupoAdicionarTag)
                .placeholder("Adicionar OPs por tag...")
                .text_size(13)
                .width(220),
            space::horizontal(),
            button(text("Limpar grupo").size(13))
                .style(button::danger)
                .on_press_maybe((!self.grupo_ops.is_empty()).then_some(Message::GrupoLimpar)),
        ]
        .spacing(10)
        .align_y(Center);

        let mut col = column![controles].spacing(8);
        if self.grupo_ops.is_empty() {
            col = col.push(
                text(format!(
                    "Nenhuma OP no grupo — mostrando a necessidade GERAL de todas as {} OPs em aberto.",
                    a.ops.len()
                ))
                .size(12)
                .style(text::secondary),
            );
        } else {
            let ops: Vec<&Op> = a.ops.iter().filter(|o| self.grupo_ops.contains(&o.codigo)).collect();
            let pecas: f64 = ops.iter().map(|o| o.pecas()).sum();
            let peso: f64 = ops.iter().map(|o| o.peso(&a.produtos)).sum();
            let mut chips = row![].spacing(6);
            for o in &ops {
                chips = chips.push(
                    button(text(format!("OP {}   ✕", o.codigo)).size(12))
                        .padding([3, 10])
                        .style(cartao_selecionado)
                        .on_press(Message::GrupoRemover(o.codigo)),
                );
            }
            col = col
                .push(
                    text(format!(
                        "Pesquisa POR GRUPO: necessidade calculada só para {} OP(s) — {} peças, {} kg. \
                         O estoque comparado é o estoque total atual. Clique numa OP abaixo para removê-la.",
                        ops.len(),
                        fmt::qtd(pecas),
                        fmt::num(peso, 1)
                    ))
                    .size(12)
                    .style(text::primary),
                )
                .push(chips.wrap().vertical_spacing(6));
        }
        let estilo: fn(&Theme) -> container::Style =
            if self.grupo_ops.is_empty() { container::bordered_box } else { grupo_ativo };
        container(col).padding(10).width(Fill).style(estilo).into()
    }

    fn view_detalhe_material<'a>(&'a self, res: &'a Resultado, n: &'a Necessidade) -> Element<'a, Message> {
        let linhas: Vec<_> = res.detalhe.iter().filter(|d| d.cod_insumo == n.cod_insumo).collect();
        let total = n.necessidade.max(1e-12);
        let colunas = vec![
            table::column(cab("OP"), |d: &'a crate::calculo::LinhaDetalhe| {
                button(text(format!("OP {}", d.op)).size(13)).padding([1, 4]).style(button::text).on_press(Message::VerOp(d.op))
            })
            .width(FillPortion(1)),
            table::column(cab("Tags da OP"), |d: &'a crate::calculo::LinhaDetalhe| cel(d.op_tags.as_str())).width(FillPortion(3)),
            table::column(cab("Produto"), |d: &'a crate::calculo::LinhaDetalhe| cel(d.produto.as_str())).width(FillPortion(5)),
            col_num("Qtd. OP", 2, |d: &'a crate::calculo::LinhaDetalhe| cel(fmt::qtd(d.quant))),
            col_num("Consumo/pç", 2, |d: &'a crate::calculo::LinhaDetalhe| cel(fmt::num(d.consumo_un, 6))),
            col_num("Necessidade", 2, |d: &'a crate::calculo::LinhaDetalhe| cel(fmt::qtd(d.necessidade))),
            col_num("% do total", 1, move |d: &'a crate::calculo::LinhaDetalhe| {
                cel(format!("{}%", fmt::num(100.0 * d.necessidade / total, 1)))
            }),
        ];
        let titulo = row![
            column![
                text(n.descricao.as_str()).size(16).font(NEGRITO),
                text(format!(
                    "Necessidade {} {}  ·  estoque {}  ·  saldo {}  ·  {} OPs  ·  {} produtos",
                    fmt::qtd(n.necessidade),
                    n.unidade,
                    fmt::qtd(n.estoque),
                    fmt::qtd(n.saldo()),
                    n.ops.len(),
                    n.produtos.len()
                ))
                .size(12)
                .style(text::secondary),
            ],
            space::horizontal(),
            button(text("Fechar").size(12)).on_press(Message::FecharMaterial).style(button::secondary),
        ]
        .align_y(Center);
        container(
            column![titulo, rolagem(table::table(colunas, linhas).padding_x(8).padding_y(3).width(Fill))]
                .spacing(8),
        )
        .padding(12)
        .style(container::bordered_box)
        .height(Fill)
        .into()
    }

    // ---------------------------------------------------- Mão de obra ---

    fn view_mao_de_obra<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let mo: Vec<&'a Necessidade> = a.res.necessidades.iter().filter(|n| n.mao_de_obra).collect();
        let colunas = vec![
            table::column(cab("Insumo de mão de obra"), |n: &'a Necessidade| cel(n.descricao.as_str())).width(FillPortion(6)),
            table::column(cab("Un."), |n: &'a Necessidade| cel(n.unidade.as_str())).width(FillPortion(1)),
            col_num("Horas", 2, |n: &'a Necessidade| cel(fmt::num(n.necessidade, 2))),
            col_num("Custo/h", 2, |n: &'a Necessidade| cel(fmt::moeda(n.custo_unit))),
            col_num("Custo total", 2, |n: &'a Necessidade| cel(fmt::moeda(n.custo_necessidade()))),
            col_num("OPs", 1, |n: &'a Necessidade| cel(n.ops.len().to_string())),
        ];

        let horas = a.res.horas_por_op();
        let max = horas.first().map(|x| x.horas).unwrap_or(0.0).max(1e-9);

        let mut lista = column![].spacing(6);
        for h in horas {
            let estilo: EstiloTexto = if h.suspeita { text::danger } else { text::base };
            lista = lista.push(
                row![
                    button(text(format!("OP {}", h.op)).size(13)).padding(0).style(button::text).on_press(Message::VerOp(h.op)).width(80),
                    text(h.tags).size(12).style(text::secondary).width(260),
                    progress_bar(0.0..=max as f32, h.horas as f32).girth(10),
                    text(format!("{} h{}", fmt::num(h.horas, 1), if h.suspeita { "  (verificar!)" } else { "" }))
                        .size(12)
                        .width(170)
                        .align_x(Right)
                        .style(estilo),
                ]
                .spacing(8)
                .align_y(Center),
            );
        }

        column![
            painel("Totais por insumo de mão de obra", table::table(colunas, mo).padding_x(8).padding_y(4).width(Fill).into()),
            container(painel("Horas por OP", scrollable(lista).into())).height(Fill),
        ]
        .spacing(12)
        .into()
    }

    // ------------------------------------------------------------ OPs ---

    fn view_ops<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let busca = self.busca_op.to_lowercase();
        let ops_com_falta = a.res.ops_com_falta();

        let mut lista = column![].spacing(6);
        let mut n = 0;
        for o in a.ops.iter().rev() {
            let texto_busca = format!(
                "{} {} {}",
                o.codigo,
                o.tags.join(" "),
                o.itens.iter().map(|i| i.descricao.as_str()).collect::<Vec<_>>().join(" ")
            );
            if !contem(&texto_busca, &busca) {
                continue;
            }
            let falta = ops_com_falta.contains(&o.codigo);
            if self.so_ops_falta && !falta {
                continue;
            }
            n += 1;
            let principal = o.itens.first().map(|i| i.descricao.as_str()).unwrap_or("(sem produtos)");
            let extra = if o.itens.len() > 1 { format!(" + {} itens", o.itens.len() - 1) } else { String::new() };
            let mut linha2 = row![text(o.tags.join(" · ")).size(11).style(text::secondary)].spacing(6);
            if falta {
                linha2 = linha2.push(space::horizontal()).push(text("falta material").size(11).style(text::danger));
            }
            let estilo: EstiloBotao = if self.op_sel == Some(o.codigo) { cartao_selecionado } else { cartao };
            lista = lista.push(
                button(
                    column![
                        row![
                            text(format!("OP {}", o.codigo)).size(14).font(NEGRITO),
                            space::horizontal(),
                            text(format!("início {}", fmt::data(&o.dt_inicio))).size(11),
                        ],
                        text(format!("{principal}{extra}")).size(12),
                        linha2,
                    ]
                    .spacing(2),
                )
                .width(Fill)
                .padding(8)
                .style(estilo)
                .on_press(Message::SelOp(o.codigo)),
            );
        }

        let esquerda = column![
            text_input("Buscar OP, produto ou tag...", &self.busca_op).on_input(Message::BuscaOp),
            toggler(self.so_ops_falta).label("Somente OPs com falta de material").on_toggle(Message::SoOpsComFalta),
            text(format!("{n} OPs")).size(12).style(text::secondary),
            scrollable(lista.padding([0, 12])).height(Fill),
        ]
        .spacing(8)
        .width(380);

        let direita: Element<'a, Message> = match self.op_sel.and_then(|c| a.ops.iter().find(|o| o.codigo == c)) {
            Some(o) => self.view_detalhe_op(a, o),
            None => center(text("Selecione uma OP à esquerda para ver produtos, composição e disponibilidade de material.").style(text::secondary)).into(),
        };

        row![esquerda, rule::vertical(1), container(direita).width(Fill).height(Fill)].spacing(12).into()
    }

    fn view_detalhe_op<'a>(&'a self, a: &'a Analise, o: &'a Op) -> Element<'a, Message> {
        let no_grupo = self.grupo_ops.contains(&o.codigo);
        let estilo_grupo: EstiloBotao = if no_grupo { button::secondary } else { button::primary };
        let cab_op = column![
            row![
                text(format!("OP {}", o.codigo)).size(22).font(NEGRITO),
                space::horizontal(),
                button(
                    text(if no_grupo {
                        format!("Remover do grupo de necessidades ({} OPs)", self.grupo_ops.len())
                    } else {
                        "Adicionar ao grupo de necessidades".to_string()
                    })
                    .size(13)
                )
                .style(estilo_grupo)
                .on_press(Message::GrupoAlternar(o.codigo)),
            ]
            .align_y(Center),
            text(o.tags.join("  ·  ")).size(13),
            text(format!(
                "Cadastro {}  ·  início {}  ·  entrega {}  ·  {} itens  ·  {} peças  ·  {} kg  ·  {}",
                fmt::data(&o.dt_cad),
                fmt::data(&o.dt_inicio),
                if o.dt_entrega.is_empty() { "—".to_string() } else { fmt::data(&o.dt_entrega) },
                o.itens.len(),
                fmt::qtd(o.pecas()),
                fmt::num(o.peso(&a.produtos), 1),
                fmt::moeda(o.valor_total),
            ))
            .size(12)
            .style(text::secondary),
        ]
        .spacing(4);

        let mut col = column![cab_op].spacing(12);
        if !o.obs.is_empty() {
            col = col.push(text(format!("Obs.: {}", o.obs)).size(12));
        }
        for al in a.res.alertas.iter().filter(|x| x.op == Some(o.codigo)) {
            col = col.push(linha_alerta(al));
        }
        for item in &o.itens {
            col = col.push(self.view_item_op(a, item));
        }
        scrollable(col.padding([0, 12])).height(Fill).into()
    }

    fn view_item_op<'a>(&'a self, a: &'a Analise, item: &'a ItemOp) -> Element<'a, Message> {
        let estoque_prod = a.produtos.get(&item.cod_produto).map(|p| p.estoque).unwrap_or(0.0);
        let nec: std::collections::HashMap<i64, &'a Necessidade> =
            a.res.necessidades.iter().map(|n| (n.cod_insumo, n)).collect();
        let quant = item.quant;

        type Linha<'x> = (&'x crate::modelo::Insumo, Option<&'x Necessidade>);
        let linhas: Vec<Linha<'a>> = item.insumos.iter().map(|i| (i, nec.get(&i.cod_insumo).copied())).collect();

        let colunas = vec![
            table::column(cab("Insumo"), |(i, _): Linha<'a>| {
                button(text(i.descricao.as_str()).size(13))
                    .padding([1, 4])
                    .style(button::text)
                    .on_press(Message::VerMaterial(i.cod_insumo))
            })
            .width(FillPortion(6)),
            table::column(cab("Categoria"), |(i, _): Linha<'a>| cel(i.categoria.as_str())).width(FillPortion(2)),
            table::column(cab("Un."), |(_, n): Linha<'a>| cel(n.map(|n| n.unidade.as_str()).unwrap_or(""))).width(FillPortion(1)),
            col_num("Consumo/pç", 2, |(i, _): Linha<'a>| cel(fmt::num(i.qtd_por_un, 6))),
            col_num("Nesta OP", 2, move |(i, _): Linha<'a>| cel(fmt::qtd(i.qtd_por_un * quant)).font(NEGRITO)),
            col_num("Estoque", 2, |(_, n): Linha<'a>| cel(n.map(|n| fmt::qtd(n.estoque)).unwrap_or_default())),
            col_num("Todas as OPs", 2, |(_, n): Linha<'a>| cel(n.map(|n| fmt::qtd(n.necessidade)).unwrap_or_default())),
            table::column(cab("Situação"), move |(i, n): Linha<'a>| {
                let nesta = i.qtd_por_un * quant;
                let (msg, estilo): (String, EstiloTexto) = match n {
                    _ if crate::calculo::eh_mao_de_obra(&i.categoria, &i.descricao) => ("mão de obra".into(), text::secondary),
                    None => ("—".into(), text::secondary),
                    Some(n) if n.falta() <= 0.0 => ("OK — cobre todas as OPs".into(), text::success),
                    Some(n) if n.estoque >= nesta => (format!("cobre esta OP; falta {} no total", fmt::qtd(n.falta())), text::warning),
                    Some(n) => (format!("FALTA {} nesta OP", fmt::qtd(nesta - n.estoque.max(0.0))), text::danger),
                };
                cel(msg).style(estilo)
            })
            .width(FillPortion(4)),
        ];

        let titulo = row![
            text(item.descricao.as_str()).size(15).font(NEGRITO),
            space::horizontal(),
            text(format!(
                "{} pç  ·  peso {} kg/pç  ·  total {} kg  ·  estoque do produto {}",
                fmt::qtd(item.quant),
                fmt::num(item.peso_un(&a.produtos), 4),
                fmt::num(item.peso(&a.produtos), 1),
                fmt::qtd(estoque_prod)
            ))
            .size(12)
            .style(text::secondary),
        ]
        .align_y(Center);

        container(column![titulo, table::table(colunas, linhas).padding_x(8).padding_y(3).width(Fill)].spacing(8))
            .padding(12)
            .style(container::bordered_box)
            .width(Fill)
            .into()
    }

    // -------------------------------------------------------- Alertas ---

    fn view_alertas<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let linhas: Vec<&'a Alerta> = a.res.alertas.iter().filter(|x| self.filtro_grav.aceita(x.gravidade)).collect();
        let conta = |g: &str| a.res.alertas.iter().filter(|x| x.gravidade == g).count();
        let filtros = row![
            pick_list(FiltroGrav::TODAS, Some(self.filtro_grav), Message::FiltroGrav),
            text(format!("ALTA: {}   ·   MÉDIA: {}   ·   BAIXA: {}", conta("ALTA"), conta("MÉDIA"), conta("BAIXA"))).size(13),
            space::horizontal(),
            text("Alertas são inconsistências de cadastro que distorcem o levantamento.").size(12).style(text::secondary),
        ]
        .spacing(14)
        .align_y(Center);

        let colunas = vec![
            table::column(cab("Gravidade"), |x: &'a Alerta| {
                cel(x.gravidade).font(NEGRITO).style(estilo_gravidade(x.gravidade))
            })
            .width(FillPortion(1)),
            table::column(cab("Tipo"), |x: &'a Alerta| cel(x.tipo)).width(FillPortion(2)),
            table::column(cab("OP"), |x: &'a Alerta| -> Element<'a, Message> {
                match x.op {
                    Some(op) => button(text(format!("OP {op}")).size(13)).padding([1, 4]).style(button::text).on_press(Message::VerOp(op)).into(),
                    None => cel("—").into(),
                }
            })
            .width(FillPortion(1)),
            table::column(cab("Produto / material"), |x: &'a Alerta| cel(x.descricao.as_str())).width(FillPortion(4)),
            table::column(cab("Mensagem"), |x: &'a Alerta| cel(x.mensagem.as_str())).width(FillPortion(7)),
        ];
        column![filtros, rolagem(table::table(colunas, linhas).padding_x(8).padding_y(4).width(Fill)).height(Fill)]
            .spacing(10)
            .into()
    }

    // ------------------------------------------------------- Produtos ---

    fn view_produtos<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let mut todos = self.produtos_filtrados(a);
        // O código desempata descrições iguais, para cada item ficar sempre na mesma página.
        todos.sort_by(|x, y| x.descricao.cmp(&y.descricao).then(x.codigo.cmp(&y.codigo)));
        let pag = Paginacao::de(todos.len(), self.pagina_prod);
        let linhas = todos[pag.inicio..pag.fim].to_vec();

        let filtros = row![
            text_input("Buscar produto, código, categoria ou tag...", &self.busca_prod).on_input(Message::BuscaProd).width(340),
            toggler(self.so_usados).label("Somente itens usados nas OPs em aberto").on_toggle(Message::SoUsados),
            space::horizontal(),
            text(format!("{} produtos", pag.total)).size(13),
            botao_config_colunas(self.modo_config_prod, Message::ModoConfigProd(!self.modo_config_prod)),
        ]
        .spacing(14)
        .align_y(Center);

        let sel = self.produto_sel;
        let colunas: Vec<_> = self.config_prod.visiveis().map(|c| coluna_produto(c, a, sel)).collect();
        let mut lista = column![].spacing(8);
        if pag.ultima > 0 {
            lista = lista.push(view_paginacao(pag));
        }
        lista = lista.push(table::table(colunas, linhas).padding_x(8).padding_y(4).width(Fill));
        if pag.ultima > 0 {
            lista = lista.push(view_paginacao(pag));
        }
        let tabela = rolagem(lista).id(ID_LISTA_PRODUTOS);
        // Em modo de configuração, o painel fica à direita da tabela (prévia ao vivo).
        let tabela: Element<'a, Message> = if self.modo_config_prod {
            row![container(tabela).width(Fill), view_config_colunas(&self.config_prod, Tabela::Produtos)].spacing(12).into()
        } else {
            tabela.into()
        };

        let mut tela = column![filtros].spacing(10);
        match sel.and_then(|c| a.produtos.get(&c)) {
            Some(p) => {
                // A árvore multinível precisa de mais espaço que a composição direta.
                let (lista, banner) = match self.visao_composicao {
                    VisaoComposicao::Direta => (3, 2),
                    VisaoComposicao::Bom | VisaoComposicao::SomaMateriais | VisaoComposicao::Custeio => (1, 2),
                };
                tela = tela.push(container(tabela).height(FillPortion(lista)));
                tela = tela.push(container(view_composicao(a, p, self.visao_composicao, &self.qtd_pecas)).height(FillPortion(banner)));
            }
            None => {
                tela = tela.push(container(tabela).height(Fill));
                let dica = if a.composicoes.is_some() {
                    text("Clique na descrição de um item para ver a composição cadastrada.").size(12).style(text::secondary)
                } else {
                    text(AVISO_SEM_COMPOSICOES).size(12).style(text::warning)
                };
                tela = tela.push(dica);
            }
        }
        tela.into()
    }
}

// ============================================================ helpers ===

fn botao_config_colunas<'a>(ativo: bool, ao_clicar: Message) -> Element<'a, Message> {
    let estilo: EstiloBotao = if ativo { button::primary } else { button::secondary };
    button(text(if ativo { "Concluir configuração" } else { "Configurar colunas" }).size(13))
        .style(estilo)
        .on_press(ao_clicar)
        .into()
}

/// Painel do modo de configuração: marcar/desmarcar colunas e mudar a ordem.
fn view_config_colunas<'a, C: Coluna>(cfg: &'a ConfigColunas<C>, tabela: Tabela) -> Element<'a, Message> {
    let acao = move |a: AcaoColuna| Message::Colunas(tabela, a);
    let total = cfg.colunas.len();
    let mut lista = column![].spacing(4);
    for (i, (c, visivel)) in cfg.colunas.iter().enumerate() {
        let (c, visivel) = (*c, *visivel);
        let mut marcador = checkbox(visivel).label(c.rotulo()).text_size(13);
        if !c.obrigatoria() {
            marcador = marcador.on_toggle(move |v| acao(AcaoColuna::Visivel(i, v)));
        }
        let seta = |rotulo: &'static str, destino: Option<usize>| {
            button(text(rotulo).size(13))
                .padding([2, 8])
                .style(button::secondary)
                .on_press_maybe(destino.map(|d| acao(AcaoColuna::Mover(i, d))))
        };
        let estilo_ordem: EstiloTexto = if visivel { text::base } else { text::secondary };
        lista = lista.push(
            container(
                row![
                    text(format!("{}.", i + 1)).size(12).width(24).style(estilo_ordem),
                    marcador,
                    if c.obrigatoria() { text(" (fixa)").size(11).style(text::secondary) } else { text("") },
                    space::horizontal(),
                    seta("⤒", (i > 0).then_some(0)),
                    seta("↑", (i > 0).then(|| i - 1)),
                    seta("↓", (i + 1 < total).then_some(i + 1)),
                    seta("⤓", (i + 1 < total).then_some(total - 1)),
                ]
                .spacing(4)
                .align_y(Center),
            )
            .padding([3, 6])
            .style(if visivel { container::rounded_box } else { container::transparent }),
        );
    }
    let n_vis = cfg.visiveis().count();
    container(
        column![
            text("Configurar colunas").size(15).font(NEGRITO),
            text(format!(
                "{n_vis} de {total} visíveis. Marque as que quer ver e use as setas para mudar a ordem \
                 (⤒ topo · ↑ subir · ↓ descer · ⤓ fim). Salvo automaticamente."
            ))
            .size(12)
            .style(text::secondary),
            row![
                button(text("Mostrar todas").size(12)).style(button::secondary).on_press(acao(AcaoColuna::MostrarTodas)),
                button(text("Restaurar padrão").size(12)).style(button::secondary).on_press(acao(AcaoColuna::Padrao)),
            ]
            .spacing(8),
            rule::horizontal(1),
            scrollable(lista.padding(iced::Padding { right: 14.0, ..iced::Padding::ZERO })).height(Fill),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(400)
    .height(Fill)
    .style(container::bordered_box)
    .into()
}

/// Barra de navegação entre páginas, repetida no começo e no fim da lista.
fn view_paginacao<'a>(p: Paginacao) -> Element<'a, Message> {
    let botao = |rotulo: &'static str, destino: Option<usize>| {
        button(text(rotulo).size(13)).padding([3, 10]).style(button::secondary).on_press_maybe(destino.map(Message::PaginaProd))
    };
    let tem_anterior = p.pagina > 0;
    let tem_proxima = p.pagina < p.ultima;
    row![
        botao("« Início", tem_anterior.then_some(0)),
        botao("‹ Anterior", tem_anterior.then(|| p.pagina - 1)),
        text(format!("Página {} de {}  ·  itens {}–{} de {}", p.pagina + 1, p.ultima + 1, p.inicio + 1, p.fim, p.total)).size(13),
        botao("Próxima ›", tem_proxima.then(|| p.pagina + 1)),
        botao("Fim »", tem_proxima.then_some(p.ultima)),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

/// Monta uma coluna da tabela de Produtos.
fn coluna_produto<'a>(c: ColunaProd, a: &'a Analise, sel: Option<i64>) -> table::Column<'a, 'a, &'a Produto, Message> {
    type P<'x> = &'x Produto;
    let titulo = c.rotulo();
    match c {
        ColunaProd::Codigo => col_num(titulo, 1, |p: P<'a>| cel(p.codigo.to_string())),
        ColunaProd::CodigoProprio => table::column(cab(titulo), |p: P<'a>| cel(p.codigo_proprio.as_str())).width(FillPortion(2)),
        ColunaProd::Descricao => table::column(cab(titulo), move |p: P<'a>| {
            let estilo: EstiloBotao = if sel == Some(p.codigo) { button::primary } else { button::text };
            button(text(p.descricao.as_str()).size(13)).padding([1, 4]).style(estilo).on_press(Message::SelProduto(p.codigo))
        })
        .width(FillPortion(6)),
        ColunaProd::Categoria => {
            table::column(cab(titulo), move |p: P<'a>| cel(a.categoria_de(p.cod_categoria))).width(FillPortion(2))
        }
        ColunaProd::Composicao => table::column(cab(titulo), move |p: P<'a>| match &a.composicoes {
            None => cel("—").style(text::secondary),
            Some(m) if m.contains_key(&p.codigo) => cel("Sim").style(text::success),
            Some(_) => cel("Não").style(text::secondary),
        })
        .width(FillPortion(2)),
        ColunaProd::Unidade => table::column(cab(titulo), |p: P<'a>| cel(p.unidade.as_str())).width(FillPortion(1)),
        ColunaProd::Estoque => col_num(titulo, 2, |p: P<'a>| {
            cel(fmt::qtd(p.estoque)).style(if p.estoque < 0.0 { text::danger } else { text::base })
        }),
        ColunaProd::EstoqueMinimo => col_num(titulo, 2, |p: P<'a>| cel(fmt::qtd(p.estoque_minimo))),
        ColunaProd::PesoLiquido => col_num(titulo, 2, |p: P<'a>| cel(fmt::num(p.peso_liquido, 4))),
        ColunaProd::PrecoCusto => col_num(titulo, 2, |p: P<'a>| cel(fmt::moeda(p.preco_custo))),
        ColunaProd::PrecoVenda => col_num(titulo, 2, |p: P<'a>| cel(fmt::moeda(p.preco_venda))),
        ColunaProd::Tags => table::column(cab(titulo), |p: P<'a>| cel(p.tags.join(", "))).width(FillPortion(3)),
    }
}

/// Nome do item na árvore, recuado conforme o nível. Compostos em negrito.
fn celula_item<'a>(n: No<'a>) -> Element<'a, Message> {
    let nome = cel(n.descricao);
    let nome = if n.tipo == TipoNo::Composto { nome.font(NEGRITO) } else { nome };
    container(nome).padding(iced::Padding { left: 18.0 * n.nivel as f32, ..iced::Padding::ZERO }).into()
}

/// Valor de custo de uma linha da árvore, deixando claro quando ele não é confiável.
fn celula_custo<'a>(n: No<'a>, valor: f64) -> iced::widget::Text<'a> {
    match n.tipo {
        TipoNo::Ciclo => cel("—").style(text::danger),
        TipoNo::Composto if n.incompleto => cel(format!("{} *", fmt::custo(valor))).style(text::warning),
        TipoNo::Composto => cel(fmt::custo(valor)).font(NEGRITO),
        _ if n.incompleto => cel("sem custo").style(text::danger),
        _ => cel(fmt::custo(valor)),
    }
}

fn aviso_ciclos<'a>(e: &Estrutura<'a>) -> Option<Element<'a, Message>> {
    (!e.ciclos.is_empty()).then(|| {
        text(format!(
            "Cadastro circular: {} aparece dentro da própria composição. Esse ramo não foi aberto de novo e não entra no custo. Corrija no eGestor.",
            e.ciclos.join(", ")
        ))
        .size(12)
        .style(text::danger)
        .into()
    })
}

/// Quantidade do produto raiz para os textos, ex.: "10 UN".
fn qtd_raiz(e: &Estrutura<'_>) -> String {
    let raiz = &e.nos[0];
    format!("{} {}", fmt::qtd(raiz.qtd_total), if raiz.unidade.is_empty() { "unidade(s)" } else { raiz.unidade })
}

/// Toda a estrutura do produto, descendo até os insumos finais.
fn view_bom<'a>(e: Estrutura<'a>) -> Element<'a, Message> {
    let aviso = aviso_ciclos(&e);
    let mut col = column![
        text(format!(
            "{} níveis  ·  {} itens na estrutura  ·  quantidade total para {}, já com perdas",
            e.niveis,
            e.nos.len() - 1,
            qtd_raiz(&e)
        ))
        .size(12)
        .style(text::secondary),
    ]
    .spacing(6);
    col = col.extend(aviso);

    let colunas = vec![
        table::column(cab("Nível"), |n: No<'a>| cel(n.nivel.to_string())).width(FillPortion(1)),
        table::column(cab("Item"), |n: No<'a>| celula_item(n)).width(FillPortion(8)),
        table::column(cab("Cód. próprio"), |n: No<'a>| cel(n.codigo_proprio)).width(FillPortion(2)),
        table::column(cab("Categoria"), |n: No<'a>| cel(n.categoria)).width(FillPortion(2)),
        table::column(cab("Tipo"), |n: No<'a>| {
            cel(n.tipo.rotulo()).style(if n.tipo == TipoNo::Ciclo { text::danger } else { text::base })
        })
        .width(FillPortion(3)),
        table::column(cab("Un."), |n: No<'a>| cel(n.unidade)).width(FillPortion(1)),
        col_num("Qtd. p/ item acima", 2, |n: No<'a>| cel(fmt::num(n.qtd_por_pai, 6))),
        col_num("Perda", 1, |n: No<'a>| cel(format!("{}%", fmt::num(n.perc_perda, 2)))),
        col_num("Qtd. total", 2, |n: No<'a>| cel(fmt::num(n.qtd_total, 6))),
    ];
    col.push(rolagem(table::table(colunas, e.nos).padding_x(8).padding_y(3).width(Fill))).into()
}

/// Quanto de cada material o produto usa no geral, somando todos os subprodutos.
fn view_soma_materiais<'a>(e: Estrutura<'a>) -> Element<'a, Message> {
    let materiais = e.materiais_somados();
    let sem_custo = materiais.iter().filter(|m| m.custo_unit <= 0.0).count();
    let aviso_custo = if sem_custo > 0 { format!(" ({sem_custo} sem custo cadastrado)") } else { String::new() };
    let aviso = aviso_ciclos(&e);
    let mut col = column![
        text(format!(
            "{} materiais  ·  custo dos materiais: {}{aviso_custo}  ·  quantidade total para {}, já com perdas  ·  mão de obra não entra",
            materiais.len(),
            fmt::custo(e.custo_materiais),
            qtd_raiz(&e)
        ))
        .size(12)
        .style(text::secondary),
    ]
    .spacing(6);
    col = col.extend(aviso);

    type M<'x> = MaterialSomado<'x>;
    let celula_valor = |m: M<'a>, valor: f64| {
        if m.custo_unit > 0.0 { cel(fmt::custo(valor)) } else { cel("sem custo").style(text::danger) }
    };
    let base = e.custo_materiais.max(1e-12);
    let colunas = vec![
        table::column(cab("Material"), |m: M<'a>| cel(m.descricao)).width(FillPortion(8)),
        table::column(cab("Cód. próprio"), |m: M<'a>| cel(m.codigo_proprio)).width(FillPortion(2)),
        table::column(cab("Categoria"), |m: M<'a>| cel(m.categoria)).width(FillPortion(2)),
        table::column(cab("Un."), |m: M<'a>| cel(m.unidade)).width(FillPortion(1)),
        col_num("Nº de usos", 1, |m: M<'a>| cel(m.usos.to_string())),
        col_num("Qtd. total", 2, |m: M<'a>| cel(fmt::num(m.qtd_total, 6)).font(NEGRITO)),
        col_num("Custo un.", 2, move |m: M<'a>| celula_valor(m, m.custo_unit)),
        col_num("Custo total", 2, move |m: M<'a>| celula_valor(m, m.custo_total)),
        col_num("% do custo", 1, move |m: M<'a>| cel(format!("{}%", fmt::num(100.0 * m.custo_total / base, 1)))),
    ];
    col.push(rolagem(table::table(colunas, materiais).padding_x(8).padding_y(3).width(Fill))).into()
}

/// Todos os custos do produto, somados de baixo para cima a partir dos insumos finais.
fn view_custeio<'a>(e: Estrutura<'a>) -> Element<'a, Message> {
    let total = e.custo_total();
    let por_unidade = e.custo_produto();
    let comparacao = match e.comparacao_custo() {
        Some(c) => {
            let sinal = if c.diferenca > 0.0 { "+" } else { "" };
            format!(
                "Custo por unidade: {}  ·  cadastrado no eGestor: {}  ·  diferença: {sinal}{} ({sinal}{}%)",
                fmt::custo(por_unidade),
                fmt::custo(c.cadastrado),
                fmt::custo(c.diferenca),
                fmt::num(c.percentual, 1)
            )
        }
        None => format!("Custo por unidade: {}  ·  sem custo cadastrado no eGestor para comparar.", fmt::custo(por_unidade)),
    };
    let aviso = aviso_ciclos(&e);
    let mut col = column![
        text(format!(
            "Custo calculado para {}: {}{}  =  materiais {}  +  mão de obra {}",
            qtd_raiz(&e),
            fmt::custo(total),
            if e.nos[0].incompleto { " *" } else { "" },
            fmt::custo(e.custo_materiais),
            fmt::custo(e.custo_mao_de_obra)
        ))
        .size(14)
        .font(NEGRITO),
        text(comparacao).size(12).style(text::secondary),
    ]
    .spacing(4);
    col = col.extend(aviso);

    if !e.sem_custo.is_empty() {
        const MAX_NOMES: usize = 6;
        let n = e.sem_custo.len();
        let mut nomes = e.sem_custo.iter().take(MAX_NOMES).map(|(d, _)| *d).collect::<Vec<_>>().join("; ");
        if n > MAX_NOMES {
            nomes.push_str(&format!("; e mais {}", n - MAX_NOMES));
        }
        let clientes = if e.sem_custo.iter().any(|(_, c)| *c == "Clientes") {
            " Os da categoria Clientes podem ser material fornecido pelo cliente."
        } else {
            ""
        };
        col = col.push(
            text(format!("* Incompleto: o custo real pode ser maior, pois {n} insumo(s) estão sem custo no cadastro: {nomes}.{clientes}"))
                .size(12)
                .style(text::warning),
        );
    }

    let base = total.max(1e-12);
    let colunas = vec![
        table::column(cab("Nível"), |n: No<'a>| cel(n.nivel.to_string())).width(FillPortion(1)),
        table::column(cab("Item"), |n: No<'a>| celula_item(n)).width(FillPortion(8)),
        table::column(cab("Un."), |n: No<'a>| cel(n.unidade)).width(FillPortion(1)),
        col_num("Qtd. total", 2, |n: No<'a>| cel(fmt::num(n.qtd_total, 6))),
        col_num("Custo un.", 2, |n: No<'a>| celula_custo(n, n.custo_unit)),
        col_num("Custo total", 2, |n: No<'a>| celula_custo(n, n.custo_total)),
        col_num("% do total", 1, move |n: No<'a>| cel(format!("{}%", fmt::num(100.0 * n.custo_total / base, 1)))),
        col_num("Custo cadastrado un.", 2, |n: No<'a>| {
            if n.tipo == TipoNo::Composto {
                cel(fmt::custo(n.custo_cadastrado))
            } else {
                cel("—").style(text::secondary)
            }
        }),
    ];
    col.push(rolagem(table::table(colunas, e.nos).padding_x(8).padding_y(3).width(Fill))).into()
}

/// Banner inferior da aba Produtos: composição cadastrada do item selecionado.
fn view_composicao<'a>(a: &'a Analise, p: &'a Produto, visao: VisaoComposicao, qtd_pecas: &'a str) -> Element<'a, Message> {
    let necessarios: HashSet<i64> = a.res.necessidades.iter().map(|n| n.cod_insumo).collect();
    let itens = a.composicoes.as_ref().and_then(|m| m.get(&p.codigo));
    let qtd = fmt::ler_qtd(qtd_pecas);
    let qtd_calculo = qtd.unwrap_or(1.0);
    let estrutura = estrutura::explodir(a, p.codigo, qtd_calculo);
    // Clicar no botão da visão aberta volta para a composição direta.
    let botao_visao = |rotulo: &'static str, alvo: VisaoComposicao| {
        let ativo = visao == alvo;
        let estilo: EstiloBotao = if ativo { button::primary } else { button::secondary };
        let destino = if ativo { VisaoComposicao::Direta } else { alvo };
        button(text(rotulo).size(12))
            .style(estilo)
            .on_press_maybe(itens.is_some().then_some(Message::VisaoComposicao(destino)))
    };

    let mut resumo = format!("Cód. {}  ·  {}", p.codigo, a.categoria_de(p.cod_categoria));
    if let Some(itens) = itens {
        resumo.push_str(&format!(
            "  ·  {} insumos  ·  consumo por {}  ·  quantidade total para {} {}",
            itens.len(),
            p.unidade,
            fmt::qtd(qtd_calculo),
            p.unidade
        ));
    }
    let mut titulo = row![
        column![
            text(p.descricao.as_str()).size(16).font(NEGRITO),
            text(resumo).size(12).style(text::secondary),
        ],
        space::horizontal(),
    ]
    .spacing(8)
    .align_y(Center);
    if itens.is_some() {
        if qtd.is_none() {
            titulo = titulo.push(text("Quantidade inválida: calculando para 1").size(12).style(text::danger));
        }
        titulo = titulo.push(text("Qtd. de peças").size(12)).push(
            text_input(QTD_PECAS_PADRAO, qtd_pecas).on_input(Message::QtdPecas).size(12).padding([4, 8]).width(90),
        );
    }
    titulo = titulo
        .push(botao_visao("BOM multinível", VisaoComposicao::Bom))
        .push(botao_visao("Soma de materiais", VisaoComposicao::SomaMateriais))
        .push(botao_visao("Custeio multinível", VisaoComposicao::Custeio));
    if necessarios.contains(&p.codigo) {
        titulo = titulo.push(
            button(text("Ver necessidade deste item").size(12))
                .on_press(Message::VerMaterial(p.codigo))
                .style(button::secondary),
        );
    }
    let titulo = titulo.push(button(text("Fechar").size(12)).on_press(Message::FecharProduto).style(button::secondary));

    let corpo: Element<'a, Message> = match (visao, estrutura, itens) {
        (VisaoComposicao::Bom, Some(e), _) => view_bom(e),
        (VisaoComposicao::SomaMateriais, Some(e), _) => view_soma_materiais(e),
        (VisaoComposicao::Custeio, Some(e), _) => view_custeio(e),
        (_, _, Some(itens)) => {
            type I<'x> = &'x ItemComposicao;
            let colunas = vec![
                table::column(cab("Insumo"), move |i: I<'a>| {
                    button(text(i.descricao.as_str()).size(13))
                        .padding([1, 4])
                        .style(button::text)
                        .on_press_maybe(necessarios.contains(&i.cod_insumo).then_some(Message::VerMaterial(i.cod_insumo)))
                })
                .width(FillPortion(6)),
                table::column(cab("Cód. próprio"), |i: I<'a>| cel(i.codigo_proprio.as_str())).width(FillPortion(2)),
                table::column(cab("Categoria"), move |i: I<'a>| {
                    cel(a.produtos.get(&i.cod_insumo).map_or("—", |c| a.categoria_de(c.cod_categoria)))
                })
                .width(FillPortion(2)),
                table::column(cab("Un."), |i: I<'a>| cel(i.unidade.as_str())).width(FillPortion(1)),
                col_num("Consumo/pç", 2, |i: I<'a>| cel(fmt::num(i.quant, 6))),
                col_num("Perda", 1, |i: I<'a>| cel(format!("{}%", fmt::num(i.perc_perda, 2)))),
                col_num("Consumo c/ perda", 2, |i: I<'a>| cel(fmt::num(i.qtd_com_perda(), 6))),
                col_num("Qtd. total", 2, move |i: I<'a>| cel(fmt::num(i.qtd_para(qtd_calculo), 6)).font(NEGRITO)),
            ];
            rolagem(table::table(colunas, itens).padding_x(8).padding_y(3).width(Fill)).into()
        }
        _ if a.composicoes.is_none() => text(AVISO_SEM_COMPOSICOES).size(13).style(text::warning).into(),
        _ => text("Este item não possui composição cadastrada.").size(13).style(text::secondary).into(),
    };

    container(column![titulo, corpo].spacing(8)).padding(12).style(container::bordered_box).height(Fill).into()
}

/// Monta uma coluna da tabela de Necessidades.
fn coluna_necessidade<'a>(c: ColunaNec, sel: Option<i64>) -> table::Column<'a, 'a, &'a Necessidade, Message> {
    type N<'x> = &'x Necessidade;
    let titulo = c.rotulo();
    match c {
        ColunaNec::Material => table::column(cab(titulo), move |n: N<'a>| {
            let estilo: EstiloBotao = if sel == Some(n.cod_insumo) { button::primary } else { button::text };
            button(text(n.descricao.as_str()).size(13)).padding([1, 4]).style(estilo).on_press(Message::SelMaterial(n.cod_insumo))
        })
        .width(FillPortion(6)),
        ColunaNec::Codigo => col_num(titulo, 1, |n: N<'a>| cel(n.cod_insumo.to_string())),
        ColunaNec::CodigoProprio => table::column(cab(titulo), |n: N<'a>| cel(n.codigo_proprio.as_str())).width(FillPortion(2)),
        ColunaNec::Categoria => table::column(cab(titulo), |n: N<'a>| cel(n.categoria.as_str())).width(FillPortion(2)),
        ColunaNec::Unidade => table::column(cab(titulo), |n: N<'a>| cel(n.unidade.as_str())).width(FillPortion(1)),
        ColunaNec::Necessidade => col_num(titulo, 2, |n: N<'a>| cel(fmt::qtd(n.necessidade))),
        ColunaNec::Estoque => col_num(titulo, 2, |n: N<'a>| {
            cel(fmt::qtd(n.estoque)).style(if n.estoque < 0.0 { text::danger } else { text::base })
        }),
        ColunaNec::EstoqueMinimo => col_num(titulo, 2, |n: N<'a>| cel(fmt::qtd(n.estoque_minimo))),
        ColunaNec::Saldo => col_num(titulo, 2, |n: N<'a>| {
            let e: EstiloTexto = if n.saldo() < 0.0 { text::danger } else { text::success };
            cel(fmt::qtd(n.saldo())).style(e)
        }),
        ColunaNec::Falta => col_num(titulo, 2, |n: N<'a>| {
            let e: EstiloTexto = if n.falta() > 0.0 { text::danger } else { text::secondary };
            cel(if n.falta() > 0.0 { fmt::qtd(n.falta()) } else { "—".into() }).style(e).font(NEGRITO)
        }),
        ColunaNec::ComprarComMinimo => col_num(titulo, 2, |n: N<'a>| cel(fmt::qtd(n.comprar_com_minimo()))),
        ColunaNec::CustoUnit => col_num(titulo, 2, |n: N<'a>| cel(fmt::moeda(n.custo_unit))),
        ColunaNec::CustoNecessidade => col_num(titulo, 2, |n: N<'a>| cel(fmt::moeda(n.custo_necessidade()))),
        ColunaNec::CustoFalta => col_num(titulo, 2, |n: N<'a>| cel(fmt::moeda(n.custo_falta()))),
        ColunaNec::ControlaEstoque => {
            table::column(cab(titulo), |n: N<'a>| cel(if n.controla_estoque { "Sim" } else { "Não" })).width(FillPortion(1))
        }
        ColunaNec::NumOps => col_num(titulo, 1, |n: N<'a>| cel(n.ops.len().to_string())),
        ColunaNec::ListaOps => table::column(cab(titulo), |n: N<'a>| {
            cel(n.ops.iter().map(|o| o.to_string()).collect::<Vec<_>>().join(", "))
        })
        .width(FillPortion(3)),
        ColunaNec::NumProdutos => col_num(titulo, 1, |n: N<'a>| cel(n.produtos.len().to_string())),
        ColunaNec::Tags => table::column(cab(titulo), |n: N<'a>| -> Element<'a, Message> {
            if n.tags.is_empty() {
                cel("—").style(text::secondary).into()
            } else {
                let mut r = row![].spacing(4);
                for t in &n.tags {
                    r = r.push(container(text(t.as_str()).size(11)).padding([1, 6]).style(container::rounded_box));
                }
                r.wrap().vertical_spacing(4).into()
            }
        })
        .width(FillPortion(3)),
    }
}

fn cab<'a>(s: &'a str) -> iced::widget::Text<'a> {
    text(s).size(12).font(NEGRITO)
}

fn cel<'a>(s: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(s).size(13)
}

/// Coluna numérica (alinhada à direita).
fn col_num<'a, 'b, T: 'a, E: Into<Element<'a, Message>>>(
    titulo: &'a str,
    peso: u16,
    f: impl Fn(T) -> E + 'b,
) -> table::Column<'a, 'b, T, Message> {
    table::column(cab(titulo), f).width(FillPortion(peso)).align_x(Right)
}

fn card<'a>(rotulo: &'a str, valor: String, detalhe: String, estilo: EstiloTexto) -> Element<'a, Message> {
    container(
        column![
            text(rotulo).size(13).style(text::secondary),
            text(valor).size(28).font(NEGRITO).style(estilo),
            text(detalhe).size(11).style(text::secondary),
        ]
        .spacing(4),
    )
    .padding(16)
    .width(Fill)
    .style(container::bordered_box)
    .into()
}

fn painel<'a>(titulo: &'a str, conteudo: Element<'a, Message>) -> Element<'a, Message> {
    container(column![text(titulo).size(15).font(NEGRITO), conteudo].spacing(10))
        .padding(14)
        .width(Fill)
        .height(Fill)
        .style(container::bordered_box)
        .into()
}

/// Botão em forma de cartão (lista de OPs): fundo claro, borda fina.
fn cartao(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let fundo = match status {
        button::Status::Hovered | button::Status::Pressed => p.background.weak.color,
        _ => p.background.weakest.color,
    };
    button::Style {
        background: Some(fundo.into()),
        text_color: p.background.base.text,
        border: iced::Border { radius: 6.0.into(), width: 1.0, color: p.background.strong.color },
        ..button::Style::default()
    }
}

/// Moldura destacada da barra de grupo quando há OPs selecionadas.
fn grupo_ativo(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        border: iced::Border { radius: 6.0.into(), width: 2.0, color: p.primary.strong.color },
        ..container::bordered_box(theme)
    }
}

fn cartao_selecionado(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    button::Style {
        background: Some(p.primary.weak.color.into()),
        border: iced::Border { radius: 6.0.into(), width: 2.0, color: p.primary.strong.color },
        ..cartao(theme, status)
    }
}

fn estilo_gravidade(g: &str) -> EstiloTexto {
    match g {
        "ALTA" => text::danger,
        "MÉDIA" => text::warning,
        _ => text::secondary,
    }
}

fn linha_alerta<'a>(al: &'a Alerta) -> Element<'a, Message> {
    let mut c: Column<'a, Message> = column![
        row![
            text(al.gravidade).size(12).font(NEGRITO).style(estilo_gravidade(al.gravidade)),
            text(al.tipo).size(12).font(NEGRITO),
        ]
        .spacing(8),
    ]
    .spacing(2);
    let mut r = row![].spacing(8).align_y(Center);
    if let Some(op) = al.op {
        r = r.push(button(text(format!("OP {op}")).size(12)).padding([0, 4]).style(button::text).on_press(Message::VerOp(op)));
    }
    r = r.push(text(al.descricao.as_str()).size(12));
    c = c.push(r).push(text(al.mensagem.as_str()).size(12).style(text::secondary));
    c.into()
}

/// Área rolável com folga à direita, para a barra de rolagem não cobrir a última coluna.
fn rolagem<'a>(conteudo: impl Into<Element<'a, Message>>) -> iced::widget::Scrollable<'a, Message> {
    scrollable(container(conteudo).padding(iced::Padding { right: 16.0, ..iced::Padding::ZERO }))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn paginas_de_300_itens() {
        let p = Paginacao::de(1787, 0);
        assert_eq!((p.pagina, p.ultima, p.inicio, p.fim), (0, 5, 0, 300));
        let p = Paginacao::de(1787, 5);
        assert_eq!((p.inicio, p.fim), (1500, 1787));
    }

    #[test]
    fn limites_da_paginacao() {
        assert_eq!(Paginacao::de(300, 0).ultima, 0, "exatamente uma página cheia");
        assert_eq!(Paginacao::de(301, 0).ultima, 1);

        // A lista encolheu (busca/atualização): cai na última página existente.
        let p = Paginacao::de(10, 4);
        assert_eq!((p.pagina, p.inicio, p.fim), (0, 0, 10));

        let p = Paginacao::de(0, 0);
        assert_eq!((p.inicio, p.fim), (0, 0), "lista vazia não pode gerar recorte inválido");
    }
}
