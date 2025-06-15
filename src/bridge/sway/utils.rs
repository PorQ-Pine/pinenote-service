use std::collections::HashSet;

use anyhow::{Context, Result, anyhow, bail};
use nalgebra::{Matrix3, Matrix3x2, Scale2, Translation2, Vector2};
use nix::libc::pid_t;
use pinenote_service::types::{
    Rect,
    rockchip_ebc::{Hint, HintBitDepth, HintConvertMode},
};
use swayipc_async::{Connection, Node, Output, Rect as SwayRect};

use super::SwayWindow;

pub(super) fn parse_hint(input: &str) -> Option<Hint> {
    let mut bitdepth: Option<HintBitDepth> = None;
    let mut dither = HintConvertMode::Threshold;
    let mut redraw = false;

    input.split("|").for_each(|h| match h {
        "Y4" => bitdepth = Some(HintBitDepth::Y4),
        "Y2" => bitdepth = Some(HintBitDepth::Y2),
        "Y1" => bitdepth = Some(HintBitDepth::Y1),
        "T" => dither = HintConvertMode::Threshold,
        "D" => dither = HintConvertMode::Dither,
        "R" => redraw = true,
        "r" => redraw = false,
        _ => {}
    });

    bitdepth.map(|bd| Hint::new(bd, dither, redraw))
}

struct StandardNodeIterator<'a> {
    queue: Vec<&'a Node>,
}

impl<'a> Iterator for StandardNodeIterator<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<&'a Node> {
        match self.queue.pop() {
            None => None,
            Some(node) => {
                self.queue.extend(node.nodes.iter());
                Some(node)
            }
        }
    }
}

fn iter_standard<'a>(node: &'a Node) -> StandardNodeIterator<'a> {
    StandardNodeIterator { queue: vec![node] }
}

pub(super) fn get_all_windows_and_app(
    workspace: &Node,
    transform: &Matrix3<f64>,
) -> (HashSet<pid_t>, Vec<SwayWindow>) {
    let mut floating_idx = 1;

    workspace
        .floating_nodes
        .iter()
        .chain(iter_standard(workspace))
        .filter_map(|n| {
            SwayWindow::try_from(n).ok().map(|n| {
                let area = apply_transform(n.area, transform);

                if n.floating {
                    let z_index = floating_idx;
                    floating_idx += 1;

                    SwayWindow { z_index, area, ..n }
                } else {
                    SwayWindow { area, ..n }
                }
            })
        })
        .map(|w| (w.pid, w))
        .collect()
}

pub(super) async fn get_output(ipc: &mut Connection, name: &str) -> Result<Output> {
    ipc.get_outputs()
        .await
        .context("Failed to retrieve outputs")?
        .into_iter()
        .find(|o| o.name.as_str() == name)
        .ok_or(anyhow!("Failed to find output '{}'", name))
}

pub(super) fn output_to_transform(output: &Output) -> Result<Matrix3<f64>> {
    let scale = output
        .scale
        .ok_or(anyhow!("Could not get output scale"))
        .and_then(|s| {
            if s == -1.0 {
                Err(anyhow!("Output is inactive"))
            } else {
                Ok(Scale2::new(s, s))
            }
        })?;

    let SwayRect {
        x,
        y,
        width,
        height,
        ..
    } = output.rect;

    let rel_to_abs = Translation2::new(-x as f64, -y as f64);

    let iso = match output
        .transform
        .as_deref()
        .ok_or(anyhow!("Bad transform"))?
    {
        "normal" => nalgebra::Isometry2::identity(),
        "90" => nalgebra::Isometry2::new(Vector2::new(height as f64, 0.0), 90_f64.to_radians()),
        "180" => nalgebra::Isometry2::new(
            Vector2::new(width as f64, height as f64),
            180_f64.to_radians(),
        ),
        "270" => nalgebra::Isometry2::new(Vector2::new(0f64, width as f64), 270_f64.to_radians()),
        _ => {
            bail!("Unsupported transform")
        }
    };

    let transform = scale.to_homogeneous() * iso.to_homogeneous() * rel_to_abs.to_homogeneous();

    Ok(transform)
}

pub(super) fn apply_transform(rect: Rect, transform: &Matrix3<f64>) -> Rect {
    let Rect { x1, y1, x2, y2 } = rect;

    let r = (transform * Matrix3x2::new(x1 as f64, x2 as f64, y1 as f64, y2 as f64, 1_f64, 1_f64))
        .map(|f| f as i32);

    let mut xs = [*r.index((0, 0)), *r.index((0, 1))];
    let mut ys = [*r.index((1, 0)), *r.index((1, 1))];

    xs.sort();
    ys.sort();

    Rect {
        x1: xs[0],
        y1: ys[0],
        x2: xs[1],
        y2: ys[1],
    }
}
