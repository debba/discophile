//! The selected Connect picture, kept in memory and cleared with the account.

use std::io::Cursor;

use discophile_core::connect::ConnectClient;
use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::app::App;
use crate::tasks::update;

#[derive(Default)]
pub struct ProfilePicture {
    pub texture: Option<TextureHandle>,
    request: u64,
}

pub fn refresh(app: &mut App) {
    app.connect.profile.request += 1;
    app.connect.profile.texture = None;
    let request = app.connect.profile.request;
    let generation = app.connect.generation;
    let Some(url) = app
        .connect
        .account
        .as_ref()
        .and_then(|account| account.account.avatar_url(&app.connect.origin))
    else {
        return;
    };
    let Some(credential) = app.connect.credential.clone() else {
        return;
    };
    let Some(info) = app.connect.server_info.clone() else {
        return;
    };
    app.tasks.spawn(async move {
        let image = ConnectClient::new()
            .avatar(&info, &credential, &url)
            .await
            .ok()
            .and_then(|bytes| decode(&bytes));
        update(move |app: &mut App| {
            if app.connect.generation != generation || app.connect.profile.request != request {
                return;
            }
            app.connect.profile.texture = image.map(|image| {
                app.ctx()
                    .load_texture("connect-profile-picture", image, TextureOptions::LINEAR)
            });
        })
    });
}

fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let edge = image.width().min(image.height());
    if edge == 0 {
        return None;
    }
    let square = image
        .crop_imm(
            (image.width() - edge) / 2,
            (image.height() - edge) / 2,
            edge,
            edge,
        )
        .resize_exact(128, 128, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [128, 128],
        square.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(image: image::RgbaImage) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn profile_picture_is_center_cropped_and_bounded_before_decoding() {
        let image = image::RgbaImage::from_fn(200, 100, |x, _| {
            if (50..150).contains(&x) {
                image::Rgba([0, 0, 255, 255])
            } else {
                image::Rgba([255, 0, 0, 255])
            }
        });
        let picture = decode(&png(image)).unwrap();
        assert_eq!(picture.size, [128, 128]);
        assert!(
            picture
                .pixels
                .iter()
                .all(|pixel| *pixel == egui::Color32::BLUE)
        );
        assert!(decode(b"not an image").is_none());
        assert!(decode(&png(image::RgbaImage::new(4097, 1))).is_none());
    }
}
