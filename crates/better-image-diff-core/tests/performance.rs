use better_image_diff_core::{CompareOptions, compare};
use image::{Rgba, RgbaImage};

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for pixel_y in y..y + height {
        for pixel_x in x..x + width {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

#[test]
#[ignore = "release-mode 1920x1080 smoke coverage"]
fn generated_full_hd_ui_completes_with_default_search_radius() {
    let background = Rgba([242, 244, 248, 255]);
    let mut expected = RgbaImage::from_pixel(1920, 1080, background);
    let mut actual = RgbaImage::from_pixel(1920, 1080, background);
    for row in 0..4 {
        for column in 0..6 {
            let x = 80 + column * 290;
            let y = 70 + row * 240;
            let fill = Rgba([
                210 + u8::try_from(column * 4).expect("red"),
                220 + u8::try_from(row * 5).expect("green"),
                245,
                255,
            ]);
            fill_rect(&mut expected, x, y, 240, 170, Rgba([35, 45, 65, 255]));
            fill_rect(&mut actual, x, y, 240, 170, Rgba([35, 45, 65, 255]));
            fill_rect(&mut expected, x + 2, y + 2, 236, 166, fill);
            fill_rect(&mut actual, x + 2, y + 2, 236, 166, fill);
        }
    }
    fill_rect(&mut actual, 952, 552, 236, 166, background);
    fill_rect(&mut actual, 959, 552, 236, 166, Rgba([226, 230, 245, 255]));

    let comparison = compare(&expected, &actual, &CompareOptions::default()).expect("comparison");

    assert!(!comparison.equivalent);
}
