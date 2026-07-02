use ecsh::specs::all_specs;

fn main() {
    let json = serde_json::to_string_pretty(all_specs()).expect("serialize specs");
    println!("{json}");
}
