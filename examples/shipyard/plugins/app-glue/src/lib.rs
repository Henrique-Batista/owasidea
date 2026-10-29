wit_bindgen::generate!({
    world: "glue",
    path: "wit",
    generate_all,
});

use serde_json::{json, Value};

struct Glue;

fn args(args_json: &str) -> Result<Vec<Value>, String> {
    serde_json::from_str(args_json).map_err(|error| format!("invalid args JSON: {error}"))
}

fn string_arg(args: &[Value], index: usize) -> Result<String, String> {
    args.get(index)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("argument {index} must be a string"))
}

fn strings_arg(args: &[Value], index: usize) -> Result<Vec<String>, String> {
    args.get(index)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("argument {index} must be a string list"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("argument {index} must contain strings"))
        })
        .collect()
}

fn u32_arg(args: &[Value], index: usize) -> Result<u32, String> {
    args.get(index)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("argument {index} must be a u32"))
}

fn i32_arg(args: &[Value], index: usize) -> Result<i32, String> {
    args.get(index)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("argument {index} must be an i32"))
}

fn json_string(value: Value) -> Result<String, String> {
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

fn dispatch(plugin: &str, function: &str, args: &[Value]) -> Result<Value, String> {
    match (plugin, function) {
        ("core-rust", "score") => Ok(json!(shipyard::core::readiness::score(
            &strings_arg(args, 0)?,
            u32_arg(args, 1)?,
            u32_arg(args, 2)?
        ))),
        ("core-rust", "verdict") => Ok(json!(shipyard::core::readiness::verdict(i32_arg(
            args, 0
        )?))),
        ("core-rust", "save-report") => shipyard::core::readiness::save_report(
            &string_arg(args, 0)?,
            &string_arg(args, 1)?,
        )
        .map(|path| json!(path)),
        ("core-rust", "load-report") => {
            Ok(json!(shipyard::core::readiness::load_report(&string_arg(args, 0)?)))
        }
        ("audit-go", "scan") => Ok(json!(shipyard::audit::api::scan(&strings_arg(args, 0)?))),
        ("changelog-ts", "render") => Ok(json!(shipyard::changelog::api::render(
            &string_arg(args, 0)?,
            &strings_arg(args, 1)?
        ))),
        ("policy-csharp", "classify") => Ok(json!(shipyard::policy::api::classify(i32_arg(
            args, 0
        )?))),
        ("policy-csharp", "next-version") => Ok(json!(shipyard::policy::api::next_version(
            &string_arg(args, 0)?,
            &string_arg(args, 1)?
        ))),
        ("policy-csharp", "summarize") => Ok(json!(shipyard::policy::api::summarize(
            &string_arg(args, 0)?,
            i32_arg(args, 1)?
        ))),
        ("fingerprint-c", "fingerprint") => Ok(json!(shipyard::fingerprint::api::fingerprint(
            &string_arg(args, 0)?
        ))),
        ("fingerprint-c", "verify") => Ok(json!(shipyard::fingerprint::api::verify(
            &string_arg(args, 0)?,
            &string_arg(args, 1)?
        ))),
        ("metrics-cpp", "analyze") => Ok(json!(shipyard::metrics::api::analyze(&string_arg(
            args, 0
        )?))),
        ("pipeline-go", "run") => Ok(json!(shipyard::pipeline::api::run(
            &string_arg(args, 0)?,
            &strings_arg(args, 1)?,
            u32_arg(args, 2)?,
            u32_arg(args, 3)?
        ))),
        _ => Err(format!("glue does not expose {plugin}.{function}")),
    }
}

fn service_state_json(state: poc::host::service_control::ServiceState) -> Value {
    json!({
        "running": state.running,
        "address": state.address,
        "last_error": state.last_error,
        "restarts": state.restarts,
    })
}

impl Guest for Glue {
    fn call(plugin: String, function: String, args_json: String) -> Result<String, String> {
        let parsed = args(&args_json)?;
        json_string(dispatch(&plugin, &function, &parsed)?)
    }

    fn service(plugin: String, action: String) -> Result<String, String> {
        let value = match action.as_str() {
            "start" => json!(poc::host::service_control::start(&plugin)?),
            "stop" => {
                poc::host::service_control::stop(&plugin)?;
                json!({"stopped": true})
            }
            "status" => service_state_json(poc::host::service_control::status(&plugin)),
            other => return Err(format!("unknown service action {other:?}")),
        };
        json_string(value)
    }
}

export!(Glue);
