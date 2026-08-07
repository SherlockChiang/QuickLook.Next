use std::collections::BTreeSet;

use image::{Rgba, RgbaImage};

use super::{
    android_typed_value, mask_android_adaptive_icon, render_android_vector,
    resolve_android_resource_values, AndroidResourceValue,
};

#[test]
fn android_resource_table_resolves_obfuscated_icon_path() {
    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn string_pool(values: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for value in values {
            offsets.push(data.len() as u32);
            data.push(value.len() as u8);
            data.push(value.len() as u8);
            data.extend_from_slice(value.as_bytes());
            data.push(0);
        }
        while data.len() % 4 != 0 {
            data.push(0);
        }
        let header_size = 28usize;
        let size = header_size + offsets.len() * 4 + data.len();
        let mut pool = vec![0; size];
        put_u16(&mut pool, 0, 0x0001);
        put_u16(&mut pool, 2, header_size as u16);
        put_u32(&mut pool, 4, size as u32);
        put_u32(&mut pool, 8, values.len() as u32);
        put_u32(&mut pool, 16, 0x100);
        put_u32(&mut pool, 20, (header_size + offsets.len() * 4) as u32);
        for (index, offset) in offsets.into_iter().enumerate() {
            put_u32(&mut pool, header_size + index * 4, offset);
        }
        pool[header_size + values.len() * 4..].copy_from_slice(&data);
        pool
    }

    let global = string_pool(&["res/9w.png"]);
    let types = string_pool(&["mipmap"]);
    let keys = string_pool(&["product_mark"]);
    let mut type_chunk = vec![0; 48];
    put_u16(&mut type_chunk, 0, 0x0201);
    put_u16(&mut type_chunk, 2, 28);
    put_u32(&mut type_chunk, 4, 48);
    type_chunk[8] = 1;
    put_u32(&mut type_chunk, 12, 1);
    put_u32(&mut type_chunk, 16, 32);
    put_u32(&mut type_chunk, 20, 8);
    put_u32(&mut type_chunk, 28, 0);
    put_u16(&mut type_chunk, 32, 8);
    put_u32(&mut type_chunk, 36, 0);
    put_u16(&mut type_chunk, 40, 8);
    type_chunk[43] = 0x03;
    put_u32(&mut type_chunk, 44, 0);
    let package_size = 288 + types.len() + keys.len() + type_chunk.len();
    let mut package = vec![0; 288];
    put_u16(&mut package, 0, 0x0200);
    put_u16(&mut package, 2, 288);
    put_u32(&mut package, 4, package_size as u32);
    put_u32(&mut package, 268, 288);
    put_u32(&mut package, 276, (288 + types.len()) as u32);
    package.extend_from_slice(&types);
    package.extend_from_slice(&keys);
    package.extend_from_slice(&type_chunk);
    let table_size = 12 + global.len() + package.len();
    let mut table = vec![0; 12];
    put_u16(&mut table, 0, 0x0002);
    put_u16(&mut table, 2, 12);
    put_u32(&mut table, 4, table_size as u32);
    put_u32(&mut table, 8, 1);
    table.extend_from_slice(&global);
    table.extend_from_slice(&package);

    let values = resolve_android_resource_values(&table, "@mipmap/product_mark");
    assert!(
        matches!(values.as_slice(), [AndroidResourceValue::Path(path)] if path == "res/9w.png")
    );
}

#[test]
fn android_vector_groups_render_transformed_foreground() {
    assert_eq!(
        android_typed_value(0x04, 0.135_f32.to_bits(), &[], None).as_deref(),
        Some("0.135")
    );
    let image = render_android_vector(
        r##"<vector android:viewportWidth="108" android:viewportHeight="108">
            <group android:scaleX="0.5" android:scaleY="0.5" android:translateX="27" android:translateY="27">
                <path android:fillColor="#ff336ab6" android:pathData="M0,0 H108 V108 H0 Z"/>
                <path android:fillColor="#ffffffff" android:pathData="M27,27 H81 V81 H27 Z"/>
            </group>
        </vector>"##,
    ).expect("render grouped Android vector").to_rgba8();
    let colors = image.pixels().map(|pixel| pixel.0).collect::<BTreeSet<_>>();

    assert!(
        colors.len() > 2,
        "grouped vector should include foreground and antialiased edges"
    );
    assert!(image.get_pixel(256, 256).0[3] > 0);
}

#[test]
fn android_adaptive_icon_crops_safe_zone_and_masks_background() {
    let mut source = RgbaImage::from_pixel(108, 108, Rgba([20, 40, 60, 255]));
    for y in 45..63 {
        for x in 45..63 {
            source.put_pixel(x, y, Rgba([240, 180, 20, 255]));
        }
    }

    let output = mask_android_adaptive_icon(source);

    assert_eq!(output.get_pixel(0, 0).0[3], 0);
    assert_eq!(output.get_pixel(511, 0).0[3], 0);
    assert_eq!(output.get_pixel(256, 256).0, [240, 180, 20, 255]);
    assert!(output.get_pixel(256, 4).0[3] > 0);
}
