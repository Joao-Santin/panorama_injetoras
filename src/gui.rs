//! Interface gráfica (iced) do panorama de necessidades de materiais.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
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
        Column, button, center, checkbox, column, container, pick_list, progress_bar, row, rule, scrollable, space,
        table, text, text_input, toggler,
    },
};

use crate::{
    calculo::{self, Alerta, Necessidade, Resultado},
    coleta::{self, Analise, DadosBrutos, Progresso},
    colunas::{ColunaNec, ConfigColunas},
    fmt,
    modelo::{ItemOp, Op, Produto},
};

const NEGRITO: Font = Font { weight: font::Weight::Bold, ..Font::DEFAULT };
const MAX_LINHAS_PRODUTOS: usize = 300;

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
    ColunaVisivel(usize, bool),
    ColunaMover(usize, usize),
    ColunasMostrarTodas,
    ColunasPadrao,
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
    config_nec: ConfigColunas,
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
            so_usados: true,
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
            Message::ColunaVisivel(..)
            | Message::ColunaMover(..)
            | Message::ColunasMostrarTodas
            | Message::ColunasPadrao => {
                match msg {
                    Message::ColunaVisivel(i, v) => self.config_nec.definir_visivel(i, v),
                    Message::ColunaMover(i, d) => self.config_nec.mover(i, d),
                    Message::ColunasMostrarTodas => self.config_nec.mostrar_todas(),
                    _ => self.config_nec = ConfigColunas::default(),
                }
                if let Err(e) = self.config_nec.salvar() {
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
            Message::BuscaProd(s) => self.busca_prod = s,
            Message::SoUsados(b) => self.so_usados = b,
        }
        Task::none()
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
            (Aba::Produtos, format!("Produtos ({})", a.produtos.len())),
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
        faltas.sort_by(|x, y| (y.falta() * y.custo_unit).total_cmp(&(x.falta() * x.custo_unit)));
        let max = faltas.iter().map(|n| n.falta() * n.custo_unit).fold(0.0_f64, f64::max).max(1e-9);
        let mut lista = column![].spacing(10);
        for n in faltas.iter().take(15) {
            let custo = n.falta() * n.custo_unit;
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

        let mut por_peso: Vec<(&Op, f64)> = a.ops.iter().map(|o| (o, peso_op(o, a))).collect();
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
                (y.falta() * y.custo_unit)
                    .total_cmp(&(x.falta() * x.custo_unit))
                    .then(y.falta().total_cmp(&x.falta()))
            }),
            OrdemNec::Falta => linhas.sort_by(|x, y| y.falta().total_cmp(&x.falta())),
            OrdemNec::Necessidade => linhas.sort_by(|x, y| y.necessidade.total_cmp(&x.necessidade)),
            OrdemNec::Saldo => linhas.sort_by(|x, y| x.saldo().total_cmp(&y.saldo())),
            OrdemNec::Nome => linhas.sort_by(|x, y| x.descricao.cmp(&y.descricao)),
        }
        let custo_total: f64 = linhas.iter().map(|n| n.falta() * n.custo_unit).sum();

        let filtros = row![
            text_input("Buscar material, código, categoria ou tag...", &self.busca_nec)
                .on_input(Message::BuscaNec)
                .width(340),
            toggler(self.so_faltas).label("Somente com falta").on_toggle(Message::SoFaltas),
            pick_list(OrdemNec::TODAS, Some(self.ordem_nec), Message::OrdemNec),
            space::horizontal(),
            text(format!("{} materiais  ·  custo das faltas listadas: {}", linhas.len(), fmt::moeda(custo_total))).size(13),
            {
                let estilo: EstiloBotao = if self.modo_config_nec { button::primary } else { button::secondary };
                button(text(if self.modo_config_nec { "Concluir configuração" } else { "Configurar colunas" }).size(13))
                    .style(estilo)
                    .on_press(Message::ModoConfigNec(!self.modo_config_nec))
            },
        ]
        .spacing(14)
        .align_y(Center);

        let sel = self.material_sel;
        let colunas: Vec<_> = self.config_nec.visiveis().map(|c| coluna_necessidade(c, sel)).collect();
        let tabela = rolagem(table::table(colunas, linhas).padding_x(8).padding_y(4).width(Fill));
        // Em modo de configuração, o painel fica à direita da tabela (prévia ao vivo).
        let tabela: Element<'a, Message> = if self.modo_config_nec {
            row![container(tabela).width(Fill), self.view_config_colunas()].spacing(12).into()
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

    /// Painel do modo de configuração: marcar/desmarcar colunas e mudar a ordem.
    fn view_config_colunas(&self) -> Element<'_, Message> {
        let total = self.config_nec.colunas.len();
        let mut lista = column![].spacing(4);
        for (i, (c, visivel)) in self.config_nec.colunas.iter().enumerate() {
            let (c, visivel) = (*c, *visivel);
            let mut marcador = checkbox(visivel).label(c.rotulo()).text_size(13);
            if !c.obrigatoria() {
                marcador = marcador.on_toggle(move |v| Message::ColunaVisivel(i, v));
            }
            let seta = |rotulo: &'static str, destino: Option<usize>| {
                button(text(rotulo).size(13)).padding([2, 8]).style(button::secondary).on_press_maybe(destino.map(|d| Message::ColunaMover(i, d)))
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
        let n_vis = self.config_nec.visiveis().count();
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
                    button(text("Mostrar todas").size(12)).style(button::secondary).on_press(Message::ColunasMostrarTodas),
                    button(text("Restaurar padrão").size(12)).style(button::secondary).on_press(Message::ColunasPadrao),
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
            let pecas: f64 = ops.iter().flat_map(|o| &o.itens).map(|i| i.quant).sum();
            let peso: f64 = ops.iter().map(|o| peso_op(o, a)).sum();
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
            col_num("Custo total", 2, |n: &'a Necessidade| cel(fmt::moeda(n.necessidade * n.custo_unit))),
            col_num("OPs", 1, |n: &'a Necessidade| cel(n.ops.len().to_string())),
        ];

        // Horas por OP
        let mut por_op: BTreeMap<i64, (f64, String)> = BTreeMap::new();
        for d in a.res.detalhe.iter().filter(|d| crate::calculo::eh_mao_de_obra(&d.categoria, &d.insumo)) {
            let e = por_op.entry(d.op).or_insert((0.0, d.op_tags.clone()));
            e.0 += d.necessidade;
        }
        let mut horas: Vec<(i64, f64, String)> = por_op.into_iter().map(|(k, (h, t))| (k, h, t)).collect();
        horas.sort_by(|x, y| y.1.total_cmp(&x.1));
        let max = horas.first().map(|x| x.1).unwrap_or(0.0).max(1e-9);
        let suspeitas: HashSet<i64> =
            a.res.alertas.iter().filter(|x| x.tipo == "Consumo de MO suspeito").filter_map(|x| x.op).collect();

        let mut lista = column![].spacing(6);
        for (op, h, tags) in horas {
            let estilo: EstiloTexto = if suspeitas.contains(&op) { text::danger } else { text::base };
            lista = lista.push(
                row![
                    button(text(format!("OP {op}")).size(13)).padding(0).style(button::text).on_press(Message::VerOp(op)).width(80),
                    text(tags).size(12).style(text::secondary).width(260),
                    progress_bar(0.0..=max as f32, h as f32).girth(10),
                    text(format!("{} h{}", fmt::num(h, 1), if suspeitas.contains(&op) { "  (verificar!)" } else { "" }))
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
        let faltando: HashSet<i64> = a
            .res
            .necessidades
            .iter()
            .filter(|n| !n.mao_de_obra && n.falta() > 0.0)
            .map(|n| n.cod_insumo)
            .collect();
        let op_tem_falta =
            |o: &Op| o.itens.iter().flat_map(|i| &i.insumos).any(|x| faltando.contains(&x.cod_insumo));

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
            let falta = op_tem_falta(o);
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
        let pecas: f64 = o.itens.iter().map(|i| i.quant).sum();
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
                fmt::qtd(pecas),
                fmt::num(peso_op(o, a), 1),
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
        let peso = a.produtos.get(&item.cod_produto).map(|p| p.peso_liquido).unwrap_or(0.0);
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
                fmt::num(peso, 4),
                fmt::num(peso * item.quant, 1),
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
        let busca = self.busca_prod.to_lowercase();
        let usados: HashSet<i64> = a
            .ops
            .iter()
            .flat_map(|o| &o.itens)
            .flat_map(|i| std::iter::once(i.cod_produto).chain(i.insumos.iter().map(|x| x.cod_insumo)))
            .collect();
        let mut todos: Vec<&'a Produto> = a
            .produtos
            .values()
            .filter(|p| !self.so_usados || usados.contains(&p.codigo))
            .filter(|p| {
                contem(&p.descricao, &busca)
                    || contem(&p.codigo_proprio, &busca)
                    || contem(&p.tags.join(" "), &busca)
                    || p.codigo.to_string() == busca
            })
            .collect();
        todos.sort_by(|x, y| x.descricao.cmp(&y.descricao));
        let total = todos.len();
        todos.truncate(MAX_LINHAS_PRODUTOS);

        let filtros = row![
            text_input("Buscar produto, código ou tag...", &self.busca_prod).on_input(Message::BuscaProd).width(340),
            toggler(self.so_usados).label("Somente itens usados nas OPs em aberto").on_toggle(Message::SoUsados),
            space::horizontal(),
            text(if total > MAX_LINHAS_PRODUTOS {
                format!("mostrando {MAX_LINHAS_PRODUTOS} de {total} — refine a busca")
            } else {
                format!("{total} produtos")
            })
            .size(13),
        ]
        .spacing(14)
        .align_y(Center);

        let colunas = vec![
            col_num("Cód.", 1, |p: &'a Produto| cel(p.codigo.to_string())),
            table::column(cab("Cód. próprio"), |p: &'a Produto| cel(p.codigo_proprio.as_str())).width(FillPortion(2)),
            table::column(cab("Descrição"), |p: &'a Produto| {
                button(text(p.descricao.as_str()).size(13))
                    .padding([1, 4])
                    .style(button::text)
                    .on_press_maybe(usados.contains(&p.codigo).then_some(Message::VerMaterial(p.codigo)))
            })
            .width(FillPortion(6)),
            table::column(cab("Un."), |p: &'a Produto| cel(p.unidade.as_str())).width(FillPortion(1)),
            col_num("Estoque", 2, |p: &'a Produto| {
                cel(fmt::qtd(p.estoque)).style(if p.estoque < 0.0 { text::danger } else { text::base })
            }),
            col_num("Est. mín.", 2, |p: &'a Produto| cel(fmt::qtd(p.estoque_minimo))),
            col_num("Peso líq. (kg)", 2, |p: &'a Produto| cel(fmt::num(p.peso_liquido, 4))),
            col_num("Preço custo", 2, |p: &'a Produto| cel(fmt::moeda(p.preco_custo))),
            col_num("Preço venda", 2, |p: &'a Produto| cel(fmt::moeda(p.preco_venda))),
            table::column(cab("Tags"), |p: &'a Produto| cel(p.tags.join(", "))).width(FillPortion(3)),
        ];

        column![
            filtros,
            rolagem(table::table(colunas, todos).padding_x(8).padding_y(4).width(Fill)).height(Fill),
            text("Clique na descrição de um insumo usado nas OPs para ver a necessidade dele.").size(12).style(text::secondary),
        ]
        .spacing(10)
        .into()
    }
}

// ============================================================ helpers ===

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
        ColunaNec::CustoNecessidade => col_num(titulo, 2, |n: N<'a>| cel(fmt::moeda(n.necessidade * n.custo_unit))),
        ColunaNec::CustoFalta => col_num(titulo, 2, |n: N<'a>| cel(fmt::moeda(n.falta() * n.custo_unit))),
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

fn peso_op(o: &Op, a: &Analise) -> f64 {
    o.itens
        .iter()
        .map(|i| i.quant * a.produtos.get(&i.cod_produto).map(|p| p.peso_liquido).unwrap_or(0.0))
        .sum()
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
