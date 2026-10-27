wit_bindgen::generate!({ world: "{{name}}", path: "wit", generate_all });

struct Component;
impl Guest for Component {
    fn greet(name: String) -> String {
        format!("Hello, {}! (from Rust)", name)
    }
}
export!(Component);
