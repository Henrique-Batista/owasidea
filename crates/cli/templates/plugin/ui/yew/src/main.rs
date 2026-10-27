use yew::prelude::*;

#[function_component(App)]
fn app() -> Html {
    let host = js_sys::Reflect::get(&js_sys::global(), &"__OWASIDEA_HOST__".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    html! {
        <main>
            <h1>{{name}}</h1>
            <p>{ "OWASIDEA host: " }{ host }</p>
        </main>
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
