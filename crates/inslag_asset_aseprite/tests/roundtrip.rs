use inslag_asset_aseprite::pack;
use inslag_asset_image::{Image, Sprite, SpriteCollection};
use inslag_asset_pipeline::Result;
use std::collections::BTreeMap;

fn sprite(name: &str, color: [u8; 4]) -> Sprite {
    Sprite {
        names: vec![name.into()],
        frames: vec![Image {
            width: 2,
            height: 2,
            rgba: color.repeat(4),
        }],
        pivot: [-2, 7],
        scale: 0.25,
        attributes: BTreeMap::new(),
        role: None,
    }
}

#[test]
fn independent_reader_preserves_pixels_aliases_pivots_and_animation_strips() -> Result<()> {
    let mut first = sprite("animated", [200, 30, 80, 255]);
    first.names.push("alias".into());
    first.frames.push(Image {
        width: 2,
        height: 2,
        rgba: [10, 250, 40, 128].repeat(4),
    });
    let second = sprite("second", [0, 0, 0, 0]);
    let packed = pack(&SpriteCollection(vec![first.clone(), second]), [16, 16])?;
    let file = asefile::AsepriteFile::read(std::io::Cursor::new(&packed.data))?;
    assert_eq!(file.num_frames(), 1);
    assert_eq!(file.slices().len(), 3);
    let canvas = file.frame(0).image();
    for slice in file.slices().iter().filter(|s| s.name != "second") {
        let key = &slice.keys[0];
        assert_eq!(key.size, (4, 2));
        assert_eq!(key.pivot, Some((-2, 7)));
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(
                    canvas
                        .get_pixel(key.origin.0 as u32 + x, key.origin.1 as u32 + y)
                        .0,
                    if x < 2 {
                        [200, 30, 80, 255]
                    } else {
                        [10, 250, 40, 128]
                    }
                );
            }
        }
    }
    assert_eq!(packed.bindings.len(), 3);
    assert_eq!(packed.bindings[0].frame_count, 2);
    assert_eq!(packed.bindings[0].frame_size, [2, 2]);
    assert_eq!(packed.bindings[0].scale, 0.25);
    Ok(())
}

#[test]
fn packing_rejects_duplicate_names_mixed_sizes_and_small_atlases() {
    let first = sprite("one", [255; 4]);
    assert!(
        pack(
            &SpriteCollection(vec![first.clone(), first.clone()]),
            [16, 16]
        )
        .is_err()
    );
    assert!(pack(&SpriteCollection(vec![first.clone()]), [1, 1]).is_err());
    let mut mixed = first;
    mixed.frames.push(Image {
        width: 1,
        height: 1,
        rgba: vec![255; 4],
    });
    assert!(pack(&SpriteCollection(vec![mixed]), [16, 16]).is_err());
    assert!(pack(&SpriteCollection(vec![]), [16, 16]).is_err());
}
