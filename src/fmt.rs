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
}
