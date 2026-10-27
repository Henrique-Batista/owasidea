wit_bindgen::generate!({ world: "{{name}}", path: ".owasidea/wit", generate_all });

#[owasidea::world("{{name}}")]
struct Component;

impl Guest for Component {
    #[owasidea::export]
    fn greet(name: String) -> String {
        format!("Hello, {name}! (from Rust)")
    }
}

export!(Component);
