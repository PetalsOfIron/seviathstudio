// Line icons drawn on a 16×16 grid, scaled into any rect. Own drawings, no icon font needed.

use std::f32::consts::PI;

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

use crate::snake::arc;

#[allow(dead_code)] // some icons are for panels that land in later steps
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    // Tabs
    Draw,
    Parts,
    Rig,
    Physics,
    Animate,
    Stream,
    // Draw tools
    Brush,
    Pencil,
    Eraser,
    Fill,
    Gradient,
    Picker,
    Select,
    Lasso,
    Move,
    Text,
    Shape,
    Pen,
    Smudge,
    Hand,
    Zoom,
    // UI
    Undo,
    Redo,
    Settings,
    User,
    Eye,
    EyeOff,
    Plus,
    Trash,
    Lock,
    Folder,
    Popout,
    // Canvas view
    ZoomIn,
    ZoomOut,
    Fit,
    Flip,
    RotateLeft,
    RotateRight,
}

pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let u = s / 16.0;
    let at = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    let w = (1.5 * u).max(1.0);
    let st = Stroke::new(w, color);
    let thick = Stroke::new(w * 1.5, color);
    let line = |pts: &[(f32, f32)], stroke: Stroke| {
        p.add(Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), stroke));
    };
    let closed = |pts: &[(f32, f32)], stroke: Stroke| {
        p.add(Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), stroke));
    };
    let circle = |x: f32, y: f32, r: f32| {
        p.circle_stroke(at(x, y), r * u, st);
    };
    let dot = |x: f32, y: f32, r: f32| {
        p.circle_filled(at(x, y), r * u, color);
    };
    let arc_line = |x: f32, y: f32, r: f32, a0: f32, a1: f32| {
        p.add(Shape::line(arc(at(x, y), r * u, a0, a1, 14), st));
    };

    match icon {
        Icon::Draw | Icon::Brush => {
            line(&[(13.0, 2.0), (7.5, 8.5)], thick);
            dot(5.6, 10.6, 2.6);
            line(&[(4.0, 12.5), (2.0, 14.5)], st);
        }
        Icon::Pencil => {
            closed(&[(11.0, 2.0), (14.0, 5.0), (5.0, 14.0), (2.0, 14.0), (2.0, 11.0)], st);
            line(&[(9.5, 3.5), (12.5, 6.5)], st);
        }
        Icon::Eraser => {
            closed(&[(9.0, 2.0), (14.0, 7.0), (8.0, 13.0), (3.0, 8.0)], st);
            line(&[(6.0, 5.0), (11.0, 10.0)], st);
            line(&[(8.0, 14.5), (14.5, 14.5)], st);
        }
        Icon::Fill => {
            closed(&[(2.5, 8.0), (7.5, 3.0), (12.5, 8.0), (7.5, 13.0)], st);
            line(&[(2.5, 8.0), (12.5, 8.0)], st);
            dot(14.0, 12.5, 1.6);
        }
        Icon::Gradient => {
            let r0 = Rect::from_min_max(at(2.0, 3.0), at(14.0, 13.0));
            for (i, a) in [1.0f32, 0.65, 0.35, 0.12].iter().enumerate() {
                let x0 = 2.0 + i as f32 * 3.0;
                p.rect_filled(Rect::from_min_max(at(x0, 3.0), at(x0 + 3.0, 13.0)), 0.0, color.gamma_multiply(*a));
            }
            p.rect_stroke(r0, 1.0, st, eframe::egui::StrokeKind::Middle);
        }
        Icon::Picker => {
            line(&[(2.5, 13.5), (9.5, 6.5)], thick);
            dot(11.5, 4.5, 2.4);
            line(&[(8.0, 4.0), (12.0, 8.0)], st);
        }
        Icon::Select => {
            for (a, b) in [
                ((2.0, 2.0), (6.0, 2.0)),
                ((10.0, 2.0), (14.0, 2.0)),
                ((14.0, 2.0), (14.0, 6.0)),
                ((14.0, 10.0), (14.0, 14.0)),
                ((14.0, 14.0), (10.0, 14.0)),
                ((6.0, 14.0), (2.0, 14.0)),
                ((2.0, 14.0), (2.0, 10.0)),
                ((2.0, 6.0), (2.0, 2.0)),
            ] {
                line(&[a, b], st);
            }
        }
        Icon::Lasso => {
            let pts: Vec<Pos2> = arc(at(8.5, 6.5), 1.0, 0.0, 2.0 * PI, 24)
                .into_iter()
                .map(|q| {
                    let d = q - at(8.5, 6.5);
                    at(8.5, 6.5) + eframe::egui::vec2(d.x * 6.0 * u, d.y * 4.0 * u)
                })
                .collect();
            p.add(Shape::closed_line(pts, st));
            line(&[(5.0, 10.0), (4.5, 12.0), (5.5, 14.5)], st);
        }
        Icon::Move => {
            line(&[(8.0, 1.5), (8.0, 14.5)], st);
            line(&[(1.5, 8.0), (14.5, 8.0)], st);
            line(&[(6.0, 3.5), (8.0, 1.5), (10.0, 3.5)], st);
            line(&[(6.0, 12.5), (8.0, 14.5), (10.0, 12.5)], st);
            line(&[(3.5, 6.0), (1.5, 8.0), (3.5, 10.0)], st);
            line(&[(12.5, 6.0), (14.5, 8.0), (12.5, 10.0)], st);
        }
        Icon::Text => {
            line(&[(3.0, 3.0), (13.0, 3.0)], thick);
            line(&[(8.0, 3.0), (8.0, 14.0)], thick);
            line(&[(6.0, 14.0), (10.0, 14.0)], st);
        }
        Icon::Shape => {
            circle(6.0, 6.0, 4.0);
            p.rect_stroke(Rect::from_min_max(at(7.5, 7.5), at(14.0, 14.0)), 1.0, st, eframe::egui::StrokeKind::Middle);
        }
        Icon::Pen => {
            closed(&[(8.0, 1.5), (13.0, 8.0), (10.0, 14.5), (6.0, 14.5), (3.0, 8.0)], st);
            line(&[(8.0, 1.5), (8.0, 8.5)], st);
            dot(8.0, 10.0, 1.2);
        }
        Icon::Smudge => {
            let pts: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    (2.0 + t * 10.0, 12.0 - t * 7.0 + (t * PI * 3.0).sin() * 1.4)
                })
                .collect();
            line(&pts, thick);
            dot(13.0, 4.0, 1.8);
        }
        Icon::Hand => {
            circle(8.0, 10.5, 3.6);
            for x in [5.0f32, 7.0, 9.0, 11.0] {
                line(&[(x, 8.0), (x, 3.0 + (x - 8.0).abs() * 0.4)], st);
            }
        }
        Icon::Zoom => {
            circle(7.0, 7.0, 4.5);
            line(&[(10.5, 10.5), (14.0, 14.0)], thick);
        }
        Icon::Parts => {
            closed(&[(2.0, 8.0), (7.0, 3.0), (14.0, 3.0), (14.0, 13.0), (7.0, 13.0)], st);
            dot(10.8, 8.0, 1.3);
        }
        Icon::Rig => {
            circle(3.8, 12.2, 2.0);
            circle(12.2, 3.8, 2.0);
            line(&[(5.3, 10.7), (10.7, 5.3)], thick);
            dot(8.0, 8.0, 1.1);
        }
        Icon::Physics => {
            for (k, x) in [4.0f32, 8.0, 12.0].iter().enumerate() {
                let pts: Vec<(f32, f32)> = (0..=10)
                    .map(|i| {
                        let t = i as f32 / 10.0;
                        (x + (t * PI * 2.0 + k as f32).sin() * 1.3 * t, 2.0 + t * 12.0)
                    })
                    .collect();
                line(&pts, st);
            }
        }
        Icon::Animate => {
            p.rect_stroke(Rect::from_min_max(at(1.5, 3.5), at(14.5, 12.5)), 2.0, st, eframe::egui::StrokeKind::Middle);
            p.add(Shape::convex_polygon(vec![at(6.5, 5.5), at(11.0, 8.0), at(6.5, 10.5)], color, Stroke::NONE));
        }
        Icon::Stream => {
            dot(8.0, 8.0, 1.8);
            arc_line(8.0, 8.0, 4.0, -PI * 0.25, PI * 0.25);
            arc_line(8.0, 8.0, 4.0, PI * 0.75, PI * 1.25);
            arc_line(8.0, 8.0, 6.8, -PI * 0.25, PI * 0.25);
            arc_line(8.0, 8.0, 6.8, PI * 0.75, PI * 1.25);
        }
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Redo { -1.0 } else { 1.0 };
            let fx = |x: f32| 8.0 + (x - 8.0) * flip;
            let pts: Vec<Pos2> = arc(at(8.0, 9.0), 5.0 * u, PI, 2.0 * PI, 16)
                .into_iter()
                .map(|q| pos2(c.x + (q.x - c.x) * flip, q.y))
                .collect();
            p.add(Shape::line(pts, st));
            line(&[(fx(3.0), 9.0), (fx(3.0), 13.0)], st);
            line(&[(fx(1.0), 6.5), (fx(3.0), 9.0), (fx(5.5), 7.0)], st);
        }
        Icon::Settings => {
            circle(8.0, 8.0, 2.6);
            for i in 0..8 {
                let a = i as f32 * PI / 4.0;
                let (sa, ca) = a.sin_cos();
                line(&[(8.0 + ca * 4.6, 8.0 + sa * 4.6), (8.0 + ca * 6.8, 8.0 + sa * 6.8)], thick);
            }
            circle(8.0, 8.0, 4.6);
        }
        Icon::User => {
            circle(8.0, 5.5, 3.0);
            arc_line(8.0, 15.0, 6.0, PI, 2.0 * PI);
        }
        Icon::Eye | Icon::EyeOff => {
            arc_line(8.0, 12.5, 7.5, PI * 1.2, PI * 1.8);
            arc_line(8.0, 3.5, 7.5, PI * 0.2, PI * 0.8);
            dot(8.0, 8.0, 2.0);
            if icon == Icon::EyeOff {
                line(&[(2.5, 13.5), (13.5, 2.5)], st);
            }
        }
        Icon::Plus => {
            line(&[(8.0, 3.0), (8.0, 13.0)], thick);
            line(&[(3.0, 8.0), (13.0, 8.0)], thick);
        }
        Icon::Trash => {
            line(&[(2.5, 4.0), (13.5, 4.0)], st);
            line(&[(6.0, 4.0), (6.5, 2.0), (9.5, 2.0), (10.0, 4.0)], st);
            closed(&[(4.0, 4.0), (12.0, 4.0), (11.0, 14.0), (5.0, 14.0)], st);
        }
        Icon::Lock => {
            p.rect_stroke(Rect::from_min_max(at(3.0, 7.0), at(13.0, 14.5)), 1.5, st, eframe::egui::StrokeKind::Middle);
            arc_line(8.0, 7.0, 3.3, PI, 2.0 * PI);
        }
        Icon::Folder => {
            closed(&[(1.5, 4.0), (6.0, 4.0), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)], st);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            circle(7.0, 7.0, 4.8);
            line(&[(10.6, 10.6), (14.0, 14.0)], thick);
            line(&[(4.8, 7.0), (9.2, 7.0)], st);
            if icon == Icon::ZoomIn {
                line(&[(7.0, 4.8), (7.0, 9.2)], st);
            }
        }
        Icon::Fit => {
            for (a, b, c) in [
                ((2.0, 6.0), (2.0, 2.0), (6.0, 2.0)),
                ((10.0, 2.0), (14.0, 2.0), (14.0, 6.0)),
                ((14.0, 10.0), (14.0, 14.0), (10.0, 14.0)),
                ((6.0, 14.0), (2.0, 14.0), (2.0, 10.0)),
            ] {
                line(&[a, b, c], st);
            }
            p.rect_stroke(Rect::from_min_max(at(5.5, 5.5), at(10.5, 10.5)), 1.0, st, eframe::egui::StrokeKind::Middle);
        }
        Icon::Flip => {
            line(&[(8.0, 1.5), (8.0, 14.5)], Stroke::new(w * 0.8, color.gamma_multiply(0.6)));
            closed(&[(6.0, 3.5), (6.0, 12.5), (1.5, 12.5)], st);
            p.add(Shape::convex_polygon(vec![at(10.0, 3.5), at(14.5, 12.5), at(10.0, 12.5)], color, Stroke::NONE));
        }
        Icon::RotateLeft | Icon::RotateRight => {
            let flip = if icon == Icon::RotateRight { -1.0 } else { 1.0 };
            let pts: Vec<Pos2> = arc(at(8.0, 8.5), 5.5 * u, PI * 1.15, PI * 2.6, 20)
                .into_iter()
                .map(|q| pos2(c.x + (q.x - c.x) * flip, q.y))
                .collect();
            p.add(Shape::line(pts, st));
            let fx = |x: f32| 8.0 + (x - 8.0) * flip;
            line(&[(fx(0.8), 5.0), (fx(2.9), 7.0), (fx(5.0), 4.6)], st);
        }
        Icon::Popout => {
            line(&[(8.0, 5.0), (2.0, 5.0), (2.0, 14.0), (11.0, 14.0), (11.0, 8.0)], st);
            line(&[(7.0, 9.0), (14.0, 2.0)], st);
            line(&[(9.5, 2.0), (14.0, 2.0), (14.0, 6.5)], st);
        }
    }
}
