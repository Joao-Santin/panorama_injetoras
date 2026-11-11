//! Seletor de cor em anel: o anel escolhe o matiz e o quadrado do meio a
//! saturação (da esquerda para a direita) e o brilho (de baixo para cima).

use std::f32::consts::PI;

use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    mouse::{self, Cursor},
    widget::canvas::{self, Event, Frame, Geometry, Path, Stroke, gradient},
};

use crate::colunas::Rgb;

/// Matiz (0–360), saturação e brilho (0–1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub fn rgb(self) -> Rgb {
        let c = self.v * self.s;
        let h = self.h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = self.v - c;
        let byte = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb(byte(r), byte(g), byte(b))
    }

    /// A partir de uma cor. Cinzas não têm matiz: fica `matiz_atual`.
    pub fn de_rgb(c: Rgb, matiz_atual: f32) -> Self {
        let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
        let max = r.max(g).max(b);
        let d = max - r.min(g).min(b);
        let h = if d == 0.0 {
            matiz_atual
        } else if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        Self {
            h,
            s: if max == 0.0 { 0.0 } else { d / max },
            v: max,
        }
    }
}

/// Largura do anel, em pixels.
const ANEL: f32 = 18.0;

/// Desenho do seletor: `ao_mudar` recebe a cor nova enquanto se clica ou arrasta.
pub struct Seletor<Message> {
    pub hsv: Hsv,
    pub ao_mudar: fn(Hsv) -> Message,
}

/// Parte do seletor sendo arrastada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Arrasto {
    #[default]
    Nenhum,
    Anel,
    Quadro,
}

/// Medidas do desenho dentro de `tamanho`.
struct Medidas {
    centro: Point,
    raio: f32,
    /// Canto superior esquerdo e lado do quadrado do meio.
    quadro: Point,
    lado: f32,
}

fn medidas(tamanho: Size) -> Medidas {
    let raio = tamanho.width.min(tamanho.height) / 2.0;
    let centro = Point::new(tamanho.width / 2.0, tamanho.height / 2.0);
    let lado = (raio - ANEL - 6.0) * std::f32::consts::SQRT_2;
    Medidas {
        centro,
        raio,
        quadro: Point::new(centro.x - lado / 2.0, centro.y - lado / 2.0),
        lado,
    }
}

impl<Message> Seletor<Message> {
    fn cor_no(&self, parte: Arrasto, p: Point, m: &Medidas) -> Hsv {
        let mut hsv = self.hsv;
        match parte {
            Arrasto::Anel => {
                hsv.h = (p.y - m.centro.y)
                    .atan2(p.x - m.centro.x)
                    .to_degrees()
                    .rem_euclid(360.0);
            }
            Arrasto::Quadro => {
                hsv.s = ((p.x - m.quadro.x) / m.lado).clamp(0.0, 1.0);
                hsv.v = (1.0 - (p.y - m.quadro.y) / m.lado).clamp(0.0, 1.0);
            }
            Arrasto::Nenhum => {}
        }
        hsv
    }
}

impl<Message> canvas::Program<Message> for Seletor<Message> {
    type State = Arrasto;

    fn update(
        &self,
        estado: &mut Arrasto,
        evento: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        let m = medidas(bounds.size());
        // Posição relativa ao desenho, mesmo fora dele (o arrasto continua).
        let pos = cursor
            .position()
            .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
        match evento {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let d = p.distance(m.centro);
                let dentro_do_quadro = p.x >= m.quadro.x
                    && p.x <= m.quadro.x + m.lado
                    && p.y >= m.quadro.y
                    && p.y <= m.quadro.y + m.lado;
                *estado = if d <= m.raio && d >= m.raio - ANEL - 2.0 {
                    Arrasto::Anel
                } else if dentro_do_quadro {
                    Arrasto::Quadro
                } else {
                    return None;
                };
                Some(
                    canvas::Action::publish((self.ao_mudar)(self.cor_no(*estado, p, &m)))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if *estado != Arrasto::Nenhum => {
                let p = pos?;
                Some(
                    canvas::Action::publish((self.ao_mudar)(self.cor_no(*estado, p, &m)))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if *estado != Arrasto::Nenhum =>
            {
                *estado = Arrasto::Nenhum;
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _estado: &Arrasto,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let m = medidas(bounds.size());

        // Anel: fatias de matiz (um pouco sobrepostas, para não aparecer fresta).
        const FATIAS: usize = 120;
        let (externo, interno) = (m.raio, m.raio - ANEL);
        for i in 0..FATIAS {
            let a0 = i as f32 / FATIAS as f32 * 2.0 * PI;
            let a1 = (i as f32 + 1.3) / FATIAS as f32 * 2.0 * PI;
            let ponto =
                |a: f32, r: f32| Point::new(m.centro.x + r * a.cos(), m.centro.y + r * a.sin());
            let fatia = Path::new(|b| {
                b.move_to(ponto(a0, interno));
                b.line_to(ponto(a0, externo));
                b.line_to(ponto(a1, externo));
                b.line_to(ponto(a1, interno));
                b.close();
            });
            let cor = Hsv {
                h: (i as f32 + 0.5) / FATIAS as f32 * 360.0,
                s: 1.0,
                v: 1.0,
            }
            .rgb();
            f.fill(&fatia, Color::from(cor));
        }

        // Quadrado: o matiz puro, clareado para a esquerda e escurecido para baixo.
        let (q, lado) = (m.quadro, m.lado);
        let tamanho = Size::new(lado, lado);
        let puro = Hsv {
            h: self.hsv.h,
            s: 1.0,
            v: 1.0,
        }
        .rgb();
        f.fill_rectangle(q, tamanho, Color::from(puro));
        let branco = gradient::Linear::new(q, Point::new(q.x + lado, q.y))
            .add_stop(0.0, Color::WHITE)
            .add_stop(
                1.0,
                Color {
                    a: 0.0,
                    ..Color::WHITE
                },
            );
        f.fill_rectangle(
            q,
            tamanho,
            canvas::Fill::from(gradient::Gradient::Linear(branco)),
        );
        let preto = gradient::Linear::new(q, Point::new(q.x, q.y + lado))
            .add_stop(
                0.0,
                Color {
                    a: 0.0,
                    ..Color::BLACK
                },
            )
            .add_stop(1.0, Color::BLACK);
        f.fill_rectangle(
            q,
            tamanho,
            canvas::Fill::from(gradient::Gradient::Linear(preto)),
        );
        let borda = theme.extended_palette().background.strong.color;
        f.stroke_rectangle(
            q,
            tamanho,
            Stroke::default().with_width(1.0).with_color(borda),
        );

        // Marcadores: anel preto com miolo branco, visíveis em qualquer cor.
        let marcador = |f: &mut Frame, p: Point, r: f32| {
            f.stroke(
                &Path::circle(p, r),
                Stroke::default().with_width(3.0).with_color(Color::BLACK),
            );
            f.stroke(
                &Path::circle(p, r),
                Stroke::default().with_width(1.5).with_color(Color::WHITE),
            );
        };
        let a = self.hsv.h.to_radians();
        let meio = m.raio - ANEL / 2.0;
        marcador(
            &mut f,
            Point::new(m.centro.x + meio * a.cos(), m.centro.y + meio * a.sin()),
            ANEL / 2.0,
        );
        marcador(
            &mut f,
            Point::new(q.x + self.hsv.s * lado, q.y + (1.0 - self.hsv.v) * lado),
            6.0,
        );

        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        estado: &Arrasto,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if *estado != Arrasto::Nenhum {
            return mouse::Interaction::Grabbing;
        }
        match cursor.position_in(bounds) {
            Some(_) => mouse::Interaction::Crosshair,
            None => mouse::Interaction::default(),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn converte_hsv_e_rgb() {
        assert_eq!(
            Hsv {
                h: 0.0,
                s: 1.0,
                v: 1.0
            }
            .rgb(),
            Rgb(255, 0, 0)
        );
        assert_eq!(
            Hsv {
                h: 120.0,
                s: 1.0,
                v: 1.0
            }
            .rgb(),
            Rgb(0, 255, 0)
        );
        assert_eq!(
            Hsv {
                h: 240.0,
                s: 1.0,
                v: 0.5
            }
            .rgb(),
            Rgb(0, 0, 128)
        );
        assert_eq!(
            Hsv {
                h: 77.0,
                s: 0.0,
                v: 1.0
            }
            .rgb(),
            Rgb(255, 255, 255)
        );
        for c in [
            Rgb(255, 238, 170),
            Rgb(12, 200, 99),
            Rgb(0, 0, 0),
            Rgb(128, 128, 128),
        ] {
            assert_eq!(Hsv::de_rgb(c, 0.0).rgb(), c);
        }
        // Cinza mantém o matiz que estava no anel.
        assert_eq!(Hsv::de_rgb(Rgb(128, 128, 128), 200.0).h, 200.0);
    }
}
