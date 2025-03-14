use shadow_rs::{BuildPattern, ShadowBuilder};

fn main() {
    let mut deny = std::collections::BTreeSet::new();
    deny.insert(shadow_rs::CARGO_TREE);
    ShadowBuilder::builder()
        .build_pattern(BuildPattern::Lazy)
        .deny_const(deny)
        .build()
        .expect("failed to retrieve build info");
}
