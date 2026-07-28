use anyhow::{anyhow, Result};
use std::path::PathBuf;

pub fn run(file: PathBuf, out: Option<PathBuf>, rank: usize) -> Result<()> {
    anyhow::ensure!(rank >= 1, "--rank must be >= 1");

    let out_path = out.unwrap_or_else(|| file.with_extension("png"));

    let mut rdr = csv::Reader::from_path(&file)
        .map_err(|e| anyhow!("cannot open {}: {e}", file.display()))?;

    let headers: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
    anyhow::ensure!(!headers.is_empty(), "CSV has no headers");

    let mut row: Option<Vec<String>> = None;
    for (i, result) in rdr.records().enumerate() {
        if i + 1 == rank {
            row = Some(result?.iter().map(|s| s.to_string()).collect());
            break;
        }
        result?;
    }

    let row = row.ok_or_else(|| anyhow!("rank {rank} not found in CSV"))?;
    let label = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("results");
    let text = format_snapshot_text(label, rank, &headers, &row);

    render_to_png(&text, &out_path)?;
    println!("PNG saved: {}", out_path.display());
    Ok(())
}

fn format_snapshot_text(label: &str, rank: usize, headers: &[String], row: &[String]) -> String {
    let key_w = headers.iter().map(|h| h.len()).max().unwrap_or(20);
    let val_w = row.iter().map(|v| v.len()).max().unwrap_or(20);
    let title = format!("  {}  ─  rank {}", label, rank);
    let bar_w = (key_w + 4 + val_w).max(title.len() + 2).max(40);
    let h_bar = "═".repeat(bar_w);
    let sep_bar = "─".repeat(bar_w);

    let mut out = String::new();
    out.push_str(&format!("{h_bar}\n{title}\n{h_bar}\n{sep_bar}\n"));
    for (i, header) in headers.iter().enumerate() {
        let val = row.get(i).map(|s| s.as_str()).unwrap_or("");
        out.push_str(&format!("  {:<kw$}  {}\n", header, val, kw = key_w));
    }
    out.push_str(&format!("{h_bar}\n"));
    out
}

fn render_to_png(text: &str, out_path: &std::path::Path) -> Result<()> {
    use ab_glyph::{Font as _, FontArc, PxScale, ScaleFont as _};
    use image::{Rgb, RgbImage};

    const BG: Rgb<u8> = Rgb([18, 18, 18]);
    const FG: Rgb<u8> = Rgb([204, 204, 204]);
    const ACCENT: Rgb<u8> = Rgb([97, 175, 239]);
    const BORDER_C: Rgb<u8> = Rgb([60, 60, 60]);
    const FONT_SIZE: f32 = 14.0;
    const PAD_X: i32 = 20;
    const PAD_Y: i32 = 20;

    let font_data = find_mono_font()
        .ok_or_else(|| anyhow!("no monospace font found; install ttf-dejavu or ttf-liberation"))?;
    let font =
        FontArc::try_from_vec(font_data).map_err(|_| anyhow!("failed to parse font file"))?;
    let scale = PxScale::from(FONT_SIZE);
    let sf = font.as_scaled(scale);

    let lines: Vec<&str> = text.lines().collect();
    let char_w = sf.h_advance(sf.glyph_id('W')).ceil() as i32;
    let line_h = ((sf.ascent() - sf.descent() + 2.0).ceil() as i32).max(18);
    let max_len = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(60)
        .max(60) as i32;
    let width = (PAD_X * 2 + char_w * max_len).max(200) as u32;
    let height = (PAD_Y * 2 + line_h * lines.len() as i32).max(60) as u32;

    let mut img = RgbImage::from_pixel(width, height, BG);
    for x in 0..width {
        img.put_pixel(x, 0, BORDER_C);
        img.put_pixel(x, height - 1, BORDER_C);
    }
    for y in 0..height {
        img.put_pixel(0, y, BORDER_C);
        img.put_pixel(width - 1, y, BORDER_C);
    }

    let box_set: std::collections::HashSet<char> = "═─╔╚╗╝║╠╣╦╩╪╫━╟╢╤╧│".chars().collect();
    for (i, line) in lines.iter().enumerate() {
        let y0 = PAD_Y + i as i32 * line_h;
        let color = if line.chars().any(|c| box_set.contains(&c)) {
            ACCENT
        } else {
            FG
        };
        draw_line(&mut img, &font, scale, PAD_X as f32, y0 as f32, line, color);
    }

    img.save(out_path)
        .map_err(|e| anyhow!("save failed: {e}"))?;
    Ok(())
}

fn draw_line(
    img: &mut image::RgbImage,
    font: &ab_glyph::FontArc,
    scale: ab_glyph::PxScale,
    x_start: f32,
    y_start: f32,
    text: &str,
    color: image::Rgb<u8>,
) {
    use ab_glyph::{point, Font as _, ScaleFont as _};
    let sf = font.as_scaled(scale);
    let baseline = y_start + sf.ascent();
    let mut cx = x_start;
    let (iw, ih) = img.dimensions();
    for ch in text.chars() {
        let gid = sf.glyph_id(ch);
        let glyph = gid.with_scale_and_position(scale, point(cx, baseline));
        if let Some(og) = font.outline_glyph(glyph) {
            let b = og.px_bounds();
            og.draw(|px, py, cov| {
                if cov < 0.01 {
                    return;
                }
                let ix = b.min.x as i32 + px as i32;
                let iy = b.min.y as i32 + py as i32;
                if ix < 0 || iy < 0 || ix >= iw as i32 || iy >= ih as i32 {
                    return;
                }
                let ix = ix as u32;
                let iy = iy as u32;
                let bg = *img.get_pixel(ix, iy);
                img.put_pixel(
                    ix,
                    iy,
                    image::Rgb([
                        (bg[0] as f32 * (1.0 - cov) + color[0] as f32 * cov) as u8,
                        (bg[1] as f32 * (1.0 - cov) + color[1] as f32 * cov) as u8,
                        (bg[2] as f32 * (1.0 - cov) + color[2] as f32 * cov) as u8,
                    ]),
                );
            });
        }
        cx += sf.h_advance(gid);
    }
}

fn find_mono_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/LiberationMono-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
        "/usr/share/fonts/TTF/Hack-Regular.ttf",
        "/usr/share/fonts/noto/NotoMono-Regular.ttf",
        "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
    ];
    CANDIDATES.iter().find_map(|p| std::fs::read(p).ok())
}
