fn main() {
    // Rebuild when embedded frontend assets change.
    println!("cargo:rerun-if-changed=web/dist");
    // Keep builtin skills embedding in sync as well.
    println!("cargo:rerun-if-changed=finally_a_value_bot.data/skills");
}
