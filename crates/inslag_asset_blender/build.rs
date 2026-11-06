fn main() {
    inslag_asset_pipeline::fingerprint_package_with_dependencies(&[
        inslag_asset_image::IMPLEMENTATION,
    ]);
}
