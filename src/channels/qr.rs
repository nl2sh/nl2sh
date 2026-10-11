//! Locally generated QQ invitation QR; never sends invitation data to a QR service.
use anyhow::Result;
use qrcode::{Color, QrCode};
pub(crate) fn invitation(app_id: &str, uin: &str) -> Option<String> {
    if app_id.is_empty()
        || uin.is_empty()
        || app_id.len() > 64
        || uin.len() > 20
        || !app_id.bytes().all(|b| b.is_ascii_digit())
        || !uin.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some(format!(
        "https://qun.qq.com/qunpro/robot/qunshare?robot_appid={app_id}&robot_uin={uin}"
    ))
}
pub(crate) fn svg(url: &str) -> Result<String> {
    let qr = QrCode::new(url)?;
    let width = qr.width();
    let size = width + 8;
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if qr[(x, y)] == Color::Dark {
                path.push_str(&format!("M{} {}h1v1h-1z", x + 4, y + 4));
            }
        }
    }
    Ok(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" shape-rendering="crispEdges"><rect width="100%" height="100%" fill="#fff"/><path d="{path}" fill="#000"/></svg>"##
    ))
}
pub(crate) fn text(url: &str) -> Result<Vec<String>> {
    let qr = QrCode::new(url)?;
    let width = qr.width() + 8;
    let dark = |x: usize, y: usize| {
        x >= 4 && y >= 4 && x < width - 4 && y < width - 4 && qr[(x - 4, y - 4)] == Color::Dark
    };
    Ok((0..width)
        .step_by(2)
        .map(|y| {
            (0..width)
                .map(|x| match (!dark(x, y), !dark(x, y + 1)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    _ => ' ',
                })
                .collect()
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invitation_is_exact_and_svg_contains_only_local_geometry() -> Result<()> {
        let url = invitation("102719228", "3889625032")
            .ok_or_else(|| anyhow::anyhow!("fixture URL missing"))?;
        assert!(url.contains("robot_appid=102719228&robot_uin=3889625032"));
        assert!(invitation("102", "javascript:alert(1)").is_none());
        let svg = svg(&url)?;
        assert!(svg.starts_with("<svg "));
        assert!(!svg.contains("script"));
        assert!(!svg.contains("https://"));
        assert!(text(&url)?.len() > 10);
        Ok(())
    }
}
