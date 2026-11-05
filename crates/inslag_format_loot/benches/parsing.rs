fn main() {
    divan::main();
}

#[divan::bench]
fn composition() {
    let input = include_str!("../tests/fixtures/composition.loot");
    divan::black_box(inslag_format_loot::parse(divan::black_box(input)).unwrap());
}

#[divan::bench(args = [100, 1_000])]
fn many_tables(bencher: divan::Bencher, count: usize) {
    let input = (0..count)
        .map(|index| {
            format!("table \"t{index}\" using=actor\n  slime role=leader\n  #escort count=1..3\n")
        })
        .collect::<String>();
    bencher.bench(|| inslag_format_loot::parse(divan::black_box(&input)).unwrap());
}
