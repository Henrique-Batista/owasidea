wit_bindgen::generate!({ world: "greeter", path: "wit", generate_all });

struct Component;

impl Guest for Component {
    fn greet(name: String) -> String {
        format!("Hello, {name}! (from Rust)")
    }

    fn sum(a: i32, b: i32) -> i32 {
        a.wrapping_add(b)
    }
}

export!(Component);
