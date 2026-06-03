use serde_json::{json, Value};

/// Render `{{ dotted.path }}` placeholders using a JSON execution context.
pub fn render_template(template: &str, ctx: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        let Some(end) = rest.find("}}") else {
            out.push_str("{{");
            out.push_str(rest);
            return out;
        };
        let key = rest[..end].trim();
        let replacement = lookup_path(ctx, key)
            .map(value_to_string)
            .unwrap_or_default();
        out.push_str(&replacement);
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

pub fn render_json_value(value: &Value, ctx: &Value) -> Result<Value, String> {
    match value {
        Value::String(s) => Ok(Value::String(render_template(s, ctx))),
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(render_json_value(item, ctx)?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), render_json_value(v, ctx)?);
            }
            Ok(Value::Object(out))
        }
        _ => Ok(value.clone()),
    }
}

fn lookup_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        current = match current {
            Value::Object(map) => map.get(part)?,
            _ => return None,
        };
    }
    Some(current)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn default_inputs(
    def_inputs: &std::collections::BTreeMap<String, super::schema::WorkflowInputDef>,
) -> Value {
    let mut map = serde_json::Map::new();
    for (name, spec) in def_inputs {
        if let Some(default) = &spec.default {
            map.insert(name.clone(), default.clone());
        } else {
            map.insert(name.clone(), json!(null));
        }
    }
    Value::Object(map)
}

pub fn merge_inputs(defaults: &Value, overrides: &Value) -> Value {
    let mut base = match defaults {
        Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    if let Value::Object(over) = overrides {
        for (k, v) in over {
            base.insert(k.clone(), v.clone());
        }
    }
    Value::Object(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_dotted_path() {
        let ctx = json!({
            "inputs": { "days": 2 },
            "steps": { "fetch": { "stdout": "body" } }
        });
        assert_eq!(render_template("days={{ inputs.days }}", &ctx), "days=2");
        assert_eq!(render_template("{{ steps.fetch.stdout }}", &ctx), "body");
    }
}
