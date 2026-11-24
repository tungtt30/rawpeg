use std::fs;
use std::path::PathBuf;

fn read(relative: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read(&path).unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

#[test]
fn source_icon_is_1024_png() {
    let bytes = read("assets/icon-source.png");
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!((image.width(), image.height()), (1024, 1024));
}

#[test]
fn runtime_icon_is_decodable_256_png() {
    let bytes = read("assets/rawpeg-256.png");
    let image = eframe::icon_data::from_png_bytes(&bytes).unwrap();
    assert_eq!((image.width, image.height), (256, 256));
    assert_eq!(image.rgba.len(), 256 * 256 * 4);
}

#[test]
fn windows_ico_has_expected_png_entries() {
    let bytes = read("assets/rawpeg.ico");
    assert_eq!(&bytes[..4], &[0, 0, 1, 0]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    assert_eq!(count, 6);

    let mut sizes = Vec::new();
    for index in 0..count {
        let entry = 6 + index * 16;
        let width = bytes[entry] as u32;
        let height = bytes[entry + 1] as u32;
        let length = u32::from_le_bytes(bytes[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(bytes[entry + 12..entry + 16].try_into().unwrap()) as usize;

        assert_eq!(width, height);
        assert!(offset + length <= bytes.len());
        assert_eq!(&bytes[offset..offset + 8], b"\x89PNG\r\n\x1a\n");
        sizes.push(if width == 0 { 256 } else { width });
    }
    assert_eq!(sizes, vec![16, 32, 48, 64, 128, 256]);
}

#[test]
fn macos_icns_has_magic_header() {
    let bytes = read("assets/rawpeg.icns");
    assert_eq!(&bytes[..4], b"icns");
    assert!(bytes.len() > 1024);
}

#[test]
fn windows_resource_and_macos_plist_reference_icons() {
    let resource = String::from_utf8(read("rawpeg.rc")).unwrap();
    assert!(resource.contains("IDI_ICON1 ICON"));
    assert!(resource.contains("assets/rawpeg.ico"));

    let plist = String::from_utf8(read(".github/macos/Info.plist")).unwrap();
    assert!(plist.contains("<key>CFBundleIconFile</key>"));
    assert!(plist.contains("<string>rawpeg.icns</string>"));
}
