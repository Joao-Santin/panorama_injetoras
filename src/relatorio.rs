//! Geração da planilha Excel com o levantamento.

use std::collections::HashMap;

use anyhow::Result;
use rust_xlsxwriter::{
    Color, ConditionalFormatCell, ConditionalFormatCellRule, Format, FormatAlign, Workbook, Worksheet,
};

use crate::{
    calculo::{Necessidade, Resultado},
    modelo::{Op, Produto},
};

#[derive(Clone, Copy)]
enum Tipo {
    Texto,
    Inteiro,
    Qtd,
    Moeda,
}

enum Cel {
    T(String),
    N(f64),
}

fn t(s: impl Into<String>) -> Cel {
    Cel::T(s.into())
}
fn n(v: f64) -> Cel {
    Cel::N(v)
}

struct Formatos {
    cabecalho: Format,
    texto: Format,
    inteiro: Format,
    qtd: Format,
    moeda: Format,
    alerta: Format,
    ok: Format,
    titulo: Format,
}

impl Formatos {
    fn new() -> Self {
        Self {
            cabecalho: Format::new()
                .set_bold()
                .set_font_color(Color::White)
                .set_background_color(Color::RGB(0x1F4E78))
                .set_align(FormatAlign::Center)
                .set_text_wrap(),
            texto: Format::new(),
            inteiro: Format::new().set_num_format("0"),
            qtd: Format::new().set_num_format("#,##0.000"),
            moeda: Format::new().set_num_format("\"R$\" #,##0.00"),
            alerta: Format::new().set_font_color(Color::RGB(0x9C0006)).set_background_color(Color::RGB(0xFFC7CE)),
            ok: Format::new().set_font_color(Color::RGB(0x006100)).set_background_color(Color::RGB(0xC6EFCE)),
            titulo: Format::new().set_bold().set_font_size(14),
        }
    }
    fn de(&self, tipo: Tipo) -> &Format {
        match tipo {
            Tipo::Texto => &self.texto,
            Tipo::Inteiro => &self.inteiro,
            Tipo::Qtd => &self.qtd,
            Tipo::Moeda => &self.moeda,
        }
    }
}

/// Escreve uma tabela com cabeçalho, filtro e painel congelado.
/// Retorna o índice da última linha escrita.
fn tabela(ws: &mut Worksheet, f: &Formatos, colunas: &[(&str, f64, Tipo)], linhas: Vec<Vec<Cel>>) -> Result<u32> {
    for (c, (nome, largura, _)) in colunas.iter().enumerate() {
        ws.write_with_format(0, c as u16, *nome, &f.cabecalho)?;
        ws.set_column_width(c as u16, *largura)?;
    }
    ws.set_row_height(0, 30)?;
    let mut ultima = 0u32;
    for (i, linha) in linhas.into_iter().enumerate() {
        let r = i as u32 + 1;
        ultima = r;
        for (c, cel) in linha.into_iter().enumerate() {
            let fmt = f.de(colunas[c].2);
            match cel {
                Cel::T(s) => ws.write_with_format(r, c as u16, s, fmt)?,
                Cel::N(v) => ws.write_with_format(r, c as u16, v, fmt)?,
            };
        }
    }
    ws.autofilter(0, 0, ultima.max(1), colunas.len() as u16 - 1)?;
    ws.set_freeze_panes(1, 0)?;
    Ok(ultima)
}

fn destacar_positivos(ws: &mut Worksheet, f: &Formatos, col: u16, ultima: u32) -> Result<()> {
    if ultima == 0 {
        return Ok(());
    }
    let cf = ConditionalFormatCell::new()
        .set_rule(ConditionalFormatCellRule::GreaterThan(0.0000001))
        .set_format(&f.alerta);
    ws.add_conditional_format(1, col, ultima, col, &cf)?;
    Ok(())
}

fn destacar_negativos(ws: &mut Worksheet, f: &Formatos, col: u16, ultima: u32) -> Result<()> {
    if ultima == 0 {
        return Ok(());
    }
    let neg = ConditionalFormatCell::new()
        .set_rule(ConditionalFormatCellRule::LessThan(-0.0000001))
        .set_format(&f.alerta);
    let pos = ConditionalFormatCell::new()
        .set_rule(ConditionalFormatCellRule::GreaterThanOrEqualTo(0))
        .set_format(&f.ok);
    ws.add_conditional_format(1, col, ultima, col, &neg)?;
    ws.add_conditional_format(1, col, ultima, col, &pos)?;
    Ok(())
}

fn juntar(v: impl IntoIterator<Item = i64>) -> String {
    v.into_iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
}

pub struct Totais {
    pub num_ops: usize,
    pub num_itens: usize,
    pub pecas: f64,
    pub peso_pecas_kg: f64,
    pub materiais: usize,
    pub materiais_em_falta: usize,
    pub valor_faltas: f64,
    pub horas_maquina: f64,
}

pub fn totais(ops: &[Op], produtos: &HashMap<i64, Produto>, res: &Resultado) -> Totais {
    let itens: Vec<_> = ops.iter().flat_map(|o| &o.itens).collect();
    let materiais: Vec<&Necessidade> = res.necessidades.iter().filter(|n| !n.mao_de_obra).collect();
    Totais {
        num_ops: ops.len(),
        num_itens: itens.len(),
        pecas: itens.iter().map(|i| i.quant).sum(),
        peso_pecas_kg: itens.iter().map(|i| i.peso(produtos)).sum(),
        materiais: materiais.len(),
        materiais_em_falta: materiais.iter().filter(|n| n.falta() > 0.0).count(),
        valor_faltas: materiais.iter().map(|n| n.custo_falta()).sum(),
        horas_maquina: res.necessidades.iter().filter(|n| n.mao_de_obra).map(|n| n.necessidade).sum(),
    }
}

pub fn gerar(
    caminho: &str,
    gerado_em: &str,
    ops: &[Op],
    produtos: &HashMap<i64, Produto>,
    res: &Resultado,
    tot: &Totais,
) -> Result<()> {
    let f = Formatos::new();
    let mut wb = Workbook::new();

    // ---------------------------------------------------------------- Resumo
    {
        let ws = wb.add_worksheet().set_name("Resumo")?;
        ws.write_with_format(0, 0, "Panorama de necessidades de materiais — OPs em aberto", &f.titulo)?;
        ws.write(1, 0, format!("Gerado em {gerado_em} (dados do eGestor)"))?;
        let linhas: [(&str, f64, &Format); 8] = [
            ("OPs em aberto", tot.num_ops as f64, &f.inteiro),
            ("Itens (produtos) nas OPs", tot.num_itens as f64, &f.inteiro),
            ("Peças a produzir", tot.pecas, &f.qtd),
            ("Peso das peças a produzir (kg, peso líquido)", tot.peso_pecas_kg, &f.qtd),
            ("Materiais diferentes necessários", tot.materiais as f64, &f.inteiro),
            ("Materiais com estoque insuficiente", tot.materiais_em_falta as f64, &f.inteiro),
            ("Valor estimado das faltas (custo)", tot.valor_faltas, &f.moeda),
            ("Horas-máquina / mão de obra previstas", tot.horas_maquina, &f.qtd),
        ];
        for (i, (rotulo, valor, fmt)) in linhas.iter().enumerate() {
            ws.write(3 + i as u32, 0, *rotulo)?;
            ws.write_with_format(3 + i as u32, 1, *valor, fmt)?;
        }
        ws.set_column_width(0, 48)?;
        ws.set_column_width(1, 18)?;
        let obs = [
            "Abas:",
            "• Alertas — inconsistências de cadastro que distorcem o cálculo (CONFIRA ANTES DE COMPRAR).",
            "• Necessidades — materiais somados de todas as OPs × estoque atual (o que falta comprar).",
            "• Mão de obra — horas de máquina/MO previstas por insumo de MO.",
            "• Detalhe por OP — cada OP × produto × insumo, com consumo por peça e total.",
            "• OPs em aberto — cabeçalho das OPs, com peças e peso total.",
            "• Produtos — cadastro completo (estoque, peso, custo) de todos os produtos.",
            "",
            "Critérios: necessidade = quantidade da OP × consumo por peça com perda (qntComPerda).",
            "Falta = necessidade − estoque atual (quando positivo). 'Comprar c/ mínimo' inclui o estoque mínimo.",
            "Categoria 'Clientes' = material normalmente fornecido pelo cliente.",
            "Explosão de 1 nível: se um insumo for um semiacabado produzido internamente, a MP dele não é somada aqui.",
            "O eGestor não informa quanto de cada OP já foi produzido; considera-se a OP inteira pendente.",
        ];
        for (i, o) in obs.iter().enumerate() {
            ws.write(13 + i as u32, 0, *o)?;
        }
    }

    // -------------------------------------------------------------- Alertas
    {
        let ws = wb.add_worksheet().set_name("Alertas")?;
        let colunas: &[(&str, f64, Tipo)] = &[
            ("Gravidade", 9.0, Tipo::Texto),
            ("Tipo", 22.0, Tipo::Texto),
            ("OP", 7.0, Tipo::Texto),
            ("Cód.", 7.0, Tipo::Inteiro),
            ("Produto / material", 45.0, Tipo::Texto),
            ("Mensagem", 90.0, Tipo::Texto),
        ];
        let linhas = res
            .alertas
            .iter()
            .map(|a| {
                vec![
                    t(a.gravidade),
                    t(a.tipo),
                    t(a.op.map(|o| o.to_string()).unwrap_or_default()),
                    n(a.codigo as f64),
                    t(&a.descricao),
                    t(&a.mensagem),
                ]
            })
            .collect();
        tabela(ws, &f, colunas, linhas)?;
    }

    // ---------------------------------------------------------- Necessidades
    let colunas_nec: &[(&str, f64, Tipo)] = &[
        ("Cód.", 7.0, Tipo::Inteiro),
        ("Cód. próprio", 12.0, Tipo::Texto),
        ("Material", 50.0, Tipo::Texto),
        ("Categoria", 12.0, Tipo::Texto),
        ("Un.", 6.0, Tipo::Texto),
        ("Necessidade OPs", 14.0, Tipo::Qtd),
        ("Estoque atual", 14.0, Tipo::Qtd),
        ("Saldo após OPs", 14.0, Tipo::Qtd),
        ("Falta", 14.0, Tipo::Qtd),
        ("Estoque mínimo", 12.0, Tipo::Qtd),
        ("Comprar c/ mínimo", 14.0, Tipo::Qtd),
        ("Custo unit.", 12.0, Tipo::Moeda),
        ("Custo necessidade", 14.0, Tipo::Moeda),
        ("Custo da falta", 14.0, Tipo::Moeda),
        ("Controla estoque", 10.0, Tipo::Texto),
        ("Nº OPs", 7.0, Tipo::Inteiro),
        ("OPs", 40.0, Tipo::Texto),
    ];
    let linha_nec = |x: &Necessidade| -> Vec<Cel> {
        vec![
            n(x.cod_insumo as f64),
            t(&x.codigo_proprio),
            t(&x.descricao),
            t(&x.categoria),
            t(&x.unidade),
            n(x.necessidade),
            n(x.estoque),
            n(x.saldo()),
            n(x.falta()),
            n(x.estoque_minimo),
            n(x.comprar_com_minimo()),
            n(x.custo_unit),
            n(x.custo_necessidade()),
            n(x.custo_falta()),
            t(if x.controla_estoque { "Sim" } else { "Não" }),
            n(x.ops.len() as f64),
            t(juntar(x.ops.iter().copied())),
        ]
    };
    {
        let ws = wb.add_worksheet().set_name("Necessidades")?;
        let linhas = res.necessidades.iter().filter(|x| !x.mao_de_obra).map(linha_nec).collect();
        let ult = tabela(ws, &f, colunas_nec, linhas)?;
        destacar_negativos(ws, &f, 7, ult)?;
        destacar_positivos(ws, &f, 8, ult)?;
        destacar_positivos(ws, &f, 13, ult)?;
    }

    // ---------------------------------------------------------- Mão de obra
    {
        let ws = wb.add_worksheet().set_name("Mão de obra")?;
        let colunas: &[(&str, f64, Tipo)] = &[
            ("Cód.", 7.0, Tipo::Inteiro),
            ("Cód. próprio", 12.0, Tipo::Texto),
            ("Insumo de MO", 50.0, Tipo::Texto),
            ("Un.", 6.0, Tipo::Texto),
            ("Quantidade (h)", 14.0, Tipo::Qtd),
            ("Custo unit.", 12.0, Tipo::Moeda),
            ("Custo total", 14.0, Tipo::Moeda),
            ("Nº OPs", 7.0, Tipo::Inteiro),
            ("OPs", 40.0, Tipo::Texto),
        ];
        let linhas = res
            .necessidades
            .iter()
            .filter(|x| x.mao_de_obra)
            .map(|x| {
                vec![
                    n(x.cod_insumo as f64),
                    t(&x.codigo_proprio),
                    t(&x.descricao),
                    t(&x.unidade),
                    n(x.necessidade),
                    n(x.custo_unit),
                    n(x.custo_necessidade()),
                    n(x.ops.len() as f64),
                    t(juntar(x.ops.iter().copied())),
                ]
            })
            .collect();
        tabela(ws, &f, colunas, linhas)?;
    }

    // ------------------------------------------------------ Detalhe por OP
    {
        let ws = wb.add_worksheet().set_name("Detalhe por OP")?;
        let colunas: &[(&str, f64, Tipo)] = &[
            ("OP", 7.0, Tipo::Inteiro),
            ("Tags OP", 30.0, Tipo::Texto),
            ("Início", 11.0, Tipo::Texto),
            ("Cód. prod.", 8.0, Tipo::Inteiro),
            ("Cód. próprio prod.", 12.0, Tipo::Texto),
            ("Produto", 42.0, Tipo::Texto),
            ("Qtd. OP", 10.0, Tipo::Qtd),
            ("Peso un. (kg)", 10.0, Tipo::Qtd),
            ("Peso total (kg)", 11.0, Tipo::Qtd),
            ("Cód. insumo", 8.0, Tipo::Inteiro),
            ("Cód. próprio insumo", 12.0, Tipo::Texto),
            ("Insumo", 42.0, Tipo::Texto),
            ("Categoria", 12.0, Tipo::Texto),
            ("Un.", 6.0, Tipo::Texto),
            ("Consumo/peça c/ perda", 12.0, Tipo::Qtd),
            ("% perda", 8.0, Tipo::Qtd),
            ("Necessidade", 12.0, Tipo::Qtd),
        ];
        let linhas = res
            .detalhe
            .iter()
            .map(|d| {
                vec![
                    n(d.op as f64),
                    t(&d.op_tags),
                    t(&d.dt_inicio),
                    n(d.cod_produto as f64),
                    t(&d.produto_cod_proprio),
                    t(&d.produto),
                    n(d.quant),
                    n(d.peso_un),
                    n(d.peso_un * d.quant),
                    n(d.cod_insumo as f64),
                    t(&d.insumo_cod_proprio),
                    t(&d.insumo),
                    t(&d.categoria),
                    t(&d.unidade),
                    n(d.consumo_un),
                    n(d.perc_perda),
                    n(d.necessidade),
                ]
            })
            .collect();
        tabela(ws, &f, colunas, linhas)?;
    }

    // ------------------------------------------------------ OPs em aberto
    {
        let ws = wb.add_worksheet().set_name("OPs em aberto")?;
        let colunas: &[(&str, f64, Tipo)] = &[
            ("OP", 7.0, Tipo::Inteiro),
            ("Cadastro", 17.0, Tipo::Texto),
            ("Início", 11.0, Tipo::Texto),
            ("Entrega", 11.0, Tipo::Texto),
            ("Tags", 36.0, Tipo::Texto),
            ("Nº produtos", 9.0, Tipo::Inteiro),
            ("Peças", 10.0, Tipo::Qtd),
            ("Peso total (kg)", 12.0, Tipo::Qtd),
            ("Valor total", 13.0, Tipo::Moeda),
            ("Produtos", 60.0, Tipo::Texto),
            ("Obs.", 30.0, Tipo::Texto),
        ];
        let linhas = ops
            .iter()
            .map(|o| {
                let pecas = o.pecas();
                let peso = o.peso(produtos);
                let nomes = o.itens.iter().map(|i| format!("{} ({})", i.descricao, i.quant)).collect::<Vec<_>>();
                vec![
                    n(o.codigo as f64),
                    t(&o.dt_cad),
                    t(&o.dt_inicio),
                    t(&o.dt_entrega),
                    t(o.tags.join(", ")),
                    n(o.itens.len() as f64),
                    n(pecas),
                    n(peso),
                    n(o.valor_total),
                    t(nomes.join("; ")),
                    t(&o.obs),
                ]
            })
            .collect();
        tabela(ws, &f, colunas, linhas)?;
    }

    // ------------------------------------------------------------ Produtos
    {
        let ws = wb.add_worksheet().set_name("Produtos")?;
        let colunas: &[(&str, f64, Tipo)] = &[
            ("Cód.", 7.0, Tipo::Inteiro),
            ("Cód. próprio", 12.0, Tipo::Texto),
            ("Descrição", 50.0, Tipo::Texto),
            ("Tipo", 9.0, Tipo::Texto),
            ("Un.", 6.0, Tipo::Texto),
            ("Estoque", 12.0, Tipo::Qtd),
            ("Estoque mínimo", 12.0, Tipo::Qtd),
            ("Controla estoque", 10.0, Tipo::Texto),
            ("Peso líquido (kg)", 11.0, Tipo::Qtd),
            ("Peso bruto (kg)", 11.0, Tipo::Qtd),
            ("Preço custo", 12.0, Tipo::Moeda),
            ("Preço venda", 12.0, Tipo::Moeda),
            ("Tags", 36.0, Tipo::Texto),
        ];
        let mut lista: Vec<&Produto> = produtos.values().collect();
        lista.sort_by_key(|p| p.codigo);
        let linhas = lista
            .into_iter()
            .map(|p| {
                vec![
                    n(p.codigo as f64),
                    t(&p.codigo_proprio),
                    t(&p.descricao),
                    t(&p.tipo),
                    t(&p.unidade),
                    n(p.estoque),
                    n(p.estoque_minimo),
                    t(if p.controlar_estoque { "Sim" } else { "Não" }),
                    n(p.peso_liquido),
                    n(p.peso_bruto),
                    n(p.preco_custo),
                    n(p.preco_venda),
                    t(p.tags.join(", ")),
                ]
            })
            .collect();
        tabela(ws, &f, colunas, linhas)?;
    }

    wb.save(caminho)?;
    Ok(())
}
