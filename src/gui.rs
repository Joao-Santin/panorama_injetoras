//! Interface gráfica (iced) do panorama de necessidades de materiais.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, mpsc},
};

use futures::channel::oneshot;
use iced::{
    Alignment::Center,
    Element, Font,
    Length::{Fill, FillPortion},
    Subscription, Task, Theme, font,
    alignment::Horizontal::{self, Left, Right},
    widget::{
        Column, button, canvas, center, checkbox, column, container, operation, pick_list, progress_bar, row, rule, scrollable,
        space, table, text, text_input, toggler,
    },
};

use crate::{
    api::{self, Servidor},
    calculo::{self, Alerta, Necessidade, Resultado},
    coleta::{self, Analise, DadosBrutos, Progresso},
    colunas::{AcaoColuna, Alvo, Coluna, ColunaNec, ColunaProd, ConfigColunas, CorColuna, Rgb},
    config,
    conta::{self, Cadastro, Conta},
    estrutura::{self, Estrutura, MaterialSomado, No, TipoNo},
    filtro::{self, AcaoFiltro, Busca, Filtro, Opcao, TipoUso},
    fmt,
    historico::{self, ClienteAtivo, Registro},
    locais::{self, Locais},
    modelo::{ItemComposicao, ItemOp, Op, Produto},
    notificacoes::{self, Aviso, EstadoSync},
    parcial::{self, Campos, Confirmacao, Grupo, Lancamento},
    seletor_cor::{Hsv, Seletor},
    simulacao::{self, Simulacao, Simulado},
    trello::{self, Etapa, Situacao, Situacoes},
};

const NEGRITO: Font = Font { weight: font::Weight::Bold, ..Font::DEFAULT };
const ITENS_POR_PAGINA: usize = 300;
const ID_LISTA_PRODUTOS: &str = "lista_produtos";
const QTD_PECAS_PADRAO: &str = "1";
const MAX_RESULTADOS_BUSCA_SIM: usize = 8;
/// Folga à direita das listas roláveis, para a barra de rolagem não cobrir a última coluna.
const FOLGA_ROLAGEM: f32 = 16.0;
const AVISO_SEM_COMPOSICOES: &str =
    "Composições e categorias ainda não coletadas: clique em \"Atualizar do eGestor\" para buscá-las.";

type EstiloTexto = fn(&Theme) -> text::Style;
type EstiloBotao = fn(&Theme, button::Status) -> button::Style;

pub fn executar() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title(App::titulo)
        .theme(App::tema)
        .subscription(App::subscription)
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
    Configuracoes,
}

impl Aba {
    /// Abas de dados, na ordem em que aparecem.
    const DE_DADOS: [Aba; 6] = [Self::Resumo, Self::Necessidades, Self::MaoDeObra, Self::Ops, Self::Alertas, Self::Produtos];

    /// Nome da tela nas permissões (`conta::TELAS`). Configurações é de todos.
    fn tela(self) -> Option<&'static str> {
        Some(match self {
            Self::Resumo => "resumo",
            Self::Necessidades => "necessidades",
            Self::MaoDeObra => "mao_de_obra",
            Self::Ops => "ops",
            Self::Alertas => "alertas",
            Self::Produtos => "produtos",
            Self::Configuracoes => return None,
        })
    }
}

/// Tela de entrada: entrar numa conta ou pedir uma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModoEntrada {
    Entrar,
    Criar,
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
    SaldoOpsSimulacao,
    Nome,
}

impl OrdemNec {
    const TODAS: [OrdemNec; 6] =
        [Self::CustoFalta, Self::Falta, Self::Necessidade, Self::Saldo, Self::SaldoOpsSimulacao, Self::Nome];

    fn comparar(self, x: &Necessidade, y: &Necessidade) -> std::cmp::Ordering {
        match self {
            Self::CustoFalta => y.custo_falta().total_cmp(&x.custo_falta()).then(y.falta().total_cmp(&x.falta())),
            Self::Falta => y.falta().total_cmp(&x.falta()),
            Self::Necessidade => y.necessidade.total_cmp(&x.necessidade),
            Self::Saldo => x.saldo().total_cmp(&y.saldo()),
            Self::SaldoOpsSimulacao => x.saldo_apos_ops_e_simulacao().total_cmp(&y.saldo_apos_ops_e_simulacao()),
            Self::Nome => x.descricao.cmp(&y.descricao),
        }
    }
}

impl std::fmt::Display for OrdemNec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CustoFalta => "Ordenar: maior custo da falta",
            Self::Falta => "Ordenar: maior falta (qtd)",
            Self::Necessidade => "Ordenar: maior necessidade",
            Self::Saldo => "Ordenar: menor saldo",
            Self::SaldoOpsSimulacao => "Ordenar: menor saldo após OPs e simulação",
            Self::Nome => "Ordenar: nome (A–Z)",
        })
    }
}

/// Ordem da lista da aba OPs em aberto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdemOp {
    Numero,
    Maquina,
    Situacao,
}

impl OrdemOp {
    const TODAS: [OrdemOp; 3] = [Self::Numero, Self::Maquina, Self::Situacao];
}

impl std::fmt::Display for OrdemOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Numero => "Ordenar: nº da OP (mais nova primeiro)",
            Self::Maquina => "Ordenar: máquina que está produzindo",
            Self::Situacao => "Ordenar: aguardando máquina → em máquina → aguardando baixa",
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

/// Dados próprios do panorama recarregados do servidor.
#[derive(Debug, Clone)]
pub enum Recarga {
    Colunas(ConfigColunas<ColunaNec>, ConfigColunas<ColunaProd>),
    Simulacoes(Vec<Simulacao>),
    Parciais(Vec<Lancamento>),
}

/// Gravação no servidor. Vão todas pela mesma thread, uma de cada vez e na ordem
/// em que foram feitas (ex.: "1" e depois "12" no realizado termina em 12).
#[derive(Debug)]
pub enum Pedido {
    Colunas(&'static str, serde_json::Value),
    CriarSimulacao { cod_produto: i64, quantidade: f64 },
    ExcluirSimulacao(u64),
    /// `None` apaga o lançamento.
    Parcial(parcial::Chave, Vec<Grupo>),
}

#[derive(Debug, Clone)]
pub enum Resposta {
    Nada,
    /// Realizado da OP gravado: recarregar as confirmações dela.
    Parcial(i64),
    /// A lista atualizada, com o id da simulação criada.
    Simulacoes { criada: Option<u64>, lista: Vec<Simulacao> },
}

type Envio = (Pedido, oneshot::Sender<Result<Resposta, String>>);

#[derive(Debug, Clone)]
pub enum Message {
    Atualizar,
    AtualizarTrello,
    /// Resposta ao pedido de sincronização feito ao servidor.
    PedidoSync(&'static str, Result<(), String>),
    Aviso(Aviso),
    Egestor(Result<Option<Arc<DadosBrutos>>, String>),
    Trello(Result<Arc<Situacoes>, String>),
    Recarga(Result<Recarga, String>),
    Gravado(Result<Resposta, String>),
    Exportar,
    AbrirArquivo(PathBuf),
    /// Abre um link no navegador (ex.: o cartão da OP no Trello).
    AbrirLink(String),
    AlternarTema,
    Aba(Aba),
    BuscaNec(String),
    SoFaltas(bool),
    OrdemNec(OrdemNec),
    ModoConfigNec(bool),
    /// Abre (ou fecha) o painel "Filtro de busca" da tabela.
    ModoFiltro(Tabela, bool),
    Filtro(Tabela, AcaoFiltro),
    /// Zera o filtro e desliga as opções "Somente..." e "Juntar locais" da tabela.
    LimparFiltros(Tabela),
    ModoSimulacoes(bool),
    SimBusca(String),
    SimEscolher(i64),
    SimQtd(String),
    SimCriar,
    SimVer(u64),
    SimExcluir(u64),
    GrupoEntrada(String),
    GrupoAdicionarEntrada,
    GrupoAdicionarOp(OpcaoOp),
    GrupoAdicionarTag(String),
    GrupoAlternar(i64),
    GrupoRemover(i64),
    GrupoLimpar,
    Colunas(Tabela, AcaoColuna),
    /// Abre (ou fecha) o seletor de cores da coluna `usize` da tabela.
    CorAbrir(Tabela, usize),
    CorAlvo(Alvo),
    CorMudou(Hsv),
    CorHex(String),
    /// Volta a cor escolhida (fundo ou texto) ao padrão do tema.
    CorPadrao,
    CorOk,
    CorCancelar,
    SelMaterial(i64),
    FecharMaterial,
    VerMaterial(i64),
    BuscaOp(String),
    SoOpsComFalta(bool),
    OrdemOp(OrdemOp),
    SelOp(i64),
    VerOp(i64),
    /// Campo de volumes do tipo de volume `usize` da linha.
    VolTotal(parcial::Chave, usize, String),
    VolQtd(parcial::Chave, usize, String),
    VolAdicionar(parcial::Chave),
    VolRemover(parcial::Chave, usize),
    ConfirmarRealizado(parcial::Chave),
    Confirmacoes(Result<(i64, Vec<Confirmacao>), String>),
    FiltroGrav(FiltroGrav),
    BuscaProd(String),
    SoUsados(bool),
    SoNegativos(bool),
    /// Junta numa linha só o item de produção e as cópias dele em outros locais (almoxarifado...).
    JuntarLocais(bool),
    /// O mesmo, na aba Necessidades (soma necessidade e estoque de todos os locais).
    JuntarLocaisNec(bool),
    ModoConfigProd(bool),
    SelProduto(i64),
    FecharProduto,
    PaginaProd(usize),
    VisaoComposicao(VisaoComposicao),
    QtdPecas(String),
    ConfigUrl(String),
    ConfigSalvar,
    ConfigPadrao,
    ConfigSub(SubConfig),
    /// Conta da sessão guardada, conferida no servidor (`None` = precisa entrar).
    Sessao(Result<Option<Conta>, String>),
    /// `true` = ainda não há contas: a primeira será a do administrador.
    SemContas(Result<bool, String>),
    VerificarDeNovo,
    EntModo(ModoEntrada),
    EntUsuario(String),
    EntSenha(String),
    EntSenha2(String),
    EntEnviar,
    EntRetorno(Result<Cadastro, String>),
    Sair,
    Contas(Result<Vec<Conta>, String>),
    ContasAtualizar,
    ContaTela(i64, &'static str, bool),
    ContaAutorizar(i64, bool),
    /// Primeiro clique pede confirmação; o segundo exclui. `None` cancela.
    ContaExcluir(Option<i64>),
    ContaGravada(Result<(), String>),
    HistFiltro(FiltroUsuario),
    HistMais,
    HistAtualizar,
    Historico(Result<PaginaHist, String>),
    HistUsuarios(Result<Vec<String>, String>),
    Ativos(Result<Vec<ClienteAtivo>, String>),
    AtivosAtualizar,
}

/// Telas dentro da aba Configurações.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubConfig {
    Geral,
    Historico,
    Ativos,
    /// Contas e telas liberadas: só o administrador.
    Permissoes,
}

impl SubConfig {
    /// Nome da tela nas permissões. Geral é de todos; Permissões, só do administrador.
    fn tela(self) -> Option<&'static str> {
        match self {
            Self::Geral | Self::Permissoes => None,
            Self::Historico => Some("historico"),
            Self::Ativos => Some("ativos"),
        }
    }
}

/// De quem mostrar o histórico.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FiltroUsuario {
    Todos,
    Usuario(String),
}

impl FiltroUsuario {
    fn nome(&self) -> Option<String> {
        match self {
            Self::Todos => None,
            Self::Usuario(u) => Some(u.clone()),
        }
    }
}

impl std::fmt::Display for FiltroUsuario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Todos => write!(f, "Todos os usuários"),
            Self::Usuario(u) => write!(f, "{u}"),
        }
    }
}

/// Página do histórico lida do servidor.
#[derive(Debug, Clone)]
pub struct PaginaHist {
    /// Filtro usado no pedido: se mudou nesse meio-tempo, a resposta é descartada.
    filtro: Option<String>,
    /// `true` = continuação ("Carregar mais"); `false` = primeira página.
    anexar: bool,
    lista: Vec<Registro>,
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

/// Seletor de cores aberto para uma coluna. As mudanças aparecem na hora na
/// tabela; só vão para o servidor ao fechar (Cancelar volta como estava).
#[derive(Debug, Clone)]
pub struct EditorCor {
    tabela: Tabela,
    /// Posição da coluna em `colunas`.
    i: usize,
    alvo: Alvo,
    /// Cores de quando o seletor abriu.
    original: CorColuna,
    hsv: Hsv,
    hex: String,
}

/// Cor inicial do seletor quando a coluna ainda usa a padrão: amarelo claro
/// para o fundo, azul escuro para o texto.
fn hsv_inicial(cor: CorColuna, alvo: Alvo) -> Hsv {
    match (cor.de(alvo), alvo) {
        (Some(c), _) => Hsv::de_rgb(c, 0.0),
        (None, Alvo::Fundo) => Hsv { h: 50.0, s: 0.35, v: 1.0 },
        (None, Alvo::Texto) => Hsv { h: 220.0, s: 0.8, v: 0.55 },
    }
}

pub struct App {
    analise: Option<Analise>,
    /// Andamento da sincronização do eGestor no servidor (vale para todos os panoramas abertos).
    coletando: Option<Progresso>,
    /// Conectado aos avisos do servidor.
    conectado: bool,
    /// O servidor ainda não terminou nenhuma sincronização do eGestor.
    servidor_sem_dados: bool,
    gravador: mpsc::Sender<Envio>,
    status: Option<(bool, String)>,
    ultimo_excel: Option<PathBuf>,
    escuro: bool,
    aba: Aba,
    busca_nec: String,
    so_faltas: bool,
    ordem_nec: OrdemNec,
    config_nec: ConfigColunas<ColunaNec>,
    modo_config_nec: bool,
    filtro_nec: Filtro,
    modo_filtro_nec: bool,
    filtro_prod: Filtro,
    modo_filtro_prod: bool,
    modo_simulacoes: bool,
    simulacoes: Vec<Simulacao>,
    /// Necessidades em uso (geral ou do grupo) somadas às simulações. É o que a tabela mostra.
    simulado: Simulado,
    busca_sim: String,
    /// Item escolhido na busca; digitar de novo desfaz a escolha.
    sim_item: Option<i64>,
    qtd_sim: String,
    sim_aberta: Option<u64>,
    /// OPs escolhidas para a pesquisa por grupo (vazio = necessidade geral).
    grupo_ops: BTreeSet<i64>,
    /// Necessidades recalculadas só para `grupo_ops` (None = usar a geral).
    res_grupo: Option<Resultado>,
    entrada_grupo: String,
    material_sel: Option<i64>,
    busca_op: String,
    so_ops_falta: bool,
    ordem_op: OrdemOp,
    /// Última situação lida do Trello (fica a anterior se uma nova busca falhar).
    trello: Arc<Situacoes>,
    buscando_trello: bool,
    /// Hora da última busca certa, ou o erro da última tentativa.
    status_trello: Option<Result<String, String>>,
    op_sel: Option<i64>,
    lancamentos: Vec<Lancamento>,
    /// Campos de volumes que estão sendo editados (ainda sem confirmar). Os que não
    /// estão aqui mostram o que está salvo.
    campos_vol: HashMap<parcial::Chave, Campos>,
    /// Confirmações de realizado da OP selecionada (nº da OP, lista).
    confirmacoes: Option<(i64, Vec<Confirmacao>)>,
    /// Total lançado de cada produto, para a aba Produtos.
    produzido_parcial: HashMap<i64, f64>,
    filtro_grav: FiltroGrav,
    busca_prod: String,
    so_usados: bool,
    /// Aba Produtos: só os itens com estoque abaixo de zero.
    so_negativos: bool,
    juntar_locais: bool,
    juntar_locais_nec: bool,
    /// Cópias de cada item em outros locais, calculadas quando os dados chegam.
    locais: locais::Locais,
    config_prod: ConfigColunas<ColunaProd>,
    modo_config_prod: bool,
    /// Seletor de cores aberto no painel "Configurar colunas".
    editor_cor: Option<EditorCor>,
    produto_sel: Option<i64>,
    pagina_prod: usize,
    visao_composicao: VisaoComposicao,
    /// Texto do campo de quantidade da composição, como foi digitado.
    qtd_pecas: String,
    /// Endereço do servidor em uso. Mudar refaz a conexão dos avisos.
    servidor: String,
    /// Texto do campo de endereço na aba Configurações, como foi digitado.
    cfg_url: String,
    /// Resultado do último "Salvar" da aba Configurações (erro?, mensagem).
    cfg_status: Option<(bool, String)>,
    cfg_sub: SubConfig,
    /// Nome da conta em uso. Mudar refaz a conexão dos avisos (o servidor vê o nome novo).
    usuario: String,
    /// Conta que entrou. `None` = mostra a tela de entrada.
    conta: Option<Conta>,
    /// Conferindo no servidor a sessão guardada neste computador.
    verificando: bool,
    /// Ainda não há contas no servidor (`None` = não sei).
    sem_contas: Option<bool>,
    ent_modo: ModoEntrada,
    ent_usuario: String,
    ent_senha: String,
    ent_senha2: String,
    ent_enviando: bool,
    /// Resultado da última tentativa (erro?, mensagem).
    ent_status: Option<(bool, String)>,
    /// Todas as contas (subtela Permissões; só o administrador).
    contas: Vec<Conta>,
    contas_erro: Option<String>,
    /// Conta esperando a confirmação de "Excluir".
    excluir_conta: Option<i64>,
    ip_local: Option<String>,
    historico: Vec<Registro>,
    hist_filtro: FiltroUsuario,
    hist_usuarios: Vec<String>,
    /// Não há registros mais antigos para carregar.
    hist_fim: bool,
    hist_carregando: bool,
    hist_erro: Option<String>,
    ativos: Vec<ClienteAtivo>,
    ativos_erro: Option<String>,
}

/// Roda `f` numa thread (as chamadas ao servidor bloqueiam) e entrega o resultado em `msg`.
fn em_segundo_plano<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    msg: fn(Result<T, String>) -> Message,
) -> Task<Message> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f().map_err(|e| format!("{e:#}")));
    });
    Task::perform(rx, move |r| msg(r.unwrap_or_else(|_| Err("a tarefa foi interrompida".into()))))
}

/// Como `em_segundo_plano`, mas o erro do servidor vem só com a mensagem dele
/// (ex.: "Usuário ou senha incorretos."), para mostrar na tela.
fn em_segundo_plano_curto<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    msg: fn(Result<T, String>) -> Message,
) -> Task<Message> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let r = f().map_err(|e| match e.downcast_ref::<api::ErroHttp>() {
            Some(h) => h.msg.clone(),
            None => format!("{e:#}"),
        });
        let _ = tx.send(r);
    });
    Task::perform(rx, move |r| msg(r.unwrap_or_else(|_| Err("a tarefa foi interrompida".into()))))
}

fn verificar_sessao() -> Task<Message> {
    em_segundo_plano(conta::verificar, Message::Sessao)
}

fn carregar_sem_contas() -> Task<Message> {
    em_segundo_plano(conta::sem_contas, Message::SemContas)
}

fn carregar_contas() -> Task<Message> {
    em_segundo_plano_curto(conta::listar, Message::Contas)
}

fn alterar_conta(id: i64, a: conta::Alteracao) -> Task<Message> {
    em_segundo_plano_curto(move || conta::alterar(id, &a).map(|_| ()), Message::ContaGravada)
}

/// Dados do eGestor guardados no servidor.
fn carregar_egestor() -> Task<Message> {
    em_segundo_plano(|| Ok(coleta::carregar(&Servidor::new()?)?.map(Arc::new)), Message::Egestor)
}

/// Situação das OPs no Trello guardada no servidor.
fn carregar_trello() -> Task<Message> {
    em_segundo_plano(|| Ok(Arc::new(trello::buscar(&api::url_servidor())?)), Message::Trello)
}

fn carregar_colunas() -> Task<Message> {
    fn ler<C: Coluna>(srv: &Servidor) -> anyhow::Result<ConfigColunas<C>> {
        Ok(ConfigColunas::do_servidor(srv.get(&format!("/api/panorama/colunas/{}", C::TABELA))?))
    }
    em_segundo_plano(
        || {
            let srv = Servidor::new()?;
            Ok(Recarga::Colunas(ler(&srv)?, ler(&srv)?))
        },
        Message::Recarga,
    )
}

fn carregar_simulacoes() -> Task<Message> {
    em_segundo_plano(|| Ok(Recarga::Simulacoes(Servidor::new()?.get("/api/panorama/simulacoes")?)), Message::Recarga)
}

fn carregar_parciais() -> Task<Message> {
    em_segundo_plano(|| Ok(Recarga::Parciais(Servidor::new()?.get("/api/panorama/parciais")?)), Message::Recarga)
}

fn carregar_usuarios_hist() -> Task<Message> {
    em_segundo_plano(historico::usuarios, Message::HistUsuarios)
}

fn carregar_ativos() -> Task<Message> {
    em_segundo_plano(historico::ativos, Message::Ativos)
}

/// Pede ao servidor para buscar de novo no eGestor ou no Trello.
fn pedir_sync(fonte: &'static str) -> Task<Message> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let r = Servidor::new().and_then(|s| coleta::pedir_atualizacao(&s, fonte));
        let _ = tx.send(r.map_err(|e| format!("{e:#}")));
    });
    Task::perform(rx, move |r| Message::PedidoSync(fonte, r.unwrap_or_else(|_| Err("o pedido foi interrompido".into()))))
}

/// Thread que faz as gravações no servidor, uma de cada vez, na ordem de chegada.
fn iniciar_gravador() -> mpsc::Sender<Envio> {
    let (tx, rx) = mpsc::channel::<Envio>();
    std::thread::spawn(move || {
        for (pedido, resposta) in rx {
            // Um cliente por pedido: o endereço do servidor pode mudar nas Configurações.
            let r = Servidor::new().and_then(|s| gravar_no_servidor(&s, pedido)).map_err(|e| format!("{e:#}"));
            let _ = resposta.send(r);
        }
    });
    tx
}

fn gravar_no_servidor(srv: &Servidor, pedido: Pedido) -> anyhow::Result<Resposta> {
    use reqwest::Method;
    let nada = None::<&()>;
    match pedido {
        Pedido::Colunas(tabela, config) => {
            srv.enviar(Method::PUT, &format!("/api/panorama/colunas/{tabela}"), Some(&config))?;
        }
        Pedido::CriarSimulacao { cod_produto, quantidade } => {
            let corpo = serde_json::json!({ "cod_produto": cod_produto, "quantidade": quantidade });
            let criada = srv.enviar(Method::POST, "/api/panorama/simulacoes", Some(&corpo))?;
            let criada = criada.and_then(|v| v["id"].as_u64());
            return Ok(Resposta::Simulacoes { criada, lista: srv.get("/api/panorama/simulacoes")? });
        }
        Pedido::ExcluirSimulacao(id) => {
            srv.enviar(Method::DELETE, &format!("/api/panorama/simulacoes/{id}"), nada)?;
        }
        Pedido::Parcial(chave, grupos) if grupos.is_empty() => {
            srv.enviar(Method::DELETE, &parcial::caminho(chave), nada)?;
            return Ok(Resposta::Parcial(chave.0));
        }
        Pedido::Parcial(chave, grupos) => {
            srv.enviar(Method::PUT, "/api/panorama/parciais", Some(&Lancamento::novo(chave, grupos)))?;
            return Ok(Resposta::Parcial(chave.0));
        }
    }
    Ok(Resposta::Nada)
}

impl App {
    /// Começa vazio: os dados chegam quando a conexão com o servidor abre (`Aviso::Conectado`).
    fn boot() -> (Self, Task<Message>) {
        let app = Self {
            analise: None,
            coletando: None,
            conectado: false,
            servidor_sem_dados: false,
            gravador: iniciar_gravador(),
            status: None,
            ultimo_excel: None,
            escuro: true,
            aba: Aba::Resumo,
            busca_nec: String::new(),
            so_faltas: false,
            ordem_nec: OrdemNec::CustoFalta,
            config_nec: ConfigColunas::default(),
            modo_config_nec: false,
            filtro_nec: Filtro::default(),
            modo_filtro_nec: false,
            filtro_prod: Filtro::default(),
            modo_filtro_prod: false,
            modo_simulacoes: false,
            simulacoes: Vec::new(),
            simulado: Simulado::default(),
            busca_sim: String::new(),
            sim_item: None,
            qtd_sim: String::new(),
            sim_aberta: None,
            grupo_ops: BTreeSet::new(),
            res_grupo: None,
            entrada_grupo: String::new(),
            material_sel: None,
            busca_op: String::new(),
            so_ops_falta: false,
            ordem_op: OrdemOp::Numero,
            trello: Arc::default(),
            buscando_trello: false,
            status_trello: None,
            op_sel: None,
            campos_vol: HashMap::new(),
            confirmacoes: None,
            produzido_parcial: HashMap::new(),
            lancamentos: Vec::new(),
            filtro_grav: FiltroGrav::Todas,
            busca_prod: String::new(),
            so_usados: false,
            so_negativos: false,
            juntar_locais: false,
            juntar_locais_nec: false,
            locais: locais::Locais::default(),
            config_prod: ConfigColunas::default(),
            modo_config_prod: false,
            editor_cor: None,
            produto_sel: None,
            pagina_prod: 0,
            visao_composicao: VisaoComposicao::Direta,
            qtd_pecas: QTD_PECAS_PADRAO.into(),
            servidor: api::url_servidor(),
            cfg_url: config::atual().servidor_url.unwrap_or_default(),
            cfg_status: None,
            cfg_sub: SubConfig::Geral,
            usuario: conta::nome_em_uso(),
            conta: None,
            verificando: conta::token().is_some(),
            sem_contas: None,
            ent_modo: ModoEntrada::Entrar,
            ent_usuario: config::atual().ultimo_usuario.unwrap_or_default(),
            ent_senha: String::new(),
            ent_senha2: String::new(),
            ent_enviando: false,
            ent_status: None,
            contas: Vec::new(),
            contas_erro: None,
            excluir_conta: None,
            ip_local: config::ip_local(),
            historico: Vec::new(),
            hist_filtro: FiltroUsuario::Todos,
            hist_usuarios: Vec::new(),
            hist_fim: false,
            hist_carregando: false,
            hist_erro: None,
            ativos: Vec::new(),
            ativos_erro: None,
        };
        // Com sessão guardada, confere se ainda vale; sem, mostra a tela de entrada.
        let inicio = if app.verificando { verificar_sessao() } else { carregar_sem_contas() };
        (app, inicio)
    }

    /// Os avisos (e os dados que eles trazem) só depois de entrar numa conta.
    fn subscription(&self) -> Subscription<Message> {
        if self.conta.is_none() {
            return Subscription::none();
        }
        Subscription::run_with((self.servidor.clone(), self.usuario.clone()), notificacoes::fluxo).map(Message::Aviso)
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
            // Quem busca é o servidor: aqui só se pede. O andamento chega pelos avisos,
            // e todos os panoramas abertos recarregam quando ele termina.
            Message::Atualizar => {
                if self.coletando.is_none() {
                    self.coletando = Some(Progresso { etapa: "Pedindo ao servidor...".into(), fracao: 0.0 });
                    self.status = None;
                    return Task::batch([pedir_sync("egestor"), self.atualizar_trello()]);
                }
            }
            Message::AtualizarTrello => return self.atualizar_trello(),
            Message::PedidoSync(_, Ok(())) => {}
            Message::PedidoSync(fonte, Err(e)) => {
                if fonte == "egestor" {
                    self.coletando = None;
                    self.status = Some((true, format!("Não consegui pedir a atualização do eGestor: {e}")));
                } else {
                    self.buscando_trello = false;
                    self.status_trello = Some(Err(e));
                }
            }
            Message::Aviso(aviso) => return self.aviso(aviso),
            Message::Egestor(Ok(Some(b))) => {
                self.servidor_sem_dados = false;
                let a = Analise::de(&b);
                self.status = Some((
                    false,
                    format!("Dados do eGestor de {}: {} OPs em aberto, {} produtos.", a.gerado_em, a.ops.len(), a.produtos.len()),
                ));
                if self.op_sel.is_some_and(|c| !a.ops.iter().any(|o| o.codigo == c)) {
                    self.op_sel = None;
                }
                self.locais = locais::agrupar(&a.produtos);
                self.analise = Some(a);
                // Mantém o grupo, descartando OPs que não estão mais em aberto.
                let abertas: HashSet<i64> = self.analise.iter().flat_map(|a| &a.ops).map(|o| o.codigo).collect();
                self.grupo_ops.retain(|c| abertas.contains(c));
                self.recalcular_grupo();
            }
            Message::Egestor(Ok(None)) => self.servidor_sem_dados = true,
            Message::Egestor(Err(e)) => self.status = Some((true, format!("Não consegui ler os dados do servidor: {e}"))),
            Message::Trello(r) => {
                self.buscando_trello = false;
                self.status_trello = Some(match r {
                    Ok(s) => {
                        self.trello = s;
                        Ok(chrono::Local::now().format("%H:%M").to_string())
                    }
                    Err(e) => Err(e),
                });
            }
            Message::Recarga(Ok(r)) => match r {
                Recarga::Colunas(nec, prod) => {
                    // A configuração veio de novo do servidor: o que estava em edição se perde.
                    self.editor_cor = None;
                    self.config_nec = nec;
                    self.config_prod = prod;
                }
                Recarga::Simulacoes(lista) => {
                    if self.sim_aberta.is_some_and(|id| !lista.iter().any(|s| s.id == id)) {
                        self.sim_aberta = None;
                    }
                    self.simulacoes = lista;
                    self.recalcular_simulacoes();
                }
                Recarga::Parciais(lista) => {
                    self.lancamentos = lista;
                    self.recalcular_simulacoes();
                }
            },
            Message::Recarga(Err(e)) => self.status = Some((true, format!("Não consegui ler os dados do servidor: {e}"))),
            Message::Gravado(Ok(Resposta::Nada)) => {}
            Message::Gravado(Ok(Resposta::Parcial(op))) if self.op_sel == Some(op) => return self.carregar_confirmacoes(),
            Message::Gravado(Ok(Resposta::Parcial(_))) => {}
            Message::Gravado(Ok(Resposta::Simulacoes { criada, lista })) => {
                self.simulacoes = lista;
                if criada.is_some() {
                    self.sim_aberta = criada;
                }
                self.recalcular_simulacoes();
            }
            Message::Gravado(Err(e)) => {
                self.status = Some((true, format!("Não consegui gravar no servidor: {e}")));
                // Volta a mostrar o que de fato está gravado.
                return Task::batch([carregar_colunas(), carregar_simulacoes(), carregar_parciais()]);
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
            Message::AbrirLink(url) => {
                if let Err(e) = std::process::Command::new("explorer").arg(&url).spawn() {
                    self.status = Some((true, format!("Não consegui abrir {url}: {e}")));
                }
            }
            Message::AbrirArquivo(p) => {
                if let Err(e) = std::process::Command::new("explorer").arg(&p).spawn() {
                    self.status = Some((true, format!("Não consegui abrir {}: {e}", p.display())));
                }
            }
            Message::AlternarTema => self.escuro = !self.escuro,
            Message::Aba(a) => {
                if self.aba_permitida(a) {
                    self.aba = a;
                }
                return self.recarregar_config_visivel();
            }
            Message::BuscaNec(s) => self.busca_nec = s,
            Message::SoFaltas(b) => self.so_faltas = b,
            Message::OrdemNec(o) => self.ordem_nec = o,
            // Os dois painéis ocupam o mesmo lugar: abrir um fecha o outro.
            Message::ModoConfigNec(b) => {
                self.modo_config_nec = b;
                self.modo_simulacoes &= !b;
                self.modo_filtro_nec &= !b;
                if !b && self.editor_cor.as_ref().is_some_and(|e| e.tabela == Tabela::Necessidades) {
                    return self.fechar_editor_cor(true);
                }
            }
            Message::ModoSimulacoes(b) => {
                self.modo_simulacoes = b;
                self.modo_config_nec &= !b;
                self.modo_filtro_nec &= !b;
                if b && self.editor_cor.as_ref().is_some_and(|e| e.tabela == Tabela::Necessidades) {
                    return self.fechar_editor_cor(true);
                }
            }
            Message::ModoFiltro(tabela, b) => {
                match tabela {
                    Tabela::Necessidades => {
                        self.modo_filtro_nec = b;
                        self.modo_config_nec &= !b;
                        self.modo_simulacoes &= !b;
                    }
                    Tabela::Produtos => {
                        self.modo_filtro_prod = b;
                        self.modo_config_prod &= !b;
                    }
                }
                if b && self.editor_cor.as_ref().is_some_and(|e| e.tabela == tabela) {
                    return self.fechar_editor_cor(true);
                }
            }
            Message::Filtro(Tabela::Necessidades, acao) => self.filtro_nec.aplicar(acao),
            Message::Filtro(Tabela::Produtos, acao) => {
                self.filtro_prod.aplicar(acao);
                return self.ir_para_pagina_prod(0);
            }
            Message::LimparFiltros(Tabela::Necessidades) => {
                self.filtro_nec.aplicar(AcaoFiltro::Limpar);
                self.so_faltas = false;
                if self.juntar_locais_nec {
                    return self.update(Message::JuntarLocaisNec(false));
                }
            }
            Message::LimparFiltros(Tabela::Produtos) => {
                self.filtro_prod.aplicar(AcaoFiltro::Limpar);
                self.so_usados = false;
                self.so_negativos = false;
                self.juntar_locais = false;
                return self.ir_para_pagina_prod(0);
            }
            Message::SimBusca(s) => {
                self.busca_sim = s;
                self.sim_item = None;
            }
            Message::SimEscolher(c) => {
                if let Some(p) = self.analise.as_ref().and_then(|a| a.produtos.get(&c)) {
                    self.busca_sim = p.descricao.clone();
                    self.sim_item = Some(c);
                }
            }
            Message::SimQtd(s) => self.qtd_sim = s,
            Message::SimCriar => {
                // O id vem do servidor: a simulação aparece quando ele responde.
                if let (Some(cod_produto), Some(quantidade)) = (self.sim_item, fmt::ler_qtd(&self.qtd_sim)) {
                    self.sim_item = None;
                    self.busca_sim.clear();
                    self.qtd_sim.clear();
                    return self.gravar(Pedido::CriarSimulacao { cod_produto, quantidade });
                }
            }
            Message::SimVer(id) => {
                self.sim_aberta = if self.sim_aberta == Some(id) { None } else { Some(id) };
            }
            Message::SimExcluir(id) => {
                self.simulacoes.retain(|s| s.id != id);
                if self.sim_aberta == Some(id) {
                    self.sim_aberta = None;
                }
                self.recalcular_simulacoes();
                return self.gravar(Pedido::ExcluirSimulacao(id));
            }
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
                // Mover colunas muda as posições: o seletor aberto grava e fecha antes.
                let antes = self.fechar_editor_cor(true);
                match tabela {
                    Tabela::Necessidades => self.config_nec.aplicar(acao),
                    Tabela::Produtos => self.config_prod.aplicar(acao),
                }
                return Task::batch([antes, self.gravar_colunas(tabela)]);
            }
            Message::CorAbrir(tabela, i) => {
                let mesmo = self.editor_cor.as_ref().is_some_and(|e| e.tabela == tabela && e.i == i);
                let antes = self.fechar_editor_cor(true);
                if !mesmo {
                    let original = self.cores_coluna(tabela, i);
                    let hsv = hsv_inicial(original, Alvo::Fundo);
                    let hex = original.fundo.map(Rgb::hex).unwrap_or_default();
                    self.editor_cor = Some(EditorCor { tabela, i, alvo: Alvo::Fundo, original, hsv, hex });
                }
                return antes;
            }
            Message::CorAlvo(alvo) => {
                if let Some(e) = &self.editor_cor {
                    let atual = self.cores_coluna(e.tabela, e.i);
                    let e = self.editor_cor.as_mut().expect("aberto");
                    e.alvo = alvo;
                    e.hsv = hsv_inicial(atual, alvo);
                    e.hex = atual.de(alvo).map(Rgb::hex).unwrap_or_default();
                }
            }
            Message::CorMudou(hsv) => {
                if let Some(e) = self.editor_cor.as_mut() {
                    e.hsv = hsv;
                    e.hex = hsv.rgb().hex();
                    let (tabela, i, alvo) = (e.tabela, e.i, e.alvo);
                    self.mudar_cor_local(tabela, i, alvo, Some(hsv.rgb()));
                }
            }
            Message::CorHex(s) => {
                if let Some(e) = self.editor_cor.as_mut() {
                    let lida = Rgb::do_hex(&s);
                    e.hex = s;
                    if let Some(c) = lida {
                        e.hsv = Hsv::de_rgb(c, e.hsv.h);
                        let (tabela, i, alvo) = (e.tabela, e.i, e.alvo);
                        self.mudar_cor_local(tabela, i, alvo, Some(c));
                    }
                }
            }
            Message::CorPadrao => {
                if let Some(e) = self.editor_cor.as_mut() {
                    e.hex.clear();
                    let (tabela, i, alvo) = (e.tabela, e.i, e.alvo);
                    self.mudar_cor_local(tabela, i, alvo, None);
                }
            }
            Message::CorOk => return self.fechar_editor_cor(true),
            Message::CorCancelar => return self.fechar_editor_cor(false),
            Message::SelMaterial(c) => self.material_sel = Some(c),
            Message::FecharMaterial => self.material_sel = None,
            Message::VerMaterial(c) => {
                self.material_sel = Some(c);
                self.aba = Aba::Necessidades;
            }
            Message::BuscaOp(s) => self.busca_op = s,
            Message::SoOpsComFalta(b) => self.so_ops_falta = b,
            Message::OrdemOp(o) => self.ordem_op = o,
            Message::SelOp(c) => {
                self.op_sel = Some(c);
                return self.carregar_confirmacoes();
            }
            Message::VerOp(c) => {
                self.op_sel = Some(c);
                self.aba = Aba::Ops;
                return self.carregar_confirmacoes();
            }
            Message::VolTotal(chave, i, s) => {
                if let Some(g) = self.campos_editados(chave).grupos.get_mut(i) {
                    g.volumes = s;
                }
            }
            Message::VolQtd(chave, i, s) => {
                if let Some(g) = self.campos_editados(chave).grupos.get_mut(i) {
                    g.qtd_volume = s;
                }
            }
            Message::VolAdicionar(chave) => self.campos_editados(chave).adicionar(),
            Message::VolRemover(chave, i) => self.campos_editados(chave).remover(i),
            Message::ConfirmarRealizado(chave) => {
                let Some(Ok(grupos)) = self.campos_vol.get(&chave).map(Campos::ler) else { return Task::none() };
                self.campos_vol.remove(&chave);
                if parcial::definir(&mut self.lancamentos, chave, grupos.clone()) {
                    self.recalcular_simulacoes();
                    return self.gravar(Pedido::Parcial(chave, grupos));
                }
            }
            Message::Confirmacoes(Ok((op, lista))) => {
                if self.op_sel == Some(op) {
                    self.confirmacoes = Some((op, lista));
                }
            }
            Message::Confirmacoes(Err(e)) => self.status = Some((true, format!("Não consegui ler o histórico do realizado: {e}"))),
            Message::FiltroGrav(f) => self.filtro_grav = f,
            Message::BuscaProd(s) => {
                self.busca_prod = s;
                return self.ir_para_pagina_prod(0);
            }
            Message::SoNegativos(b) => {
                self.so_negativos = b;
                return self.ir_para_pagina_prod(0);
            }
            Message::JuntarLocaisNec(b) => {
                self.juntar_locais_nec = b;
                // O material aberto pode ser uma cópia, que some: passa a valer o item de produção.
                if b && let Some(c) = self.material_sel.filter(|c| self.locais.copias.contains(c)) {
                    self.material_sel = self.locais.outros.iter().find(|(_, v)| v.iter().any(|x| x.codigo == c)).map(|(k, _)| *k);
                }
                self.recalcular_simulacoes();
            }
            Message::JuntarLocais(b) => {
                self.juntar_locais = b;
                // A cópia aberta some da lista: passa a valer o item de produção.
                if b && let Some(c) = self.produto_sel.filter(|c| self.locais.copias.contains(c)) {
                    self.produto_sel = self.locais.outros.iter().find(|(_, v)| v.iter().any(|x| x.codigo == c)).map(|(k, _)| *k);
                }
                return self.ir_para_pagina_prod(0);
            }
            Message::SoUsados(b) => {
                self.so_usados = b;
                return self.ir_para_pagina_prod(0);
            }
            Message::ModoConfigProd(b) => {
                self.modo_config_prod = b;
                self.modo_filtro_prod &= !b;
                if !b && self.editor_cor.as_ref().is_some_and(|e| e.tabela == Tabela::Produtos) {
                    return self.fechar_editor_cor(true);
                }
            }
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
            Message::ConfigUrl(s) => {
                self.cfg_url = s;
                self.cfg_status = None;
            }
            Message::ConfigSalvar => return self.salvar_config(),
            Message::ConfigPadrao => {
                self.cfg_url.clear();
                return self.salvar_config();
            }
            Message::ConfigSub(s) => {
                if self.sub_permitida(s) {
                    self.cfg_sub = s;
                    self.excluir_conta = None;
                }
                return self.recarregar_config_visivel();
            }
            Message::Sessao(r) => return self.sessao(r),
            Message::SemContas(Ok(sem)) => {
                self.sem_contas = Some(sem);
                if sem {
                    self.ent_modo = ModoEntrada::Criar;
                }
            }
            Message::SemContas(Err(e)) => {
                self.sem_contas = None;
                self.ent_status = Some((true, format!("Não consegui falar com o servidor: {e}")));
            }
            Message::VerificarDeNovo => {
                self.ent_status = None;
                if conta::token().is_some() {
                    self.verificando = true;
                    return verificar_sessao();
                }
                return carregar_sem_contas();
            }
            Message::EntModo(m) => {
                self.ent_modo = m;
                self.ent_status = None;
            }
            Message::EntUsuario(s) => self.ent_usuario = s,
            Message::EntSenha(s) => self.ent_senha = s,
            Message::EntSenha2(s) => self.ent_senha2 = s,
            Message::EntEnviar => return self.enviar_entrada(),
            Message::EntRetorno(r) => {
                self.ent_enviando = false;
                self.ent_senha.clear();
                self.ent_senha2.clear();
                match r {
                    Ok(Cadastro::Entrou(c)) => {
                        self.ent_status = None;
                        return self.entrou(c);
                    }
                    Ok(Cadastro::Pendente) => {
                        self.ent_modo = ModoEntrada::Entrar;
                        self.sem_contas = Some(false);
                        self.ent_status = Some((
                            false,
                            "Conta criada! Agora o administrador precisa autorizar o seu acesso. \
                             Depois disso, entre com o seu usuário e senha."
                                .into(),
                        ));
                    }
                    Err(e) => self.ent_status = Some((true, e)),
                }
            }
            Message::Sair => {
                if let Err(e) = conta::sair() {
                    self.status = Some((true, format!("Não consegui esquecer a sessão: {e:#}")));
                }
                self.saiu(None);
                return carregar_sem_contas();
            }
            Message::Contas(Ok(lista)) => {
                self.contas = lista;
                self.contas_erro = None;
            }
            Message::Contas(Err(e)) => self.contas_erro = Some(e),
            Message::ContasAtualizar => return carregar_contas(),
            Message::ContaTela(id, tela, liberar) => {
                let Some(c) = self.contas.iter_mut().find(|c| c.id == id) else { return Task::none() };
                // Muda na hora na tela; a resposta do servidor confirma (ou desfaz, se der erro).
                c.telas.retain(|t| t != tela);
                if liberar {
                    c.telas.push(tela.to_string());
                }
                let telas = c.telas.clone();
                return alterar_conta(id, conta::Alteracao { telas: Some(telas), ..Default::default() });
            }
            Message::ContaAutorizar(id, aprovado) => {
                if let Some(c) = self.contas.iter_mut().find(|c| c.id == id) {
                    c.aprovado = aprovado;
                }
                return alterar_conta(id, conta::Alteracao { aprovado: Some(aprovado), ..Default::default() });
            }
            Message::ContaExcluir(None) => self.excluir_conta = None,
            Message::ContaExcluir(Some(id)) if self.excluir_conta == Some(id) => {
                self.excluir_conta = None;
                self.contas.retain(|c| c.id != id);
                return em_segundo_plano_curto(move || conta::excluir(id), Message::ContaGravada);
            }
            Message::ContaExcluir(Some(id)) => self.excluir_conta = Some(id),
            Message::ContaGravada(r) => {
                if let Err(e) = r {
                    self.contas_erro = Some(format!("Não gravei: {e}"));
                }
                return carregar_contas();
            }
            Message::HistFiltro(f) => {
                self.hist_filtro = f;
                self.historico.clear();
                return self.carregar_historico(false);
            }
            Message::HistMais => return self.carregar_historico(true),
            Message::HistAtualizar => {
                return Task::batch([self.carregar_historico(false), carregar_usuarios_hist()]);
            }
            Message::Historico(Ok(p)) => {
                if p.filtro == self.hist_filtro.nome() {
                    self.hist_carregando = false;
                    self.hist_erro = None;
                    self.hist_fim = p.lista.len() < historico::POR_PAGINA;
                    if !p.anexar {
                        self.historico.clear();
                    }
                    self.historico.extend(p.lista);
                }
            }
            Message::Historico(Err(e)) => {
                self.hist_carregando = false;
                self.hist_erro = Some(e);
            }
            Message::HistUsuarios(Ok(u)) => self.hist_usuarios = u,
            Message::HistUsuarios(Err(e)) => self.hist_erro = Some(e),
            Message::Ativos(Ok(lista)) => {
                self.ativos = lista;
                self.ativos_erro = None;
            }
            Message::Ativos(Err(e)) => self.ativos_erro = Some(e),
            Message::AtivosAtualizar => return carregar_ativos(),
        }
        Task::none()
    }

    /// Campos de volumes da linha para editar: começa do que está salvo.
    fn campos_editados(&mut self, chave: parcial::Chave) -> &mut Campos {
        let salvo = self.lancamento(chave).cloned();
        self.campos_vol.entry(chave).or_insert_with(|| Campos::do_lancamento(salvo.as_ref()))
    }

    fn lancamento(&self, (op, linha, cod_produto): parcial::Chave) -> Option<&Lancamento> {
        self.lancamentos.iter().find(|l| l.op == op && l.linha == linha && l.cod_produto == cod_produto)
    }

    /// Lê do servidor as confirmações de realizado da OP selecionada.
    fn carregar_confirmacoes(&self) -> Task<Message> {
        let Some(op) = self.op_sel else { return Task::none() };
        em_segundo_plano(
            move || Ok((op, Servidor::new()?.get(&format!("/api/panorama/parciais/lancamentos?op={op}"))?)),
            Message::Confirmacoes,
        )
    }

    /// Recarrega o histórico ou os usuários ativos, se estiverem na tela.
    fn recarregar_config_visivel(&mut self) -> Task<Message> {
        if self.aba != Aba::Configuracoes {
            return Task::none();
        }
        match self.cfg_sub {
            SubConfig::Geral => Task::none(),
            SubConfig::Historico => Task::batch([self.carregar_historico(false), carregar_usuarios_hist()]),
            SubConfig::Ativos => carregar_ativos(),
            SubConfig::Permissoes => carregar_contas(),
        }
    }

    // ------------------------------------------------------ conta ---

    fn aba_permitida(&self, aba: Aba) -> bool {
        match (aba.tela(), &self.conta) {
            (None, _) => true,
            (Some(t), Some(c)) => c.pode(t),
            (Some(_), None) => false,
        }
    }

    fn sub_permitida(&self, sub: SubConfig) -> bool {
        match (sub, &self.conta) {
            (_, None) => false,
            (SubConfig::Permissoes, Some(c)) => c.admin,
            (s, Some(c)) => s.tela().is_none_or(|t| c.pode(t)),
        }
    }

    /// Se a aba (ou a subtela) aberta deixou de ser permitida, vai para a primeira que é.
    fn ajustar_aba(&mut self) {
        if !self.aba_permitida(self.aba) {
            self.aba = Aba::DE_DADOS.into_iter().find(|&a| self.aba_permitida(a)).unwrap_or(Aba::Configuracoes);
        }
        if !self.sub_permitida(self.cfg_sub) {
            self.cfg_sub = SubConfig::Geral;
        }
    }

    /// Resposta da conferência da sessão (ao abrir, ao trocar de servidor ou
    /// quando o administrador muda alguma conta).
    fn sessao(&mut self, r: Result<Option<Conta>, String>) -> Task<Message> {
        self.verificando = false;
        match r {
            Ok(Some(c)) => self.entrou(c),
            Ok(None) => {
                let aviso = self.conta.as_ref().map(|_| {
                    "Sua sessão foi encerrada: a conta foi excluída ou perdeu a autorização.".to_string()
                });
                self.saiu(aviso);
                carregar_sem_contas()
            }
            // Já dentro: um tropeço de rede não tira ninguém do programa.
            Err(_) if self.conta.is_some() => Task::none(),
            Err(e) => {
                self.ent_status = Some((true, format!("Não consegui conferir a sua sessão no servidor: {e}")));
                Task::none()
            }
        }
    }

    /// Entrou (ou as permissões mudaram). Com outro nome, a conexão de avisos é
    /// refeita e tudo é recarregado (as colunas são de cada usuário).
    fn entrou(&mut self, c: Conta) -> Task<Message> {
        let primeira_vez = self.conta.is_none();
        self.usuario = c.usuario.clone();
        let admin = c.admin;
        self.conta = Some(c);
        if primeira_vez {
            self.aba = Aba::Resumo;
            self.cfg_sub = SubConfig::Geral;
        }
        self.ajustar_aba();
        if admin { carregar_contas() } else { Task::none() }
    }

    /// Volta à tela de entrada e esquece os dados lidos com a conta.
    fn saiu(&mut self, aviso: Option<String>) {
        self.conta = None;
        self.usuario = conta::nome_em_uso();
        self.contas.clear();
        self.excluir_conta = None;
        self.ent_modo = ModoEntrada::Entrar;
        self.ent_usuario = config::atual().ultimo_usuario.unwrap_or_default();
        self.ent_status = aviso.map(|a| (true, a));
        self.limpar_dados();
    }

    fn enviar_entrada(&mut self) -> Task<Message> {
        if self.ent_enviando {
            return Task::none();
        }
        let usuario = self.ent_usuario.trim().to_string();
        let senha = self.ent_senha.clone();
        let erro = match self.ent_modo {
            _ if usuario.is_empty() => Some("Informe o usuário."),
            _ if senha.is_empty() => Some("Informe a senha."),
            ModoEntrada::Criar if senha.chars().count() < 4 => Some("A senha deve ter pelo menos 4 caracteres."),
            ModoEntrada::Criar if senha != self.ent_senha2 => Some("As senhas não são iguais."),
            _ => None,
        };
        if let Some(e) = erro {
            self.ent_status = Some((true, e.into()));
            return Task::none();
        }
        self.ent_enviando = true;
        self.ent_status = None;
        match self.ent_modo {
            ModoEntrada::Entrar => {
                em_segundo_plano_curto(move || conta::entrar(&usuario, &senha).map(Cadastro::Entrou), Message::EntRetorno)
            }
            ModoEntrada::Criar => em_segundo_plano_curto(move || conta::cadastrar(&usuario, &senha), Message::EntRetorno),
        }
    }

    /// Primeira página do histórico (`anexar = false`) ou a seguinte, mais antiga.
    fn carregar_historico(&mut self, anexar: bool) -> Task<Message> {
        let antes = if anexar { self.historico.last().map(|r| r.id) } else { None };
        if anexar && antes.is_none() {
            return Task::none();
        }
        self.hist_carregando = true;
        let filtro = self.hist_filtro.nome();
        em_segundo_plano(
            move || {
                let lista = historico::buscar(filtro.as_deref(), antes)?;
                Ok(PaginaHist { filtro, anexar, lista })
            },
            Message::Historico,
        )
    }

    /// Envia ao servidor a configuração de colunas da tabela.
    fn gravar_colunas(&self, tabela: Tabela) -> Task<Message> {
        let pedido = match tabela {
            Tabela::Necessidades => {
                Pedido::Colunas(ColunaNec::TABELA, serde_json::to_value(&self.config_nec).expect("serializável"))
            }
            Tabela::Produtos => Pedido::Colunas(ColunaProd::TABELA, serde_json::to_value(&self.config_prod).expect("serializável")),
        };
        self.gravar(pedido)
    }

    fn cores_coluna(&self, tabela: Tabela, i: usize) -> CorColuna {
        match tabela {
            Tabela::Necessidades => self.config_nec.colunas.get(i).map(|&(c, _)| self.config_nec.cor_de(c)),
            Tabela::Produtos => self.config_prod.colunas.get(i).map(|&(c, _)| self.config_prod.cor_de(c)),
        }
        .unwrap_or_default()
    }

    /// Muda a cor só na tela (prévia); vai para o servidor ao fechar o seletor.
    fn mudar_cor_local(&mut self, tabela: Tabela, i: usize, alvo: Alvo, cor: Option<Rgb>) {
        match tabela {
            Tabela::Necessidades => self.config_nec.aplicar(AcaoColuna::Cor(i, alvo, cor)),
            Tabela::Produtos => self.config_prod.aplicar(AcaoColuna::Cor(i, alvo, cor)),
        }
    }

    /// Fecha o seletor de cores: `salvar` grava as cores escolhidas (se mudaram);
    /// senão, volta às de quando abriu.
    fn fechar_editor_cor(&mut self, salvar: bool) -> Task<Message> {
        let Some(e) = self.editor_cor.take() else {
            return Task::none();
        };
        if self.cores_coluna(e.tabela, e.i) == e.original {
            return Task::none();
        }
        if salvar {
            return self.gravar_colunas(e.tabela);
        }
        match e.tabela {
            Tabela::Necessidades => {
                if let Some(&(c, _)) = self.config_nec.colunas.get(e.i) {
                    self.config_nec.definir_cores(c, e.original);
                }
            }
            Tabela::Produtos => {
                if let Some(&(c, _)) = self.config_prod.colunas.get(e.i) {
                    self.config_prod.definir_cores(c, e.original);
                }
            }
        }
        Task::none()
    }

    /// Pede ao servidor para ler o Trello de novo; a situação nova chega pelos avisos.
    fn atualizar_trello(&mut self) -> Task<Message> {
        if self.buscando_trello {
            return Task::none();
        }
        self.buscando_trello = true;
        pedir_sync("trello")
    }

    /// Manda a gravação para a thread do gravador; a resposta volta em `Message::Gravado`.
    fn gravar(&self, pedido: Pedido) -> Task<Message> {
        let (tx, rx) = oneshot::channel();
        if self.gravador.send((pedido, tx)).is_err() {
            return Task::done(Message::Gravado(Err("a gravação no servidor parou; feche e abra o programa".into())));
        }
        Task::perform(rx, |r| Message::Gravado(r.unwrap_or_else(|_| Err("a gravação foi interrompida".into()))))
    }

    /// Grava as configurações. Se o endereço do servidor mudou, descarta os dados
    /// do servidor antigo e confere a sessão no novo (cada servidor tem as suas
    /// contas); a nova conexão de avisos recarrega tudo ao abrir.
    fn salvar_config(&mut self) -> Task<Message> {
        let url = match config::normalizar_url(&self.cfg_url) {
            Ok(u) => u,
            Err(e) => {
                self.cfg_status = Some((true, format!("Não salvei: {e}")));
                return Task::none();
            }
        };
        let mut nova = config::atual();
        nova.servidor_url = url;
        if let Err(e) = config::salvar(nova.clone()) {
            self.cfg_status = Some((true, format!("Não salvei: {e:#}")));
            return Task::none();
        }
        self.cfg_url = nova.servidor_url.unwrap_or_default();
        let servidor = api::url_servidor();
        if servidor == self.servidor {
            self.cfg_status = Some((false, "Configurações salvas.".into()));
            return Task::none();
        }
        self.servidor = servidor;
        self.ip_local = config::ip_local();
        self.limpar_dados();
        self.cfg_status = Some((false, format!("Configurações salvas. Conectando a {}...", self.servidor)));
        self.ent_status = None;
        self.sem_contas = None;
        if conta::token().is_some() {
            self.verificando = self.conta.is_none();
            verificar_sessao()
        } else {
            carregar_sem_contas()
        }
    }

    /// Esquece os dados lidos do servidor (trocou de servidor ou saiu da conta).
    fn limpar_dados(&mut self) {
        self.historico.clear();
        self.hist_usuarios.clear();
        self.ativos.clear();
        self.analise = None;
        self.coletando = None;
        self.conectado = false;
        self.servidor_sem_dados = false;
        self.buscando_trello = false;
        self.status = None;
        self.trello = Arc::default();
        self.simulacoes.clear();
        self.lancamentos.clear();
        self.confirmacoes = None;
        self.recalcular_grupo();
    }

    fn aviso(&mut self, aviso: Aviso) -> Task<Message> {
        match aviso {
            // Pode ter perdido avisos enquanto estava desconectado: recarrega tudo.
            Aviso::Conectado => {
                self.conectado = true;
                if self.status.as_ref().is_some_and(|(erro, _)| *erro) {
                    self.status = None;
                }
                self.buscando_trello = true;
                Task::batch([
                    carregar_egestor(),
                    carregar_trello(),
                    carregar_colunas(),
                    carregar_simulacoes(),
                    carregar_parciais(),
                    self.recarregar_config_visivel(),
                ])
            }
            Aviso::Desconectado(e) => {
                self.conectado = false;
                self.coletando = None;
                self.buscando_trello = false;
                self.status = Some((true, format!("Sem conexão com o servidor; tentando de novo... ({e})")));
                Task::none()
            }
            Aviso::Sync { fonte, estado, etapa, fracao, erro } => match (fonte.as_str(), estado) {
                ("egestor", EstadoSync::Andamento) => {
                    self.coletando = Some(Progresso { etapa, fracao });
                    Task::none()
                }
                ("egestor", EstadoSync::Concluido) => {
                    self.coletando = None;
                    carregar_egestor()
                }
                ("egestor", EstadoSync::Erro) => {
                    self.coletando = None;
                    let e = erro.unwrap_or_default();
                    self.status = Some((true, format!("O servidor não conseguiu atualizar do eGestor: {e}")));
                    Task::none()
                }
                ("trello", EstadoSync::Andamento) => {
                    self.buscando_trello = true;
                    Task::none()
                }
                ("trello", EstadoSync::Concluido) => carregar_trello(),
                ("trello", EstadoSync::Erro) => {
                    self.buscando_trello = false;
                    self.status_trello = Some(Err(erro.unwrap_or_default()));
                    Task::none()
                }
                _ => Task::none(),
            },
            Aviso::Dados(recurso) => match recurso.as_str() {
                "colunas" => carregar_colunas(),
                "simulacoes" => carregar_simulacoes(),
                "parciais" => Task::batch([carregar_parciais(), self.carregar_confirmacoes()]),
                "historico" if self.aba == Aba::Configuracoes && self.cfg_sub == SubConfig::Historico => {
                    Task::batch([self.carregar_historico(false), carregar_usuarios_hist()])
                }
                "clientes" if self.aba == Aba::Configuracoes && self.cfg_sub == SubConfig::Ativos => carregar_ativos(),
                // O administrador mudou alguma conta: relê a própria (permissões valem na hora).
                // A lista de contas do administrador é relida na resposta (`entrou`).
                "contas" => verificar_sessao(),
                _ => Task::none(),
            },
        }
    }

    /// Situações da OP no Trello (vazio = sem cartão nas 3 listas).
    fn situacoes(&self, op: i64) -> &[Situacao] {
        self.trello.get(&op).map_or(&[], Vec::as_slice)
    }

    /// Troca a página e volta a rolagem ao topo, para a nova página começar do primeiro item.
    fn ir_para_pagina_prod(&mut self, pagina: usize) -> Task<Message> {
        self.pagina_prod = pagina;
        operation::snap_to(ID_LISTA_PRODUTOS, scrollable::RelativeOffset::START)
    }

    /// Produtos que passam pela busca e pelo filtro da aba Produtos (sem ordem definida).
    /// Cópias em outros locais, quando a opção de juntá-las está ligada.
    fn locais_juntados(&self) -> Option<&Locais> {
        self.juntar_locais.then_some(&self.locais)
    }

    /// Cada um vem com a nota da busca por aproximação (0 com a busca vazia).
    fn produtos_filtrados<'a>(&self, a: &'a Analise) -> Vec<(i64, &'a Produto)> {
        let busca = Busca::nova(&self.busca_prod);
        let usados: HashSet<i64> = if self.so_usados {
            a.ops
                .iter()
                .flat_map(|o| &o.itens)
                .flat_map(|i| std::iter::once(i.cod_produto).chain(i.insumos.iter().map(|x| x.cod_insumo)))
                .collect()
        } else {
            HashSet::new()
        };
        let juntar = self.locais_juntados();
        // Com os locais juntados, a linha é do item de produção e vale para todas as cópias:
        // a busca acha por qualquer uma, e "estoque negativo" olha o total.
        let nota = |p: &Produto| {
            let codigo = p.codigo.to_string();
            let campos = [p.descricao.as_str(), &p.codigo_proprio, &codigo, a.categoria_de(p.cod_categoria)];
            busca.nota(campos.into_iter().chain(p.tags.iter().map(String::as_str)))
        };
        let filtro = &self.filtro_prod;
        let usado = |p: &Produto| match juntar {
            Some(l) => l.codigos(p).any(|c| usados.contains(&c)),
            None => usados.contains(&p.codigo),
        };
        a.produtos
            .values()
            .filter(|p| juntar.is_none_or(|l| !l.copias.contains(&p.codigo)))
            .filter(|p| !self.so_usados || usado(p))
            .filter(|p| !self.so_negativos || juntar.map_or(p.estoque, |l| l.total(p)) < 0.0)
            .filter(|p| filtro.aceita(&p.codigo_proprio, a.categoria_de(p.cod_categoria), &p.tags))
            .filter_map(|p| {
                let n = match juntar {
                    Some(l) => l.codigos(p).filter_map(|c| a.produtos.get(&c)).filter_map(nota).max(),
                    None => nota(p),
                };
                Some((n?, p))
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
        self.recalcular_simulacoes();
    }

    /// Refaz as linhas da aba Necessidades: mudou o grupo, os dados, as simulações ou os lançamentos.
    fn recalcular_simulacoes(&mut self) {
        self.produzido_parcial =
            self.analise.as_ref().map(|a| parcial::produzidos(&a.ops, &self.lancamentos)).unwrap_or_default();
        self.simulado = match &self.analise {
            Some(a) => {
                let mut s = simulacao::calcular(a, &self.res_necessidades(a).necessidades, &self.simulacoes);
                // Sempre de todas as OPs abertas, mesmo com grupo: o material já saiu do estoque físico.
                parcial::aplicar(&mut s.linhas, &parcial::consumos(&a.ops, &self.lancamentos));
                if self.juntar_locais_nec {
                    locais::juntar_necessidades(&mut s.linhas, &self.locais, &a.produtos, |p| {
                        a.categoria_de(p.cod_categoria).to_string()
                    });
                }
                s
            }
            None => Simulado::default(),
        };
    }

    /// Resultado usado na aba Necessidades: do grupo, se houver, senão o geral.
    fn res_necessidades<'a>(&'a self, a: &'a Analise) -> &'a Resultado {
        self.res_grupo.as_ref().unwrap_or(&a.res)
    }

    // ============================================================ view ===

    fn view(&self) -> Element<'_, Message> {
        if self.conta.is_none() {
            return self.view_entrada();
        }
        let corpo: Element<'_, Message> = match &self.analise {
            _ if self.aba == Aba::Configuracoes => self.view_configuracoes(),
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
                Aba::Configuracoes => unreachable!("tratada acima"),
            },
        };

        column![self.view_cabecalho(), self.view_abas(self.analise.as_ref()), rule::horizontal(1), corpo]
            .spacing(10)
            .padding(16)
            .into()
    }

    fn view_cabecalho(&self) -> Element<'_, Message> {
        let quem = match &self.conta {
            Some(c) if c.admin => format!("{} (administrador)", c.usuario),
            Some(c) => c.usuario.clone(),
            None => String::new(),
        };
        let sub = match &self.analise {
            Some(a) => format!("Dados coletados em {}  ·  servidor {}  ·  {quem}", a.gerado_em, self.servidor),
            None => format!("Sem dados  ·  servidor {}  ·  {quem}", self.servidor),
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

    /// Contas esperando o administrador autorizar.
    fn pendentes(&self) -> usize {
        self.contas.iter().filter(|c| !c.aprovado).count()
    }

    /// Sem dados, só Resumo (a tela de "nenhum dado") e Configurações. Só as abas
    /// que a conta pode abrir.
    fn view_abas(&self, a: Option<&Analise>) -> Element<'_, Message> {
        let abas = match a {
            Some(a) => self.abas_com_dados(a),
            None => vec![(Aba::Resumo, "Resumo".to_string())],
        };
        let configuracoes = match self.pendentes() {
            0 => "Configurações".to_string(),
            1 => "Configurações · 1 conta para autorizar".to_string(),
            n => format!("Configurações · {n} contas para autorizar"),
        };
        let mut r = row![].spacing(4);
        let abas = abas.into_iter().filter(|(aba, _)| self.aba_permitida(*aba));
        for (aba, rotulo) in abas.chain([(Aba::Configuracoes, configuracoes)]) {
            let estilo: EstiloBotao = if self.aba == aba { button::primary } else { button::text };
            r = r.push(button(text(rotulo).size(14)).padding([6, 14]).style(estilo).on_press(Message::Aba(aba)));
        }
        r.into()
    }

    fn abas_com_dados(&self, a: &Analise) -> Vec<(Aba, String)> {
        let n_falta = a.tot.materiais_em_falta;
        let n_alta = a.res.alertas.iter().filter(|x| x.gravidade == "ALTA").count();
        vec![
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
        ]
    }

    // -------------------------------------------------- Configurações ---

    fn view_configuracoes(&self) -> Element<'_, Message> {
        let subs = [
            (SubConfig::Geral, "Geral".to_string()),
            (SubConfig::Historico, "Histórico".to_string()),
            (SubConfig::Ativos, format!("Usuários ativos{}", if self.ativos.is_empty() { String::new() } else { format!(" ({})", self.ativos.len()) })),
            (SubConfig::Permissoes, match self.pendentes() {
                0 => "Permissões".to_string(),
                n => format!("Permissões ({n} para autorizar)"),
            }),
        ];
        let mut abas = row![].spacing(4);
        for (sub, rotulo) in subs.into_iter().filter(|(s, _)| self.sub_permitida(*s)) {
            let estilo: EstiloBotao = if self.cfg_sub == sub { button::secondary } else { button::text };
            abas = abas.push(button(text(rotulo).size(13)).padding([4, 12]).style(estilo).on_press(Message::ConfigSub(sub)));
        }
        let corpo = match self.cfg_sub {
            SubConfig::Geral => self.view_config_geral(),
            SubConfig::Historico => self.view_config_historico(),
            SubConfig::Ativos => self.view_config_ativos(),
            SubConfig::Permissoes => self.view_config_permissoes(),
        };
        column![abas, corpo].spacing(12).into()
    }

    /// Todas as contas: autorizar, escolher as telas de cada uma, excluir.
    fn view_config_permissoes(&self) -> Element<'_, Message> {
        let mut col = column![
            row![
                text("Contas do Panorama").size(15).font(NEGRITO),
                space::horizontal(),
                button(text("Atualizar").size(12)).padding([4, 10]).style(button::secondary).on_press(Message::ContasAtualizar),
            ]
            .align_y(Center),
            text(
                "Quem cria uma conta só entra depois que você autorizar, e abre só as telas marcadas. \
                 As mudanças valem na hora, inclusive para quem está com o programa aberto."
            )
            .size(12)
            .style(text::secondary),
        ]
        .spacing(8);
        if let Some(e) = &self.contas_erro {
            col = col.push(text(e.as_str()).size(13).style(text::danger));
        }

        let mut linhas = column![].spacing(6);
        for c in &self.contas {
            let (situacao, estilo): (&str, EstiloTexto) = match (c.admin, c.aprovado) {
                (true, _) => ("Administrador", text::primary),
                (false, true) => ("Autorizado", text::success),
                (false, false) => ("Aguardando autorização", text::warning),
            };
            let mut acoes = row![].spacing(6).align_y(Center);
            if !c.admin {
                let id = c.id;
                acoes = acoes.push(if c.aprovado {
                    button(text("Tirar autorização").size(12))
                        .padding([4, 10])
                        .style(button::secondary)
                        .on_press(Message::ContaAutorizar(id, false))
                } else {
                    button(text("Autorizar").size(12)).padding([4, 10]).style(button::success).on_press(Message::ContaAutorizar(id, true))
                });
                acoes = acoes.push(if self.excluir_conta == Some(id) {
                    row![
                        button(text("Confirmar exclusão").size(12))
                            .padding([4, 10])
                            .style(button::danger)
                            .on_press(Message::ContaExcluir(Some(id))),
                        button(text("Cancelar").size(12)).padding([4, 10]).style(button::text).on_press(Message::ContaExcluir(None)),
                    ]
                    .spacing(6)
                } else {
                    row![
                        button(text(if c.aprovado { "Excluir" } else { "Recusar" }).size(12))
                            .padding([4, 10])
                            .style(button::text)
                            .on_press(Message::ContaExcluir(Some(id)))
                    ]
                });
            }
            let topo = row![
                column![
                    text(c.usuario.as_str()).size(15).font(NEGRITO),
                    text(format!("Criada em {}", historico::hora_local(&c.criado_em))).size(11).style(text::secondary),
                ]
                .spacing(2)
                .width(260),
                text(situacao).size(13).style(estilo).width(200),
                space::horizontal(),
                acoes,
            ]
            .spacing(10)
            .align_y(Center);

            let mut telas = row![text("Telas:").size(12).style(text::secondary)].spacing(14).align_y(Center);
            for (id_tela, nome) in conta::TELAS {
                let mut marcador = checkbox(c.pode(id_tela)).label(nome).text_size(12);
                if !c.admin {
                    let id = c.id;
                    marcador = marcador.on_toggle(move |v| Message::ContaTela(id, id_tela, v));
                }
                telas = telas.push(marcador);
            }
            let telas: Element<'_, Message> = if c.admin {
                column![telas, text("O administrador sempre abre todas as telas, inclusive Permissões.").size(11).style(text::secondary)]
                    .spacing(4)
                    .into()
            } else {
                telas.wrap().into()
            };
            linhas = linhas.push(container(column![topo, telas].spacing(10)).padding([10, 12]).style(container::rounded_box));
        }
        if self.contas.is_empty() && self.contas_erro.is_none() {
            linhas = linhas.push(text("Carregando...").size(13).style(text::secondary));
        }
        col.push(scrollable(linhas.padding(iced::Padding::ZERO.right(FOLGA_ROLAGEM))).height(Fill)).into()
    }

    /// Antes de entrar: entrar numa conta, ou criar uma (a primeira é a do administrador).
    fn view_entrada(&self) -> Element<'_, Message> {
        let titulo = column![
            text("Panorama de Injetados").size(26).font(NEGRITO),
            text("Necessidades de materiais das OPs em aberto — eGestor").size(13).style(text::secondary),
        ]
        .spacing(2);

        if self.verificando {
            let c = column![titulo, text(format!("Conferindo a sessão em {}...", self.servidor)).size(14)].spacing(16);
            return center(container(c).padding(24).max_width(460).style(container::rounded_box)).into();
        }

        let primeira = self.sem_contas == Some(true);
        let criar = self.ent_modo == ModoEntrada::Criar;
        let mut c = column![titulo].spacing(12);
        if primeira {
            c = c.push(
                text("Ainda não há nenhuma conta. A conta criada agora será a do administrador: entra direto e é quem autoriza as próximas.")
                    .size(13)
                    .style(text::warning),
            );
        } else {
            let aba = |rotulo: &'static str, modo: ModoEntrada| {
                let estilo: EstiloBotao = if self.ent_modo == modo { button::secondary } else { button::text };
                button(text(rotulo).size(13)).padding([4, 12]).style(estilo).on_press(Message::EntModo(modo))
            };
            c = c.push(row![aba("Entrar", ModoEntrada::Entrar), aba("Criar conta", ModoEntrada::Criar)].spacing(4));
            if criar {
                c = c.push(
                    text("Depois de criar, o administrador precisa autorizar a sua conta antes do primeiro acesso.")
                        .size(12)
                        .style(text::secondary),
                );
            }
        }

        let campo = |rotulo: &'static str, valor: &str, ao_mudar: fn(String) -> Message, segredo: bool| {
            column![
                text(rotulo).size(12).style(text::secondary),
                text_input("", valor).on_input(ao_mudar).on_submit(Message::EntEnviar).secure(segredo).padding(8),
            ]
            .spacing(4)
        };
        c = c.push(campo("Usuário", &self.ent_usuario, Message::EntUsuario, false));
        c = c.push(campo("Senha", &self.ent_senha, Message::EntSenha, true));
        if criar {
            c = c.push(campo("Repita a senha", &self.ent_senha2, Message::EntSenha2, true));
        }
        let rotulo = match (self.ent_enviando, criar, primeira) {
            (true, _, _) => "Aguarde...",
            (false, true, true) => "Criar conta de administrador",
            (false, true, false) => "Criar conta",
            (false, false, _) => "Entrar",
        };
        c = c.push(
            button(text(rotulo).size(14).width(Fill).align_x(Center))
                .width(Fill)
                .padding(10)
                .style(button::primary)
                .on_press_maybe((!self.ent_enviando).then_some(Message::EntEnviar)),
        );
        if let Some((erro, msg)) = &self.ent_status {
            let estilo: EstiloTexto = if *erro { text::danger } else { text::success };
            c = c.push(text(msg.as_str()).size(13).style(estilo));
            if *erro {
                c = c.push(
                    button(text("Tentar de novo").size(12)).padding([4, 10]).style(button::secondary).on_press(Message::VerificarDeNovo),
                );
            }
        }

        // O endereço do servidor também fica aqui: sem ele não dá para entrar.
        let digitado = config::normalizar_url(&self.cfg_url);
        let mudou = digitado.as_ref().is_ok_and(|u| *u != config::atual().servidor_url);
        let mut servidor = column![
            text(format!("Servidor: {}", self.servidor)).size(12).style(text::secondary),
            row![
                text_input(&format!("Vazio = {}", config::url_padrao()), &self.cfg_url)
                    .on_input(Message::ConfigUrl)
                    .on_submit(Message::ConfigSalvar)
                    .size(12),
                button(text("Salvar").size(12)).padding([4, 10]).style(button::secondary).on_press_maybe(mudou.then_some(Message::ConfigSalvar)),
            ]
            .spacing(6)
            .align_y(Center),
        ]
        .spacing(4);
        if let Err(e) = &digitado {
            servidor = servidor.push(text(format!("Endereço inválido: {e}")).size(12).style(text::warning));
        }
        c = c.push(rule::horizontal(1)).push(servidor);

        // A rolagem ocupa a largura toda: o cartão é centralizado dentro dela.
        let cartao = container(c).padding(24).max_width(460).style(container::rounded_box);
        center(scrollable(container(cartao).center_x(Fill))).padding(16).into()
    }

    fn view_config_historico(&self) -> Element<'_, Message> {
        let mut opcoes = vec![FiltroUsuario::Todos];
        let mut nomes = self.hist_usuarios.clone();
        if !nomes.contains(&self.usuario) {
            nomes.push(self.usuario.clone());
            nomes.sort();
        }
        opcoes.extend(nomes.into_iter().map(FiltroUsuario::Usuario));
        let so_eu = FiltroUsuario::Usuario(self.usuario.clone());

        let mut topo = row![
            pick_list(opcoes, Some(self.hist_filtro.clone()), Message::HistFiltro).text_size(13).width(260),
            button(text("Só eu").size(12))
                .padding([4, 10])
                .style(button::secondary)
                .on_press_maybe((self.hist_filtro != so_eu).then_some(Message::HistFiltro(so_eu))),
            button(text("Atualizar").size(12)).padding([4, 10]).style(button::secondary).on_press(Message::HistAtualizar),
            space::horizontal(),
        ]
        .spacing(8)
        .align_y(Center);
        topo = topo.push(
            text(if self.hist_carregando { "Carregando...".to_string() } else { format!("{} registros", self.historico.len()) })
                .size(12)
                .style(text::secondary),
        );

        let mut col = column![topo].spacing(8);
        if let Some(e) = &self.hist_erro {
            col = col.push(text(format!("Não consegui ler o histórico: {e}")).size(13).style(text::danger));
        }
        let cab = |t: &'static str| text(t).size(12).font(NEGRITO).style(text::secondary);
        col = col.push(
            row![cab("Data e hora").width(150), cab("Usuário").width(150), cab("Computador").width(200), cab("Ação").width(Fill)]
                .spacing(10)
                .padding([0, 8]),
        );
        let produtos = self.analise.as_ref().map(|a| &a.produtos);
        let mut linhas = column![].spacing(2);
        for r in &self.historico {
            let computador = match (&r.maquina, &r.ip) {
                (Some(m), Some(ip)) => format!("{m} ({ip})"),
                (Some(m), None) => m.clone(),
                (None, Some(ip)) => ip.clone(),
                (None, None) => "—".into(),
            };
            linhas = linhas.push(
                container(
                    row![
                        text(historico::hora_local(&r.em)).size(12).width(150),
                        text(r.usuario.clone().unwrap_or_else(|| "—".into())).size(12).width(150),
                        text(computador).size(12).width(200),
                        text(historico::descrever(r, produtos)).size(12).width(Fill),
                    ]
                    .spacing(10),
                )
                .padding([4, 8])
                .style(container::rounded_box),
            );
        }
        if self.historico.is_empty() && !self.hist_carregando && self.hist_erro.is_none() {
            linhas = linhas.push(text("Nenhuma ação registrada.").size(13).style(text::secondary));
        }
        if !self.hist_fim && !self.historico.is_empty() {
            linhas = linhas.push(
                button(text("Carregar mais").size(12))
                    .padding([4, 12])
                    .style(button::secondary)
                    .on_press_maybe((!self.hist_carregando).then_some(Message::HistMais)),
            );
        }
        col.push(scrollable(linhas.padding(iced::Padding::ZERO.right(FOLGA_ROLAGEM))).height(Fill)).into()
    }

    fn view_config_ativos(&self) -> Element<'_, Message> {
        let mut col = column![
            row![
                text(format!("{} panorama(s) aberto(s) agora", self.ativos.len())).size(15).font(NEGRITO),
                space::horizontal(),
                button(text("Atualizar").size(12)).padding([4, 10]).style(button::secondary).on_press(Message::AtivosAtualizar),
            ]
            .align_y(Center),
            text("A lista se atualiza sozinha quando alguém abre ou fecha o programa.").size(12).style(text::secondary),
        ]
        .spacing(8);
        if let Some(e) = &self.ativos_erro {
            col = col.push(text(format!("Não consegui ler os usuários ativos: {e}")).size(13).style(text::danger));
        }
        let cab = |t: &'static str| text(t).size(12).font(NEGRITO).style(text::secondary);
        col = col.push(
            row![cab("Usuário").width(200), cab("Computador").width(200), cab("IP").width(150), cab("Aberto desde").width(Fill)]
                .spacing(10)
                .padding([0, 8]),
        );
        let eu = api::cliente_id();
        let mut linhas = column![].spacing(2);
        for c in &self.ativos {
            let mut usuario = c.usuario.clone().unwrap_or_else(|| "—".into());
            if c.cliente_id.as_deref() == Some(eu) {
                usuario.push_str("  (este programa)");
            }
            linhas = linhas.push(
                container(
                    row![
                        text(usuario).size(13).width(200),
                        text(c.maquina.clone().unwrap_or_else(|| "—".into())).size(13).width(200),
                        text(c.ip.clone().unwrap_or_else(|| "—".into())).size(13).width(150),
                        text(historico::hora_local(&c.conectado_em)).size(13).width(Fill),
                    ]
                    .spacing(10),
                )
                .padding([6, 8])
                .style(container::rounded_box),
            );
        }
        col.push(scrollable(linhas.padding(iced::Padding::ZERO.right(FOLGA_ROLAGEM))).height(Fill)).into()
    }

    fn view_config_geral<'a>(&'a self) -> Element<'a, Message> {
        let digitado = config::normalizar_url(&self.cfg_url);
        let mudou = digitado.as_ref().is_ok_and(|u| *u != config::atual().servidor_url);
        let conexao = if self.conectado {
            text(format!("Conectado a {}", self.servidor)).size(13).style(text::success)
        } else {
            text(format!("Sem conexão com {}", self.servidor)).size(13).style(text::danger)
        };

        let mut servidor = column![
            text("Servidor").size(18).font(NEGRITO),
            text("Endereço do biplas_server_claude, sem o /api. Ex.: http://192.168.0.10:3000").size(13).style(text::secondary),
            row![
                text_input(&format!("Vazio = {}", config::url_padrao()), &self.cfg_url)
                    .on_input(Message::ConfigUrl)
                    .on_submit(Message::ConfigSalvar)
                    .width(420),
                button("Salvar").style(button::primary).on_press_maybe(mudou.then_some(Message::ConfigSalvar)),
                button("Usar padrão")
                    .style(button::secondary)
                    .on_press_maybe(config::atual().servidor_url.is_some().then_some(Message::ConfigPadrao)),
            ]
            .spacing(8)
            .align_y(Center),
            conexao,
        ]
        .spacing(8);
        if let Err(e) = &digitado {
            servidor = servidor.push(text(format!("Endereço inválido: {e}")).size(13).style(text::warning));
        }
        if let Some((erro, msg)) = &self.cfg_status {
            let estilo: EstiloTexto = if *erro { text::danger } else { text::success };
            servidor = servidor.push(text(msg.as_str()).size(13).style(estilo));
        }

        let papel = match &self.conta {
            Some(c) if c.admin => "Administrador: abre todas as telas e autoriza as contas em Configurações → Permissões.".to_string(),
            Some(c) => {
                let telas: Vec<&str> = c.telas.iter().map(|t| conta::nome_da_tela(t)).collect();
                if telas.is_empty() {
                    "Nenhuma tela liberada ainda: peça ao administrador.".to_string()
                } else {
                    format!("Telas liberadas: {}.", telas.join(", "))
                }
            }
            None => String::new(),
        };
        let usuario = column![
            text("Conta").size(18).font(NEGRITO),
            row![
                text(format!("Conectado como {}", self.usuario)).size(15),
                space::horizontal(),
                button("Sair da conta").style(button::secondary).on_press(Message::Sair),
            ]
            .align_y(Center),
            text(papel).size(13).style(text::secondary),
            text("É o nome que aparece no histórico e em Usuários ativos, e cada conta tem a sua configuração de colunas.")
                .size(12)
                .style(text::secondary),
            text(format!(
                "Computador: {}  ·  IP: {}",
                config::maquina(),
                self.ip_local.as_deref().unwrap_or("não identificado"),
            ))
            .size(13),
        ]
        .spacing(8);

        let rodape = text(format!("Configurações deste computador, guardadas em {}", config::caminho().display()))
            .size(12)
            .style(text::secondary);

        let cartao = |c: Column<'a, Message>| container(c).padding(16).style(container::rounded_box);
        scrollable(column![cartao(usuario), cartao(servidor), rodape].spacing(16).max_width(900)).into()
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
        let busca = Busca::nova(&self.busca_nec);
        let nota = |n: &Necessidade| {
            let codigo = n.cod_insumo.to_string();
            let campos = [n.descricao.as_str(), &n.codigo_proprio, &codigo, &n.categoria];
            let propria = busca.nota(campos.into_iter().chain(n.tags.iter().map(String::as_str)));
            // Com os locais juntados, acha também pelo código ou descrição das cópias.
            let copias = self
                .juntar_locais_nec
                .then(|| {
                    self.locais.codigos_de(n.cod_insumo).skip(1).filter_map(|c| a.produtos.get(&c)).filter_map(|p| {
                        let codigo = p.codigo.to_string();
                        busca.nota([p.descricao.as_str(), &p.codigo_proprio, &codigo])
                    })
                })
                .into_iter()
                .flatten();
            propria.into_iter().chain(copias).max()
        };
        let mut linhas: Vec<(i64, &'a Necessidade)> = self
            .simulado
            .linhas
            .iter()
            .filter(|n| !n.mao_de_obra)
            .filter(|n| !self.so_faltas || n.falta() > 0.0)
            .filter(|n| self.filtro_nec.aceita(&n.codigo_proprio, &n.categoria, &n.tags))
            .filter_map(|n| Some((nota(n)?, n)))
            .collect();
        // Primeiro a ordem escolhida; com texto na busca, os mais parecidos sobem (a
        // ordenação é estável, então a escolhida desempata notas iguais).
        linhas.sort_by(|(_, x), (_, y)| self.ordem_nec.comparar(x, y));
        linhas.sort_by(|(x, _), (y, _)| y.cmp(x));
        let linhas: Vec<&'a Necessidade> = linhas.into_iter().map(|(_, n)| n).collect();
        let custo_total: f64 = linhas.iter().map(|n| n.custo_falta()).sum();

        let filtros = row![
            text_input("Buscar material, código, categoria ou tag (busca aproximada)...", &self.busca_nec)
                .on_input(Message::BuscaNec)
                .width(420),
            space::horizontal(),
            text(format!("{} materiais  ·  custo das faltas listadas: {}", linhas.len(), fmt::moeda(custo_total))).size(13),
            botao_filtro(Tabela::Necessidades, self.modo_filtro_nec, self.ativos_nec()),
            button(text(if self.modo_simulacoes {
                "Fechar simulações".to_string()
            } else {
                format!("Simulações ({})", self.simulacoes.len())
            })
            .size(13))
            .style(if self.modo_simulacoes { button::primary } else { button::secondary })
            .on_press(Message::ModoSimulacoes(!self.modo_simulacoes)),
            botao_config_colunas(self.modo_config_nec, Message::ModoConfigNec(!self.modo_config_nec)),
        ]
        .spacing(14)
        .align_y(Center);
        let sel = self.material_sel;
        let locais_nec = self.juntar_locais_nec.then_some(&self.locais);
        // O cabeçalho é uma tabela à parte, fora da rolagem, para não descer com a lista. As colunas
        // são todas proporcionais: com a mesma largura disponível, as duas tabelas ficam alinhadas.
        // O espaçamento das células fica dentro delas (`pintar`), para a cor preencher a coluna.
        let titulos: Vec<_> = self.config_nec.visiveis().map(|c| coluna_necessidade(c, self.config_nec.cor_de(c), sel, locais_nec, cab(c.rotulo()).into())).collect();
        let sem_linhas: [&'a Necessidade; 0] = [];
        let cabecalho = container(table::table(titulos, sem_linhas).padding(0).width(Fill))
            .padding(iced::Padding { right: FOLGA_ROLAGEM, ..iced::Padding::ZERO })
            .width(Fill)
            .style(container::rounded_box);
        let colunas: Vec<_> = self.config_nec.visiveis().map(|c| coluna_necessidade(c, self.config_nec.cor_de(c), sel, locais_nec, space().into())).collect();
        let lista = rolagem(table::table(colunas, linhas).padding(0).width(Fill)).height(Fill);
        let tabela = column![cabecalho, lista].width(Fill);
        // Em modo de configuração, o painel fica à direita da tabela (prévia ao vivo).
        let tabela: Element<'a, Message> = if self.modo_config_nec {
            row![container(tabela).width(Fill), view_config_colunas(&self.config_nec, Tabela::Necessidades, self.editor_cor.as_ref())].spacing(12).into()
        } else if self.modo_simulacoes {
            row![container(tabela).width(Fill), self.view_simulacoes(a)].spacing(12).into()
        } else if self.modo_filtro_nec {
            row![container(tabela).width(Fill), self.view_filtro(a, Tabela::Necessidades)].spacing(12).into()
        } else {
            tabela.into()
        };

        let mut tela = column![self.view_grupo_ops(a), filtros].spacing(10);
        match sel.and_then(|c| self.simulado.linhas.iter().find(|n| n.cod_insumo == c)) {
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

    /// Filtros ligados na aba Necessidades (a ordem não conta).
    fn ativos_nec(&self) -> usize {
        self.filtro_nec.ativos() + usize::from(self.so_faltas) + usize::from(self.juntar_locais_nec)
    }

    fn ativos_prod(&self) -> usize {
        self.filtro_prod.ativos()
            + usize::from(self.so_usados)
            + usize::from(self.so_negativos)
            + usize::from(self.juntar_locais)
    }

    /// Painel lateral "Filtro de busca" das abas Necessidades e Produtos.
    fn view_filtro<'a>(&'a self, a: &'a Analise, tabela: Tabela) -> Element<'a, Message> {
        let (f, ativos) = match tabela {
            Tabela::Necessidades => (&self.filtro_nec, self.ativos_nec()),
            Tabela::Produtos => (&self.filtro_prod, self.ativos_prod()),
        };
        let acao = move |acao: AcaoFiltro| Message::Filtro(tabela, acao);

        // Categorias e tags que existem na própria tabela.
        let (categorias, tags): (BTreeSet<String>, BTreeSet<String>) = match tabela {
            Tabela::Necessidades => {
                let linhas = self.simulado.linhas.iter().filter(|n| !n.mao_de_obra);
                (
                    linhas.clone().map(|n| n.categoria.clone()).collect(),
                    linhas.flat_map(|n| n.tags.iter().map(|t| t.trim().to_string())).collect(),
                )
            }
            Tabela::Produtos => (
                a.produtos.values().map(|p| a.categoria_de(p.cod_categoria).to_string()).collect(),
                a.produtos.values().flat_map(|p| p.tags.iter().map(|t| t.trim().to_string())).collect(),
            ),
        };
        let tags_livres: Vec<String> =
            tags.into_iter().filter(|t| !t.is_empty() && !f.tags.iter().any(|x| x.eq_ignore_ascii_case(t))).collect();

        let categoria = pick_list(filtro::opcoes(categorias), Some(Opcao(f.categoria.clone())), move |o: Opcao<String>| {
            acao(AcaoFiltro::Categoria(o.0))
        })
        .text_size(13)
        .width(Fill);
        let tipo = pick_list(filtro::opcoes(TipoUso::TODOS), Some(Opcao(f.tipo)), move |o: Opcao<TipoUso>| {
            acao(AcaoFiltro::Tipo(o.0))
        })
        .text_size(13)
        .width(Fill);
        let prefixo = text_input("Ex.: 21", &f.prefixo)
            .on_input(move |s| acao(AcaoFiltro::Prefixo(s)))
            .size(13)
            .width(Fill);
        let escolhidas = iced::widget::Row::with_children(f.tags.iter().map(|t| {
            button(text(format!("{t}  ×")).size(12))
                .padding([2, 8])
                .style(button::secondary)
                .on_press(acao(AcaoFiltro::RemoverTag(t.clone())))
                .into()
        }))
        .spacing(6)
        .wrap();
        let nova_tag = pick_list(tags_livres, None::<String>, move |t| acao(AcaoFiltro::AdicionarTag(t)))
            .placeholder("Adicionar tag...")
            .text_size(13)
            .width(Fill);

        let mut col = column![
            text("Filtro de busca").size(15).font(NEGRITO),
            text("Vale junto com a barra de busca. Itens precisam atender a todos os campos preenchidos.")
                .size(12)
                .style(text::secondary),
            campo_filtro("Categoria", categoria),
            campo_filtro("Código próprio começa com (2 primeiros dígitos)", prefixo),
            campo_filtro("Tipo de uso (pelo 1º dígito do código próprio)", tipo),
            campo_filtro("Tags (palavras-chave) — precisa ter todas", column![nova_tag, escolhidas].spacing(6)),
            rule::horizontal(1),
        ]
        .spacing(10);

        col = match tabela {
            Tabela::Necessidades => col.push(column![
                campo_filtro("Ordenar", pick_list(OrdemNec::TODAS, Some(self.ordem_nec), Message::OrdemNec).text_size(13).width(Fill)),
                toggler(self.so_faltas).label("Somente com falta").on_toggle(Message::SoFaltas),
                toggler(self.juntar_locais_nec).label("Juntar locais (almoxarifado...)").on_toggle(Message::JuntarLocaisNec),
            ]
            .spacing(10)),
            Tabela::Produtos => col.push(column![
                toggler(self.so_usados).label("Somente itens usados nas OPs em aberto").on_toggle(Message::SoUsados),
                toggler(self.so_negativos).label("Somente com estoque negativo").on_toggle(Message::SoNegativos),
                toggler(self.juntar_locais).label("Juntar locais (almoxarifado...)").on_toggle(Message::JuntarLocais),
            ]
            .spacing(10)),
        };
        col = col.push(rule::horizontal(1));
        col = col.push(
            button(text("Limpar filtros").size(13))
                .style(button::secondary)
                .on_press_maybe((ativos > 0).then_some(Message::LimparFiltros(tabela))),
        );

        container(scrollable(col.padding(iced::Padding { right: 14.0, ..iced::Padding::ZERO })).height(Fill))
            .padding(12)
            .width(360)
            .height(Fill)
            .style(container::bordered_box)
            .into()
    }

    /// Painel lateral: criar, ver e excluir simulações.
    fn view_simulacoes<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let qtd = fmt::ler_qtd(&self.qtd_sim);
        let mut col = column![
            text("Simulações").size(15).font(NEGRITO),
            text(
                "Produto e quantidade que ainda não viraram OP. Subprodutos usam primeiro o estoque que sobra depois \
                 das OPs; só a parte que faltar desce até a matéria-prima. Salvo automaticamente."
            )
            .size(12)
            .style(text::secondary),
            row![
                text_input("Buscar item por código ou nome...", &self.busca_sim)
                    .on_input(Message::SimBusca)
                    .size(13)
                    .width(Fill),
                text_input("Qtd.", &self.qtd_sim)
                    .on_input(Message::SimQtd)
                    .on_submit(Message::SimCriar)
                    .size(13)
                    .width(90),
            ]
            .spacing(6),
        ]
        .spacing(8);

        if a.composicoes.is_none() {
            col = col.push(text(AVISO_SEM_COMPOSICOES).size(12).style(text::warning));
        }

        if self.sim_item.is_none() && !self.busca_sim.trim().is_empty() {
            let itens = simulacao::buscar_itens(a, &self.busca_sim, MAX_RESULTADOS_BUSCA_SIM);
            if itens.is_empty() && a.composicoes.is_some() {
                col = col.push(text("Nenhum item com composição encontrado.").size(12).style(text::secondary));
            }
            for p in itens {
                col = col.push(
                    button(
                        column![
                            text(p.descricao.as_str()).size(12),
                            text(format!("Cód. {}  ·  {}", p.codigo, p.codigo_proprio)).size(11).style(text::secondary),
                        ]
                        .spacing(1),
                    )
                    .width(Fill)
                    .padding([4, 8])
                    .style(cartao)
                    .on_press(Message::SimEscolher(p.codigo)),
                );
            }
        }

        if !self.qtd_sim.trim().is_empty() && qtd.is_none() {
            col = col.push(text("Quantidade inválida.").size(12).style(text::danger));
        }
        col = col.push(
            button(text("Criar simulação").size(13))
                .style(button::primary)
                .on_press_maybe((self.sim_item.is_some() && qtd.is_some()).then_some(Message::SimCriar)),
        );
        col = col.push(rule::horizontal(1));

        let mut lista = column![].spacing(8);
        if self.simulacoes.is_empty() {
            lista = lista.push(text("Nenhuma simulação criada.").size(12).style(text::secondary));
        }
        for s in &self.simulacoes {
            lista = lista.push(self.view_simulacao(a, s));
        }
        col = col.push(scrollable(lista.padding(iced::Padding { right: 14.0, ..iced::Padding::ZERO })).height(Fill));

        container(col).padding(12).width(460).height(Fill).style(container::bordered_box).into()
    }

    /// Uma simulação da lista, com o que ela consome quando aberta.
    fn view_simulacao<'a>(&'a self, a: &'a Analise, s: &'a Simulacao) -> Element<'a, Message> {
        let produto = a.produtos.get(&s.cod_produto);
        let tem_composicao = a.composicoes.as_ref().is_some_and(|m| m.contains_key(&s.cod_produto));
        let consumos = self.simulado.por_simulacao.get(&s.id).map_or(&[][..], Vec::as_slice);
        let aberta = self.sim_aberta == Some(s.id);

        let mut col = column![
            text(produto.map_or_else(|| format!("Item {} (fora do cadastro)", s.cod_produto), |p| p.descricao.clone()))
                .size(13)
                .font(NEGRITO),
            row![
                text(format!(
                    "{} {}  ·  {} itens",
                    fmt::qtd(s.quantidade),
                    produto.map_or("", |p| p.unidade.as_str()),
                    consumos.len()
                ))
                .size(12)
                .style(text::secondary),
                space::horizontal(),
                button(text(if aberta { "Ocultar" } else { "Ver consumo" }).size(12))
                    .padding([2, 8])
                    .style(button::secondary)
                    .on_press(Message::SimVer(s.id)),
                button(text("Excluir").size(12)).padding([2, 8]).style(button::danger).on_press(Message::SimExcluir(s.id)),
            ]
            .spacing(6)
            .align_y(Center),
        ]
        .spacing(4);

        if !tem_composicao {
            col = col.push(text("Sem composição cadastrada: não entra no cálculo.").size(12).style(text::danger));
        }
        if aberta {
            for c in consumos {
                let detalhe = match (c.composto, c.fabricar > 0.0) {
                    (true, true) => format!("subproduto: fabricar {}", fmt::qtd(c.fabricar)),
                    (true, false) => "subproduto: sai do estoque".to_string(),
                    (false, _) => String::new(),
                };
                let mut nome = column![text(c.descricao.as_str()).size(12)];
                if !detalhe.is_empty() {
                    nome = nome.push(text(detalhe).size(11).style(text::secondary));
                }
                col = col.push(
                    row![
                        nome.width(Fill),
                        text(format!("{} {}", fmt::num(c.qtd, 3), c.unidade)).size(12),
                    ]
                    .spacing(8),
                );
            }
        }
        container(col).padding(8).width(Fill).style(container::rounded_box).into()
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
                    "Necessidade {} {}  ·  estoque {}{}  ·  saldo {}  ·  {} OPs  ·  {} produtos{}",
                    fmt::qtd(n.necessidade),
                    n.unidade,
                    fmt::qtd(n.estoque),
                    if n.consumo_parcial > 0.0 {
                        format!(
                            "  ·  consumo parcial {}  ·  estoque físico real {}",
                            fmt::qtd(n.consumo_parcial),
                            fmt::qtd(n.estoque_fisico_real())
                        )
                    } else {
                        String::new()
                    },
                    fmt::qtd(n.saldo()),
                    n.ops.len(),
                    n.produtos.len(),
                    if n.simulada > 0.0 {
                        format!(
                            "  ·  em simulação {}  ·  saldo após OPs e simulação {}",
                            fmt::qtd(n.simulada),
                            fmt::qtd(n.saldo_apos_ops_e_simulacao())
                        )
                    } else {
                        String::new()
                    }
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
        // Uma situação ou máquina digitada por inteiro filtra por ela; o resto é busca de texto.
        let filtro = trello::FiltroSituacao::da_busca(&self.busca_op);
        let busca = trello::normalizar(&self.busca_op);
        let ops_com_falta = a.res.ops_com_falta();

        // Começa do nº mais novo; as outras ordens são estáveis, então o nº desempata.
        let mut ops: Vec<&'a Op> = a.ops.iter().rev().collect();
        let posicao = |o: &Op| trello::posicao_por_situacao(self.situacoes(o.codigo));
        match self.ordem_op {
            OrdemOp::Numero => {}
            // Dentro de "em máquina", da MAQUINA 1 à 11.
            OrdemOp::Situacao => ops.sort_by_key(|o| (posicao(o), trello::chave_maquina(self.situacoes(o.codigo)))),
            // Primeiro as que estão em máquina, pela máquina; depois as demais, pela situação.
            OrdemOp::Maquina => ops.sort_by_key(|o| {
                let maquina = trello::chave_maquina(self.situacoes(o.codigo));
                (maquina.is_none(), maquina, posicao(o))
            }),
        }

        let mut lista = column![].spacing(6);
        let mut n = 0;
        for o in ops {
            let texto_busca = format!(
                "{} {} {} {} {}",
                o.codigo,
                o.tags.join(" "),
                o.itens.iter().map(|i| i.descricao.as_str()).collect::<Vec<_>>().join(" "),
                self.situacoes(o.codigo).iter().map(Situacao::texto).collect::<Vec<_>>().join(" "),
                trello::etiquetas(self.situacoes(o.codigo)).iter().map(|e| e.nome.as_str()).collect::<Vec<_>>().join(" ")
            );
            let passa = match filtro {
                Some(f) => f.aceita(self.situacoes(o.codigo)),
                None => trello::normalizar(&texto_busca).contains(&busca),
            };
            if !passa {
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
            let etiquetas = trello::etiquetas(self.situacoes(o.codigo));
            let linha_etiquetas: Element<'a, Message> = if etiquetas.is_empty() {
                space::vertical().height(0).into()
            } else {
                row(etiquetas.into_iter().map(|e| selo_etiqueta(e, 10))).spacing(4).wrap().into()
            };
            let estilo: EstiloBotao = if self.op_sel == Some(o.codigo) { cartao_selecionado } else { cartao };
            lista = lista.push(
                button(
                    column![
                        row![text(format!("OP {}", o.codigo)).size(14).font(NEGRITO)]
                            .extend(self.situacoes(o.codigo).iter().map(selo_trello))
                            .push(space::horizontal())
                            .push(text(format!("início {}", fmt::data(&o.dt_inicio))).size(11))
                            .spacing(6)
                            .align_y(Center),
                        linha_etiquetas,
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
            text_input("Buscar OP, produto, tag, máquina ou situação...", &self.busca_op).on_input(Message::BuscaOp),
            toggler(self.so_ops_falta).label("Somente OPs com falta de material").on_toggle(Message::SoOpsComFalta),
            pick_list(OrdemOp::TODAS, Some(self.ordem_op), Message::OrdemOp).text_size(13).width(Fill),
            text(match filtro {
                Some(f) => format!("{n} OPs  ·  filtrando: {}", f.rotulo()),
                None => format!("{n} OPs"),
            })
            .size(12)
            .style(if filtro.is_some() { text::primary } else { text::secondary }),
            self.view_status_trello(a),
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

    /// Linha da lista de OPs que diz de quando é a situação do Trello, com o botão de atualizar.
    fn view_status_trello<'a>(&'a self, a: &'a Analise) -> Element<'a, Message> {
        let com_cartao = a.ops.iter().filter(|o| self.trello.contains_key(&o.codigo)).count();
        let (msg, estilo): (String, EstiloTexto) = match (&self.status_trello, self.buscando_trello) {
            (None, _) => ("Trello: buscando a situação das OPs...".into(), text::secondary),
            (Some(Ok(hora)), buscando) => (
                format!(
                    "Trello: {com_cartao} de {} OPs com cartão · lido às {hora}{}",
                    a.ops.len(),
                    if buscando { " · atualizando..." } else { "" }
                ),
                text::secondary,
            ),
            (Some(Err(e)), _) => (
                format!(
                    "Trello indisponível{}: {e}",
                    if self.trello.is_empty() { "" } else { " (mostrando a última leitura)" }
                ),
                text::danger,
            ),
        };
        row![
            text(msg).size(11).style(estilo).width(Fill),
            button(text(if self.buscando_trello { "Buscando..." } else { "Atualizar Trello" }).size(11))
                .padding([2, 8])
                .style(button::secondary)
                .on_press_maybe((!self.buscando_trello).then_some(Message::AtualizarTrello)),
        ]
        .spacing(8)
        .align_y(Center)
        .into()
    }

    /// Etiquetas dos cartões da OP e o botão de abrir cada cartão no Trello.
    fn view_trello_op<'a>(&'a self, op: i64) -> Element<'a, Message> {
        let situacoes = self.situacoes(op);
        let mut r = row(trello::etiquetas(situacoes).into_iter().map(|e| selo_etiqueta(e, 12))).spacing(6).align_y(Center);
        let mut urls: Vec<&Situacao> = Vec::new();
        for s in situacoes.iter().filter(|s| !s.url.is_empty()) {
            if !urls.iter().any(|u| u.url == s.url) {
                urls.push(s);
            }
        }
        r = r.push(space::horizontal());
        for s in &urls {
            let rotulo = if urls.len() > 1 { format!("Abrir no Trello ({})", s.texto()) } else { "Abrir no Trello".into() };
            r = r.push(
                button(text(rotulo).size(12)).padding([3, 10]).style(button::secondary).on_press(Message::AbrirLink(s.url.clone())),
            );
        }
        r.into()
    }

    fn view_detalhe_op<'a>(&'a self, a: &'a Analise, o: &'a Op) -> Element<'a, Message> {
        let no_grupo = self.grupo_ops.contains(&o.codigo);
        let estilo_grupo: EstiloBotao = if no_grupo { button::secondary } else { button::primary };
        let cab_op = column![
            row![text(format!("OP {}", o.codigo)).size(22).font(NEGRITO)]
                .extend(self.situacoes(o.codigo).iter().map(selo_trello))
                .push(if self.trello.contains_key(&o.codigo) || self.status_trello.is_none() {
                    text("")
                } else {
                    text("sem cartão no Trello").size(12).style(text::secondary)
                })
                .push(space::horizontal())
                .push(
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
                )
                .spacing(10)
                .align_y(Center),
            self.view_trello_op(o.codigo),
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
        for (linha, item) in o.itens.iter().enumerate() {
            col = col.push(self.view_item_op(a, (o.codigo, linha, item.cod_produto), item));
        }
        scrollable(col.padding([0, 12])).height(Fill).into()
    }

    /// Realizado parcial da linha: um ou mais tipos de volume (volumes × quantidade por
    /// volume), somados e gravados só ao confirmar. Embaixo, produzido × pedido e cada
    /// confirmação com quanto entrou.
    fn view_realizado<'a>(&'a self, chave: parcial::Chave, quant: f64) -> Element<'a, Message> {
        let salvo = self.lancamento(chave);
        let editado = self.campos_vol.get(&chave);
        let campos = editado.cloned().unwrap_or_else(|| Campos::do_lancamento(salvo));
        let lido = campos.ler();
        let grupos_salvos = salvo.map(Lancamento::grupos).unwrap_or_default();
        // Confirmar só quando o digitado é válido e muda o que está salvo.
        let muda = match &lido {
            Ok(g) if g.is_empty() => salvo.is_some(),
            Ok(g) => *g != grupos_salvos,
            Err(_) => false,
        };
        let confirmar = (editado.is_some() && muda).then_some(Message::ConfirmarRealizado(chave));

        let varios = campos.grupos.len() > 1;
        let mut tipos = column![].spacing(4);
        for (i, c) in campos.grupos.iter().enumerate() {
            let subtotal = match (fmt::ler_qtd(&c.volumes), fmt::ler_qtd(&c.qtd_volume)) {
                (Some(v), Some(q)) => format!("= {}", fmt::qtd(v * q)),
                _ => String::new(),
            };
            tipos = tipos.push(
                row![
                    text(format!("Realizado parcial ({})", i + 1)).size(12).width(130),
                    text_input("0", &c.volumes)
                        .on_input(move |s| Message::VolTotal(chave, i, s))
                        .on_submit_maybe(confirmar.clone())
                        .size(12)
                        .padding([4, 8])
                        .width(80),
                    text("volumes ×").size(12).style(text::secondary),
                    text_input("0", &c.qtd_volume)
                        .on_input(move |s| Message::VolQtd(chave, i, s))
                        .on_submit_maybe(confirmar.clone())
                        .size(12)
                        .padding([4, 8])
                        .width(90),
                    text("pç/volume").size(12).style(text::secondary),
                    text(subtotal).size(12).width(90),
                    button(text("Remover").size(11))
                        .padding([2, 8])
                        .style(button::danger)
                        .on_press_maybe(varios.then_some(Message::VolRemover(chave, i))),
                ]
                .spacing(8)
                .align_y(Center),
            );
        }

        let resumo = match &lido {
            Ok(g) if g.is_empty() && salvo.is_some() => text("REALIZADO PARCIAL = 0 (apaga o lançamento)").size(12).style(text::warning),
            Ok(g) if g.is_empty() => text("REALIZADO PARCIAL = 0").size(12).style(text::secondary),
            Ok(g) => text(format!("REALIZADO PARCIAL = {} ({})", fmt::qtd(parcial::total(g)), parcial::composicao(g))).size(12).font(NEGRITO),
            Err(e) => text(e.clone()).size(12).style(text::warning),
        };
        let acoes = row![
            button(text("+ Adicionar tipo de volume").size(11))
                .padding([2, 8])
                .style(button::secondary)
                .on_press(Message::VolAdicionar(chave)),
            resumo,
            space::horizontal(),
            button(text("Confirmar").size(12)).padding([3, 12]).style(button::primary).on_press_maybe(confirmar),
        ]
        .spacing(10)
        .align_y(Center);

        let produzido = salvo.map_or(0.0, |l| l.realizado);
        let pct = if quant > 0.0 { produzido / quant * 100.0 } else { 0.0 };
        let (resto, estilo): (String, EstiloTexto) = if produzido > quant {
            (format!("{} pç acima do pedido", fmt::qtd(produzido - quant)), text::warning)
        } else if produzido == quant && quant > 0.0 {
            ("pedido completo".into(), text::success)
        } else {
            (format!("faltam {} pç", fmt::qtd(quant - produzido)), text::secondary)
        };
        let composicao_salva =
            if grupos_salvos.is_empty() { String::new() } else { format!(" ({})", parcial::composicao(&grupos_salvos)) };
        let total = row![
            text(format!("Produzido {}{composicao_salva} de {} pç pedidas", fmt::qtd(produzido), fmt::qtd(quant)))
                .size(12)
                .font(NEGRITO),
            text(format!("{}%", fmt::num(pct, 1))).size(12),
            text(resto).size(12).style(estilo),
        ]
        .spacing(10)
        .align_y(Center);

        let mut col = column![tipos, acoes, total].spacing(6);
        let (op, linha, cod_produto) = chave;
        if let Some((op_conf, lista)) = &self.confirmacoes
            && *op_conf == op
        {
            let mut anterior: Option<&Confirmacao> = None;
            for c in lista.iter().filter(|c| c.linha == linha && c.cod_produto == cod_produto) {
                let quem = c.usuario.as_deref().map(|u| format!("  ·  {u}")).unwrap_or_default();
                col = col.push(
                    text(format!(
                        "{}   {}   → total {}{quem}",
                        historico::hora_local(&c.em),
                        c.delta(anterior),
                        fmt::qtd(c.realizado)
                    ))
                    .size(11)
                    .style(text::secondary),
                );
                anterior = Some(c);
            }
        }
        col.into()
    }

    fn view_item_op<'a>(&'a self, a: &'a Analise, chave: parcial::Chave, item: &'a ItemOp) -> Element<'a, Message> {
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

        container(
            column![titulo, self.view_realizado(chave, quant), table::table(colunas, linhas).padding_x(8).padding_y(3).width(Fill)]
                .spacing(8),
        )
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
        // Com texto na busca, os mais parecidos vêm primeiro. O código desempata descrições
        // iguais, para cada item ficar sempre na mesma página.
        todos.sort_by(|(nx, x), (ny, y)| ny.cmp(nx).then(x.descricao.cmp(&y.descricao)).then(x.codigo.cmp(&y.codigo)));
        let pag = Paginacao::de(todos.len(), self.pagina_prod);
        let linhas: Vec<&Produto> = todos[pag.inicio..pag.fim].iter().map(|(_, p)| *p).collect();

        let filtros = row![
            text_input("Buscar produto, código, categoria ou tag (busca aproximada)...", &self.busca_prod)
                .on_input(Message::BuscaProd)
                .width(420),
            space::horizontal(),
            text(format!("{} produtos", pag.total)).size(13),
            botao_filtro(Tabela::Produtos, self.modo_filtro_prod, self.ativos_prod()),
            botao_config_colunas(self.modo_config_prod, Message::ModoConfigProd(!self.modo_config_prod)),
        ]
        .spacing(14)
        .align_y(Center);

        let sel = self.produto_sel;
        let locais = self.locais_juntados();
        let coluna = |c: ColunaProd, cabecalho: Element<'a, Message>| {
            coluna_produto(c, self.config_prod.cor_de(c), a, sel, &self.produzido_parcial, locais, cabecalho)
        };
        // Como na aba Necessidades: o cabeçalho é uma tabela à parte, fora da rolagem, para não
        // descer com a lista. Com as mesmas colunas proporcionais, as duas tabelas ficam alinhadas.
        let titulos: Vec<_> = self.config_prod.visiveis().map(|c| coluna(c, cab(c.rotulo()).into())).collect();
        let sem_linhas: [&'a Produto; 0] = [];
        let cabecalho = container(table::table(titulos, sem_linhas).padding(0).width(Fill))
            .padding(iced::Padding { right: FOLGA_ROLAGEM, ..iced::Padding::ZERO })
            .width(Fill)
            .style(container::rounded_box);
        let colunas: Vec<_> = self.config_prod.visiveis().map(|c| coluna(c, space().into())).collect();
        let mut lista = column![table::table(colunas, linhas).padding(0).width(Fill)].spacing(8);
        if pag.ultima > 0 {
            lista = lista.push(view_paginacao(pag));
        }
        let mut tabela = column![].spacing(8).width(Fill);
        if pag.ultima > 0 {
            tabela = tabela.push(view_paginacao(pag));
        }
        let tabela = tabela.push(column![cabecalho, rolagem(lista).id(ID_LISTA_PRODUTOS).height(Fill)].width(Fill));
        // Em modo de configuração, o painel fica à direita da tabela (prévia ao vivo).
        let tabela: Element<'a, Message> = if self.modo_config_prod {
            row![container(tabela).width(Fill), view_config_colunas(&self.config_prod, Tabela::Produtos, self.editor_cor.as_ref())].spacing(12).into()
        } else if self.modo_filtro_prod {
            row![container(tabela).width(Fill), self.view_filtro(a, Tabela::Produtos)].spacing(12).into()
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

/// Botão que abre o painel "Filtro de busca", com quantos filtros estão ligados.
fn botao_filtro<'a>(tabela: Tabela, aberto: bool, ativos: usize) -> Element<'a, Message> {
    let rotulo = match ativos {
        0 => "Filtros".to_string(),
        n => format!("Filtros ({n})"),
    };
    let estilo: EstiloBotao = if aberto || ativos > 0 { button::primary } else { button::secondary };
    button(text(if aberto { "Fechar filtros".to_string() } else { rotulo }).size(13))
        .style(estilo)
        .on_press(Message::ModoFiltro(tabela, !aberto))
        .into()
}

/// Linha do painel de filtro: rótulo em cima, campo embaixo.
fn campo_filtro<'a>(rotulo: &'a str, campo: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(rotulo).size(12).style(text::secondary), campo.into()].spacing(4).into()
}

fn botao_config_colunas<'a>(ativo: bool, ao_clicar: Message) -> Element<'a, Message> {
    let estilo: EstiloBotao = if ativo { button::primary } else { button::secondary };
    button(text(if ativo { "Concluir configuração" } else { "Configurar colunas" }).size(13))
        .style(estilo)
        .on_press(ao_clicar)
        .into()
}

/// Painel do modo de configuração: marcar/desmarcar colunas e mudar a ordem.
fn view_config_colunas<'a, C: Coluna>(
    cfg: &'a ConfigColunas<C>,
    tabela: Tabela,
    editor: Option<&'a EditorCor>,
) -> Element<'a, Message> {
    let acao = move |a: AcaoColuna| Message::Colunas(tabela, a);
    let total = cfg.colunas.len();
    let editor = editor.filter(|e| e.tabela == tabela);
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
        let aberto = editor.is_some_and(|e| e.i == i);
        lista = lista.push(
            container(
                row![
                    text(format!("{}.", i + 1)).size(12).width(24).style(estilo_ordem),
                    marcador,
                    if c.obrigatoria() { text(" (fixa)").size(11).style(text::secondary) } else { text("") },
                    space::horizontal(),
                    amostra_cor(cfg.cor_de(c), aberto, Message::CorAbrir(tabela, i)),
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
        if let Some(e) = editor.filter(|_| aberto) {
            lista = lista.push(view_editor_cor(e, cfg.cor_de(c)));
        }
    }
    let n_vis = cfg.visiveis().count();
    container(
        column![
            text("Configurar colunas").size(15).font(NEGRITO),
            text(format!(
                "{n_vis} de {total} visíveis. Marque as que quer ver e use as setas para mudar a ordem \
                 (⤒ topo · ↑ subir · ↓ descer · ⤓ fim). Clique em \"Aa\" para escolher as cores da coluna. \
                 Salvo automaticamente, só para o seu usuário."
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
    .width(440)
    .height(Fill)
    .style(container::bordered_box)
    .into()
}

/// Botão "Aa" com as cores da coluna (fundo e texto), que abre o seletor.
fn amostra_cor<'a>(cor: CorColuna, aberto: bool, ao_clicar: Message) -> Element<'a, Message> {
    button(text("Aa").size(12).font(NEGRITO))
        .padding([2, 8])
        .style(move |theme: &Theme, status| {
            let p = theme.extended_palette();
            let base = button::secondary(theme, status);
            button::Style {
                background: Some(cor.fundo.map_or(p.background.base.color, iced::Color::from).into()),
                text_color: cor.texto.map_or(p.background.base.text, iced::Color::from),
                border: iced::Border {
                    radius: 4.0.into(),
                    width: if aberto { 2.0 } else { 1.0 },
                    color: if aberto { p.primary.strong.color } else { p.background.strong.color },
                },
                ..base
            }
        })
        .on_press(ao_clicar)
        .into()
}

/// Seletor de cores aberto embaixo da coluna: fundo ou texto, anel, código e botões.
fn view_editor_cor<'a>(e: &'a EditorCor, cor: CorColuna) -> Element<'a, Message> {
    let alvo = |rotulo: &'static str, a: Alvo| {
        button(text(rotulo).size(12))
            .padding([3, 10])
            .style(if e.alvo == a { button::primary } else { button::secondary })
            .on_press(Message::CorAlvo(a))
    };
    let usa_padrao = cor.de(e.alvo).is_none();
    let exemplo = container(text("Exemplo 1.234").size(13))
        .padding([4, 8])
        .width(Fill)
        .style(move |theme: &Theme| estilo_celula(theme, cor));
    let anel = canvas(Seletor { hsv: e.hsv, ao_mudar: Message::CorMudou }).width(170).height(170);
    let lado = column![
        exemplo,
        text("Código da cor").size(11).style(text::secondary),
        text_input("#RRGGBB", &e.hex).on_input(Message::CorHex).on_submit(Message::CorOk).size(12).padding([4, 8]),
        if usa_padrao {
            text("Usando a cor padrão").size(11).style(text::secondary)
        } else if Rgb::do_hex(&e.hex).is_none() {
            text("Código inválido").size(11).style(text::warning)
        } else {
            text("")
        },
        button(text("Voltar ao padrão").size(12))
            .padding([3, 10])
            .style(button::secondary)
            .on_press_maybe((!usa_padrao).then_some(Message::CorPadrao)),
    ]
    .spacing(6)
    .width(Fill);
    container(
        column![
            row![alvo("Fundo", Alvo::Fundo), alvo("Texto", Alvo::Texto)].spacing(6),
            row![anel, lado].spacing(10),
            row![
                space::horizontal(),
                button(text("Cancelar").size(12)).padding([3, 12]).style(button::secondary).on_press(Message::CorCancelar),
                button(text("OK").size(12)).padding([3, 16]).style(button::primary).on_press(Message::CorOk),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
    .padding(8)
    .style(container::bordered_box)
    .into()
}

/// Fundo e cor de texto escolhidos para a coluna (o que não foi escolhido fica como o tema).
fn estilo_celula(_theme: &Theme, cor: CorColuna) -> container::Style {
    container::Style {
        background: cor.fundo.map(|c| iced::Color::from(c).into()),
        text_color: cor.texto.map(iced::Color::from),
        ..container::Style::default()
    }
}

/// Célula de tabela com as cores da coluna. O espaçamento fica dentro da célula
/// (a tabela fica sem `padding`), para o fundo preencher a coluna toda.
fn pintar<'a>(cor: CorColuna, alinhar: Horizontal, conteudo: Element<'a, Message>) -> Element<'a, Message> {
    container(conteudo).padding([4, 8]).width(Fill).align_x(alinhar).style(move |t: &Theme| estilo_celula(t, cor)).into()
}

/// Coluna de texto com as cores escolhidas.
fn col_cor<'a, 'b, T: 'a, E: Into<Element<'a, Message>>>(
    cor: CorColuna,
    cabecalho: Element<'a, Message>,
    peso: u16,
    f: impl Fn(T) -> E + 'b,
) -> table::Column<'a, 'b, T, Message> {
    table::column(pintar(cor, Left, cabecalho), move |t| pintar(cor, Left, f(t).into())).width(FillPortion(peso))
}

/// Coluna numérica (alinhada à direita) com as cores escolhidas.
fn col_num_cor<'a, 'b, T: 'a, E: Into<Element<'a, Message>>>(
    cor: CorColuna,
    cabecalho: Element<'a, Message>,
    peso: u16,
    f: impl Fn(T) -> E + 'b,
) -> table::Column<'a, 'b, T, Message> {
    table::column(pintar(cor, Right, cabecalho), move |t| pintar(cor, Right, f(t).into())).width(FillPortion(peso))
}

/// Texto sem cor própria: usa a da coluna, se houver, senão a do tema.
fn sem_cor(_: &Theme) -> text::Style {
    text::Style::default()
}

/// Botão de texto (Material, Descrição) que respeita a cor de texto da coluna.
fn botao_da_coluna(cor: CorColuna, selecionado: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selecionado {
            return button::primary(theme, status);
        }
        let mut s = button::text(theme, status);
        if let Some(c) = cor.texto {
            s.text_color = c.into();
        }
        s
    }
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
fn coluna_produto<'a>(
    c: ColunaProd,
    cor: CorColuna,
    a: &'a Analise,
    sel: Option<i64>,
    produzido: &'a HashMap<i64, f64>,
    locais: Option<&'a Locais>,
    titulo: Element<'a, Message>,
) -> table::Column<'a, 'a, &'a Produto, Message> {
    type P<'x> = &'x Produto;
    let produzido_de = move |p: P<'a>| produzido.get(&p.codigo).copied().unwrap_or(0.0);
    // Com os locais juntados, o estoque é a soma de todos os locais.
    let estoque_de = move |p: P<'a>| locais.map_or(p.estoque, |l| l.total(p));
    match c {
        ColunaProd::Codigo => col_num_cor(cor, titulo, 1, |p: P<'a>| cel(p.codigo.to_string())),
        ColunaProd::CodigoProprio => col_cor(cor, titulo, 2, |p: P<'a>| cel(p.codigo_proprio.as_str())),
        ColunaProd::Descricao => col_cor(cor, titulo, 6, move |p: P<'a>| {
            button(text(p.descricao.as_str()).size(13))
                .padding([1, 4])
                .style(botao_da_coluna(cor, sel == Some(p.codigo)))
                .on_press(Message::SelProduto(p.codigo))
        }),
        ColunaProd::Categoria => col_cor(cor, titulo, 2, move |p: P<'a>| cel(a.categoria_de(p.cod_categoria))),
        ColunaProd::Composicao => col_cor(cor, titulo, 2, move |p: P<'a>| match &a.composicoes {
            None => cel("—").style(text::secondary),
            Some(m) if m.contains_key(&p.codigo) => cel("Sim").style(text::success),
            Some(_) => cel("Não").style(text::secondary),
        }),
        ColunaProd::Unidade => col_cor(cor, titulo, 1, |p: P<'a>| cel(p.unidade.as_str())),
        ColunaProd::Estoque => {
            let peso = if locais.is_some() { 3 } else { 2 };
            col_num_cor(cor, titulo, peso, move |p: P<'a>| celula_estoque_locais(locais, p.codigo, p.estoque, estoque_de(p)))
        }
        ColunaProd::ProduzidoParcial => col_num_cor(cor, titulo, 2, move |p: P<'a>| match produzido_de(p) {
            v if v > 0.0 => cel(fmt::qtd(v)),
            _ => cel("—").style(text::secondary),
        }),
        ColunaProd::EstoqueReal => col_num_cor(cor, titulo, 2, move |p: P<'a>| {
            let real = estoque_de(p) + produzido_de(p);
            cel(fmt::qtd(real)).style(if real < 0.0 { text::danger } else { sem_cor })
        }),
        ColunaProd::EstoqueMinimo => col_num_cor(cor, titulo, 2, |p: P<'a>| cel(fmt::qtd(p.estoque_minimo))),
        ColunaProd::PesoLiquido => col_num_cor(cor, titulo, 2, |p: P<'a>| cel(fmt::num(p.peso_liquido, 4))),
        ColunaProd::PrecoCusto => col_num_cor(cor, titulo, 2, |p: P<'a>| cel(fmt::moeda(p.preco_custo))),
        ColunaProd::PrecoVenda => col_num_cor(cor, titulo, 2, |p: P<'a>| cel(fmt::moeda(p.preco_venda))),
        ColunaProd::Tags => col_cor(cor, titulo, 3, |p: P<'a>| cel(p.tags.join(", "))),
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

fn celula_saldo<'a>(saldo: f64) -> iced::widget::Text<'a> {
    let e: EstiloTexto = if saldo < 0.0 { text::danger } else { text::success };
    cel(fmt::qtd(saldo)).style(e)
}

/// Estoque do item. Com os locais juntados e o item tendo cópias, mostra cada local
/// e o total: "Produção: 114", "Almoxarifado: 3.000", "Total: 3.114".
fn celula_estoque_locais<'a>(locais: Option<&'a Locais>, codigo: i64, producao: f64, total: f64) -> Element<'a, Message> {
    let valor = |v: f64| cel(fmt::estoque(v)).style(if v < 0.0 { text::danger } else { sem_cor });
    let Some(outros) = locais.and_then(|l| l.outros.get(&codigo)) else {
        return valor(total).into();
    };
    let linha = |nome: &str, v: f64| row![cel(format!("{nome}:")).style(text::secondary), valor(v)].spacing(4);
    let mut col = column![linha(locais::PRODUCAO, producao)].align_x(Right);
    for l in outros {
        col = col.push(linha(&l.nome, l.estoque));
    }
    col.push(row![cel("Total:").font(NEGRITO), valor(total).font(NEGRITO)].spacing(4)).into()
}

/// Monta uma coluna da tabela de Necessidades.
fn coluna_necessidade<'a>(
    c: ColunaNec,
    cor: CorColuna,
    sel: Option<i64>,
    locais: Option<&'a Locais>,
    cabecalho: Element<'a, Message>,
) -> table::Column<'a, 'a, &'a Necessidade, Message> {
    type N<'x> = &'x Necessidade;
    match c {
        ColunaNec::Material => col_cor(cor, cabecalho, 6, move |n: N<'a>| {
            button(text(n.descricao.as_str()).size(13))
                .padding([1, 4])
                .style(botao_da_coluna(cor, sel == Some(n.cod_insumo)))
                .on_press(Message::SelMaterial(n.cod_insumo))
        }),
        ColunaNec::Codigo => col_num_cor(cor, cabecalho, 1, |n: N<'a>| cel(n.cod_insumo.to_string())),
        ColunaNec::CodigoProprio => col_cor(cor, cabecalho, 2, |n: N<'a>| cel(n.codigo_proprio.as_str())),
        ColunaNec::Categoria => col_cor(cor, cabecalho, 2, |n: N<'a>| cel(n.categoria.as_str())),
        ColunaNec::Unidade => col_cor(cor, cabecalho, 1, |n: N<'a>| cel(n.unidade.as_str())),
        ColunaNec::Necessidade => col_num_cor(cor, cabecalho, 2, |n: N<'a>| cel(fmt::qtd(n.necessidade))),
        // Com os locais juntados, `n.estoque` já é o total de todos os locais.
        ColunaNec::Estoque => col_num_cor(cor, cabecalho, if locais.is_some() { 3 } else { 2 }, move |n: N<'a>| {
            let fora = locais.map_or(0.0, |l| l.estoque_fora(n.cod_insumo));
            celula_estoque_locais(locais, n.cod_insumo, n.estoque - fora, n.estoque)
        }),
        ColunaNec::ConsumoParcial => col_num_cor(cor, cabecalho, 2, |n: N<'a>| {
            if n.consumo_parcial > 0.0 { cel(fmt::qtd(n.consumo_parcial)) } else { cel("—").style(text::secondary) }
        }),
        ColunaNec::EstoqueFisicoReal => col_num_cor(cor, cabecalho, 2, |n: N<'a>| {
            let real = n.estoque_fisico_real();
            cel(fmt::qtd(real)).style(if real < 0.0 { text::danger } else { sem_cor })
        }),
        ColunaNec::EstoqueMinimo => col_num_cor(cor, cabecalho, 2, |n: N<'a>| cel(fmt::qtd(n.estoque_minimo))),
        ColunaNec::Saldo => col_num_cor(cor, cabecalho, 2, |n: N<'a>| celula_saldo(n.saldo())),
        ColunaNec::NecessidadeSimulacao => col_num_cor(cor, cabecalho, 2, |n: N<'a>| {
            if n.simulada > 0.0 { cel(fmt::qtd(n.simulada)) } else { cel("—").style(text::secondary) }
        }),
        ColunaNec::SaldoAposSimulacao => col_num_cor(cor, cabecalho, 2, |n: N<'a>| celula_saldo(n.saldo_apos_simulacao())),
        ColunaNec::SaldoAposOpsESimulacao => {
            col_num_cor(cor, cabecalho, 2, |n: N<'a>| celula_saldo(n.saldo_apos_ops_e_simulacao()))
        }
        ColunaNec::Falta => col_num_cor(cor, cabecalho, 2, |n: N<'a>| {
            let e: EstiloTexto = if n.falta() > 0.0 { text::danger } else { text::secondary };
            cel(if n.falta() > 0.0 { fmt::qtd(n.falta()) } else { "—".into() }).style(e).font(NEGRITO)
        }),
        ColunaNec::ComprarComMinimo => col_num_cor(cor, cabecalho, 2, |n: N<'a>| cel(fmt::qtd(n.comprar_com_minimo()))),
        ColunaNec::CustoUnit => col_num_cor(cor, cabecalho, 2, |n: N<'a>| cel(fmt::moeda(n.custo_unit))),
        ColunaNec::CustoNecessidade => col_num_cor(cor, cabecalho, 2, |n: N<'a>| cel(fmt::moeda(n.custo_necessidade()))),
        ColunaNec::CustoFalta => col_num_cor(cor, cabecalho, 2, |n: N<'a>| cel(fmt::moeda(n.custo_falta()))),
        ColunaNec::ControlaEstoque => {
            col_cor(cor, cabecalho, 1, |n: N<'a>| cel(if n.controla_estoque { "Sim" } else { "Não" }))
        }
        ColunaNec::NumOps => col_num_cor(cor, cabecalho, 1, |n: N<'a>| cel(n.ops.len().to_string())),
        ColunaNec::ListaOps => col_cor(cor, cabecalho, 3, |n: N<'a>| {
            cel(n.ops.iter().map(|o| o.to_string()).collect::<Vec<_>>().join(", "))
        }),
        ColunaNec::NumProdutos => col_num_cor(cor, cabecalho, 1, |n: N<'a>| cel(n.produtos.len().to_string())),
        ColunaNec::Tags => col_cor(cor, cabecalho, 3, |n: N<'a>| -> Element<'a, Message> {
            if n.tags.is_empty() {
                cel("—").style(text::secondary).into()
            } else {
                let mut r = row![].spacing(4);
                for t in &n.tags {
                    r = r.push(container(text(t.as_str()).size(11)).padding([1, 6]).style(container::rounded_box));
                }
                r.wrap().vertical_spacing(4).into()
            }
        }),
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
    col_num_com(cab(titulo).into(), peso, f)
}

fn col_num_com<'a, 'b, T: 'a, E: Into<Element<'a, Message>>>(
    cabecalho: Element<'a, Message>,
    peso: u16,
    f: impl Fn(T) -> E + 'b,
) -> table::Column<'a, 'b, T, Message> {
    table::column(cabecalho, f).width(FillPortion(peso)).align_x(Right)
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

/// Etiqueta colorida da situação da OP no Trello: vermelho aguardando máquina,
/// laranja em máquina e amarelo aguardando baixa. Cores fixas, iguais nos dois temas.
fn selo_trello<'a>(s: &Situacao) -> Element<'a, Message> {
    let (fundo, letra) = match s.etapa {
        Etapa::AguardandoMaquina => (iced::Color::from_rgb8(0xC6, 0x28, 0x28), iced::Color::WHITE),
        Etapa::EmMaquina => (iced::Color::from_rgb8(0xEF, 0x6C, 0x00), iced::Color::WHITE),
        Etapa::AguardandoBaixa => (iced::Color::from_rgb8(0xF9, 0xC8, 0x0E), iced::Color::BLACK),
    };
    container(text(s.texto()).size(11).font(NEGRITO))
        .padding([1, 6])
        .style(move |_: &Theme| container::Style {
            background: Some(fundo.into()),
            text_color: Some(letra),
            border: iced::Border { radius: 4.0.into(), ..iced::Border::default() },
            ..container::Style::default()
        })
        .into()
}

/// Etiqueta do cartão no Trello, com a cor dela. Sem nome, fica só a faixa colorida.
fn selo_etiqueta<'a>(e: &trello::Etiqueta, tamanho: u32) -> Element<'a, Message> {
    let (r, g, b) = e.rgb();
    let fundo = iced::Color::from_rgb8(r, g, b);
    let letra = if e.letra_clara() { iced::Color::WHITE } else { iced::Color::from_rgb8(0x17, 0x2B, 0x4D) };
    let conteudo: Element<'a, Message> = if e.nome.is_empty() {
        space::horizontal().width(tamanho as f32 * 2.5).height(tamanho as f32 + 2.0).into()
    } else {
        text(e.nome.clone()).size(tamanho).font(NEGRITO).into()
    };
    container(conteudo)
        .padding([1, 6])
        .style(move |_: &Theme| container::Style {
            background: Some(fundo.into()),
            text_color: Some(letra),
            border: iced::Border { radius: 4.0.into(), ..iced::Border::default() },
            ..container::Style::default()
        })
        .into()
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
    scrollable(container(conteudo).padding(iced::Padding { right: FOLGA_ROLAGEM, ..iced::Padding::ZERO }))
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
