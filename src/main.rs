//! Panorama de injetados: mostra o levantamento de necessidades de materiais
//! das OPs em aberto, com os dados do eGestor e do Trello que o servidor
//! `biplas_server_claude` guarda no banco. Colunas, simulações e realizado
//! parcial também ficam no servidor, e todos os panoramas abertos se
//! atualizam juntos quando algo muda.
//!
//! Uso:
//!   cargo run --release            → interface gráfica (iced)
//!   cargo run --release -- --cli   → só lê os dados do servidor e gera a planilha Excel
//!
//! Variáveis de ambiente (opcionais; também lidas do `.env`, veja `.env.example`):
//!   BIPLAS_SERVER_URL  endereço do servidor (padrão: http://localhost:3000); o definido na
//!                      aba Configurações (em %APPDATA%\panorama_injetados) tem prioridade
//!   BIPLAS_API_KEY     chave enviada em x-api-key, se o servidor exigir
//!   SAIDA_DIR          pasta das planilhas (padrão: ./relatorios)
//!
//! O `.env` é procurado na pasta atual e nas acima dela; se não achar, na pasta
//! do executável e nas acima. Variáveis já definidas no sistema têm prioridade.

mod api;
mod calculo;
mod coleta;
mod colunas;
mod config;
mod estrutura;
mod fmt;
mod gui;
mod historico;
mod modelo;
mod notificacoes;
mod parcial;
mod relatorio;
mod simulacao;
mod trello;

use anyhow::Result;

/// Carrega o `.env` da pasta atual (ou acima); se não houver, o da pasta do executável
/// (ou acima), para funcionar também ao abrir o `.exe` direto de `target\release`.
fn carregar_env() {
    if dotenvy::dotenv().is_ok() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    for pasta in exe.ancestors().skip(1) {
        if dotenvy::from_path(pasta.join(".env")).is_ok() {
            return;
        }
    }
}

fn main() {
    carregar_env();
    if std::env::args().any(|a| a == "--cli") {
        if let Err(e) = executar_cli() {
            eprintln!("\nERRO: {e:#}");
            std::process::exit(1);
        }
    } else if let Err(e) = gui::executar() {
        eprintln!("Erro na interface: {e}");
        std::process::exit(1);
    }
}

fn executar_cli() -> Result<()> {
    let srv = api::Servidor::new()?;
    println!("Servidor: {}", api::url_servidor());
    let Some(brutos) = coleta::carregar(&srv)? else {
        anyhow::bail!("O servidor ainda não terminou nenhuma sincronização do eGestor. Tente de novo em alguns minutos.");
    };
    println!("Dados do eGestor de {}", brutos.gerado_em);
    let a = coleta::Analise::de(&brutos);
    let caminho = a.exportar_excel()?;
    imprimir_resumo(&a);
    println!("\nPlanilha gerada: {}", caminho.display());
    Ok(())
}

fn imprimir_resumo(a: &coleta::Analise) {
    let (res, tot) = (&a.res, &a.tot);
    println!("\n================ RESUMO ================");
    println!("OPs em aberto ..................: {}", tot.num_ops);
    println!("Itens nas OPs ..................: {}", tot.num_itens);
    println!("Peças a produzir ...............: {:.0}", tot.pecas);
    println!("Peso das peças (kg) ............: {:.2}", tot.peso_pecas_kg);
    println!("Horas-máquina / MO .............: {:.2}", tot.horas_maquina);
    println!("Materiais necessários ..........: {}", tot.materiais);
    println!("Materiais com estoque insuf. ...: {}", tot.materiais_em_falta);
    println!("Valor estimado das faltas ......: R$ {:.2}", tot.valor_faltas);

    let altas: Vec<_> = res.alertas.iter().filter(|a| a.gravidade == "ALTA").collect();
    println!(
        "Alertas de cadastro ............: {} ({} de gravidade ALTA — veja a aba Alertas)",
        res.alertas.len(),
        altas.len()
    );
    for a in &altas {
        println!("  ! OP {} — {}: {}", a.op.unwrap_or(0), a.descricao, a.mensagem);
    }

    let faltas: Vec<_> = res.necessidades.iter().filter(|n| !n.mao_de_obra && n.falta() > 0.0).collect();
    if !faltas.is_empty() {
        println!("\nMateriais em falta (maior custo primeiro):");
        println!("{:<52} {:>4} {:>12} {:>12} {:>12}", "Material", "Un", "Necessário", "Estoque", "Falta");
        for x in faltas.iter().take(30) {
            let nome: String = x.descricao.chars().take(52).collect();
            println!(
                "{:<52} {:>4} {:>12.3} {:>12.3} {:>12.3}",
                nome, x.unidade, x.necessidade, x.estoque, x.falta()
            );
        }
        if faltas.len() > 30 {
            println!("... e mais {} (veja a planilha)", faltas.len() - 30);
        }
    }
}
