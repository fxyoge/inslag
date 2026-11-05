fn main() {
    inslag_kui_workbench::workbench_app(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../inslag_kui/assets"),
    )
    .run();
}
