wit_bindgen::generate!({
    world: "notes-client",
    path: "wit",
    generate_all,
});

struct Client;

impl Guest for Client {
    fn run(name: String) -> String {
        let g = poc::notes::api::greet(&name);
        format!("client got: {}", g)
    }
}

export!(Client);