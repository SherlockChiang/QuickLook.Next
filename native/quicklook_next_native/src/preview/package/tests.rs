use std::fs;
use std::io::{Cursor, Write};

use image::{DynamicImage, Rgba, RgbaImage};

use super::{extract_package_icon_bgra, package_icon_candidate_score};

#[test]
fn package_icon_candidates_accept_arbitrary_android_mipmap_names() {
    assert!(package_icon_candidate_score("res/mipmap-xxxhdpi/product_mark.png") > 0);
    assert!(package_icon_candidate_score("base/res/mipmap-hdpi/brand_asset.webp") > 0);
    assert_eq!(
        package_icon_candidate_score("res/drawable/random_photo.png"),
        0
    );
    assert_eq!(
        package_icon_candidate_score("res/mipmap-anydpi-v26/product_mark.xml"),
        0
    );
}

#[test]
fn package_icon_resolves_manifest_adaptive_icon_layers() {
    let path = std::env::temp_dir().join(format!(
        "quicklook-next-adaptive-icon-{}.apk",
        std::process::id()
    ));
    let file = fs::File::create(&path).expect("create adaptive icon APK");
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("AndroidManifest.xml", options)
        .expect("start manifest");
    writer.write_all(br#"<manifest xmlns:android="http://schemas.android.com/apk/res/android"><application android:icon="@mipmap/product_mark"/></manifest>"#).expect("write manifest");
    writer
        .start_file("res/mipmap-anydpi-v26/product_mark.xml", options)
        .expect("start adaptive icon");
    writer.write_all(br##"<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android"><background android:drawable="#112233"/><foreground android:drawable="@drawable/product_foreground"/></adaptive-icon>"##).expect("write adaptive icon");
    let foreground =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 32, Rgba([20, 220, 40, 255])));
    let mut foreground_png = Cursor::new(Vec::new());
    foreground
        .write_to(&mut foreground_png, image::ImageFormat::Png)
        .expect("encode foreground");
    writer
        .start_file("res/drawable-xxxhdpi/product_foreground.png", options)
        .expect("start foreground");
    writer
        .write_all(foreground_png.get_ref())
        .expect("write foreground");
    let unrelated =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(256, 256, Rgba([240, 10, 10, 255])));
    let mut unrelated_png = Cursor::new(Vec::new());
    unrelated
        .write_to(&mut unrelated_png, image::ImageFormat::Png)
        .expect("encode unrelated image");
    writer
        .start_file("res/mipmap-xxxhdpi/unrelated.png", options)
        .expect("start unrelated image");
    writer
        .write_all(unrelated_png.get_ref())
        .expect("write unrelated image");
    writer.finish().expect("finish adaptive icon APK");

    let (width, height, bgra) =
        extract_package_icon_bgra(path.to_str().unwrap(), None).expect("extract adaptive icon");
    let _ = fs::remove_file(path);

    assert_eq!((width, height), (512, 512));
    let center = ((256 * width + 256) * 4) as usize;
    assert_eq!(&bgra[center..center + 4], &[40, 220, 20, 255]);
}
