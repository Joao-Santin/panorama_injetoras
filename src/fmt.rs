//! Formatação de números no padrão brasileiro (1.234,56).

pub fn num(v: f64, casas: usize) -> String {
    if !v.is_finite() {
        return "-".into();
    }
    let s = format!("{:.*}", casas, v.abs());
    let (int, dec) = match s.split_once('.') {
        Some((i, d)) => (i.to_string(), Some(d.to_string())),
        None => (s, None),
    };
    let mut agrupado = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            agrupado.push('.');
        }
        agrupado.push(c);
    }
    let neg = v < 0.0 && agrupado.chars().chain(dec.iter().flat_map(|d| d.chars())).any(|c| c.is_ascii_digit() && c != '0');
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&agrupado);
    if let Some(d) = dec {
        out.push(',');
        out.push_str(&d);
    }
    out
}

/// Quantidade: inteiro se for "redondo", senão 3 casas.
pub fn qtd(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { num(v, 0) } else { num(v, 3) }
}

pub fn moeda(v: f64) -> String {
    // Espaço não separável: "R$" nunca fica numa linha e o valor em outra.
    format!("R$\u{a0}{}", num(v, 2))
}

/// Custo unitário: 4 casas, pois há itens de frações de centavo.
pub fn custo(v: f64) -> String {
    format!("R$\u{a0}{}", num(v, 4))
}

/// Lê uma quantidade digitada, maior que zero, no padrão brasileiro: "1.000" é mil e
/// "2,5" é dois e meio. Sem vírgula, um ponto só é decimal se não separar milhares ("1.5").
pub fn ler_qtd(s: &str) -> Option<f64> {
    let s = s.trim();
    if !s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
        return None;
    }
    let separa_milhares = s.contains('.') && s.split('.').skip(1).all(|g| g.len() == 3);
    let normalizado = if s.contains(',') {
        s.replace('.', "").replace(',', ".")
    } else if separa_milhares {
        s.replace('.', "")
    } else {
        s.to_string()
    };
    normalizado.parse().ok().filter(|v: &f64| v.is_finite() && *v > 0.0)
}

/// "2026-11-01" → "01/11/2026"; mantém o texto se não for nesse formato.
pub fn data(s: &str) -> String {
    let d = s.get(..10).unwrap_or(s);
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 { format!("{}/{}/{}", p[2], p[1], p[0]) } else { s.to_string() }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn formata() {
        assert_eq!(num(1234567.891, 2), "1.234.567,89");
        assert_eq!(num(-1605.0, 0), "-1.605");
        assert_eq!(num(12.0, 3), "12,000");
        assert_eq!(num(-0.0001, 2), "0,00");
        assert_eq!(qtd(30000.0), "30.000");
        assert_eq!(data("2026-11-01"), "01/11/2026");
    }

    #[test]
    fn le_quantidade_digitada() {
        assert_eq!(ler_qtd("1"), Some(1.0));
        assert_eq!(ler_qtd(" 250 "), Some(250.0));
        assert_eq!(ler_qtd("2,5"), Some(2.5));
        assert_eq!(ler_qtd("1.000"), Some(1000.0));
        assert_eq!(ler_qtd("1.000.000"), Some(1_000_000.0));
        assert_eq!(ler_qtd("1.234,5"), Some(1234.5));
        assert_eq!(ler_qtd("1.5"), Some(1.5));
        for invalida in ["", "0", "0,0", "-5", "abc", "1,2,3", "1e5", "inf", "1.2.3"] {
            assert_eq!(ler_qtd(invalida), None, "{invalida:?}");
        }
    }
}
