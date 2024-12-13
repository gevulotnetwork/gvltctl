fn main() -> shadow_rs::SdResult<()> {
    let mut deny = std::collections::BTreeSet::new();
    deny.insert(shadow_rs::CARGO_TREE);
    deny.insert(shadow_rs::CARGO_METADATA);
    shadow_rs::new_deny(deny)
}
