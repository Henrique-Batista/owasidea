wit_bindgen::generate!({ world: "greeter", path: ".owasidea/wit", generate_all });

#[owasidea::world("greeter")]
struct Component;

impl Guest for Component {
    #[owasidea::export]
    fn greet(name: String) -> String {
        format!("Hello, {name}! (from Rust)")
    }

    #[owasidea::export]
    fn sum(a: i32, b: i32) -> i32 {
        a.wrapping_add(b)
    }
}

export!(Component);
