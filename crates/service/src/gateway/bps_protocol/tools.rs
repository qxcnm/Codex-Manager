use super::{fingerprint, object, required_string, string, Bridge, ClientTool, ToolChoice};
use serde_json::{json, Value};

pub(super) fn is_call(item: &Value) -> bool {
    matches!(string(item, "type"), "function_call" | "custom_tool_call")
}

fn qualified_name(item: &Value) -> Result<String, String> {
    let name = required_string(item, "name")?;
    Ok(
        match item
            .get("namespace")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            Some(namespace) => format!("{namespace}.{name}"),
            None => name.to_owned(),
        },
    )
}

impl Bridge {
    pub(super) fn collect_tools(
        &mut self,
        declarations: &Value,
        namespace: Option<&str>,
        depth: usize,
    ) -> Result<(), String> {
        if declarations.is_null() {
            return Ok(());
        }
        if depth > 8 {
            return Err("BPS tool namespaces are nested too deeply".into());
        }
        let declarations = declarations
            .as_array()
            .ok_or("BPS tools must be an array")?;
        for item in declarations {
            object(item)?;
            match string(item, "type") {
                "namespace" => {
                    let name = required_string(item, "name")?;
                    let full = namespace
                        .map(|ns| format!("{ns}.{name}"))
                        .unwrap_or_else(|| name.to_owned());
                    self.collect_tools(&item["tools"], Some(&full), depth + 1)?;
                }
                "function" | "custom" => {
                    let declaration = item
                        .get("function")
                        .filter(|v| v.is_object())
                        .unwrap_or(item);
                    let name = required_string(declaration, "name")?.to_owned();
                    let namespace = namespace
                        .or_else(|| item.get("namespace").and_then(Value::as_str))
                        .map(str::to_owned);
                    let full = namespace
                        .as_ref()
                        .map(|ns| format!("{ns}.{name}"))
                        .unwrap_or_else(|| name.clone());
                    if self.tools.len() >= 1024 {
                        return Err("BPS tool catalog is too large".into());
                    }
                    let custom = string(item, "type") == "custom";
                    let mut catalog = declaration.clone();
                    let catalog_object = catalog
                        .as_object_mut()
                        .ok_or("invalid BPS tool declaration")?;
                    catalog_object.insert("name".into(), json!(full));
                    catalog_object.insert(
                        "type".into(),
                        json!(if custom { "custom" } else { "function" }),
                    );
                    if let Some(old) = self.tools.get(&full) {
                        if old.namespace != namespace || old.name != name || old.custom != custom {
                            return Err("BPS tool names must be unambiguous".into());
                        }
                    }
                    self.tools.insert(
                        full,
                        ClientTool {
                            name,
                            namespace,
                            custom,
                            declaration: catalog,
                        },
                    );
                }
                "web_search"
                | "web_search_preview"
                | "web_search_preview_2025_03_11"
                | "web_search_2025_08_26"
                | "image_generation"
                | "file_search"
                | "code_interpreter"
                | "computer_use_preview"
                | "computer"
                | "mcp" => {
                    let kind = string(item, "type").to_owned();
                    if !self.omitted_tools.contains(&kind) {
                        self.omitted_tools.push(kind);
                    }
                }
                _ => return Err(
                    "BPS does not support this tool declaration type; disable BPS for this request"
                        .into(),
                ),
            }
        }
        Ok(())
    }

    pub(super) fn set_choice(&mut self, choice: &Value) -> Result<(), String> {
        self.choice = match choice {
            Value::Null => ToolChoice::Auto,
            Value::String(choice) if choice == "auto" => ToolChoice::Auto,
            Value::String(choice) if choice == "none" => ToolChoice::None,
            Value::String(choice)
                if choice == "required"
                    && !self.tools.is_empty()
                    && self.omitted_tools.is_empty() =>
            {
                ToolChoice::Required
            }
            Value::Object(_) if matches!(string(choice, "type"), "function" | "custom") => {
                let named = choice
                    .get("function")
                    .filter(|v| v.is_object())
                    .unwrap_or(choice);
                let name = qualified_name(named)?;
                if !self.tools.contains_key(&name) {
                    return Err("BPS requested tool is unavailable".into());
                }
                ToolChoice::Named(name)
            }
            _ => return Err(
                "BPS cannot satisfy this tool_choice; use auto, none, or a declared client tool"
                    .into(),
            ),
        };
        Ok(())
    }

    pub(super) fn tool_prompt(&self) -> String {
        let mut prompt = String::from("You are responding to a CodexManager client through the BPS interface. This session has no Excel workbook. Answer the user's task as ordinary assistant text. Do not perform Excel, workbook, connector, or other hosted operations.");
        if !self.tools.is_empty() && !matches!(self.choice, ToolChoice::None) {
            let catalog: Vec<_> = self.tools.values().map(|tool| &tool.declaration).collect();
            prompt.push_str("\nThe following tools run in the client. To request a tool, use the native run_officejs function strictly as a message carrier. Its code field must be a JSON object serialized as a string, with the exact qualified tool name in tool. For function tools use {\"tool\":\"name\",\"arguments\":{...}}; for custom tools use {\"tool\":\"name\",\"input\":\"exact raw tool input\"}. Escape JSON strings correctly. Supply a short summary, extended_summary, destructive=false, and references=[]. CodexManager decodes this message and asks the client to run the named tool; no Office JavaScript is executed. Request one tool at a time, wait for its result, and then continue the task. Never invent a tool name or change a tool result. Client tool catalog:\n");
            prompt.push_str(&serde_json::to_string(&catalog).expect("catalog JSON"));
        }
        match &self.choice {
            ToolChoice::None => prompt.push_str("\nDo not call any tool in this response."),
            ToolChoice::Required => {
                prompt.push_str("\nThis response must request a declared client tool.")
            }
            ToolChoice::Named(name) => prompt.push_str(&format!(
                "\nThis response must request exactly the client tool {}.",
                serde_json::to_string(name).expect("tool name JSON")
            )),
            ToolChoice::Auto => {}
        }
        if !self.omitted_tools.is_empty() {
            prompt.push_str(&format!("\nThese hosted capabilities are unavailable on this route: {}. Tell the user when the task requires them; do not claim to have used them.", self.omitted_tools.join(", ")));
        }
        prompt
    }

    pub(super) fn translate_call(&self, item: &Value) -> Result<Value, String> {
        if matches!(self.choice, ToolChoice::None) {
            return Err("BPS returned a tool despite tool_choice=none".into());
        }
        if !matches!(
            string(item, "name"),
            "run_officejs" | "functions.run_officejs"
        ) {
            return Err("BPS returned an unexpected native tool".into());
        }
        let arguments: Value = match &item["arguments"] {
            Value::String(raw) => serde_json::from_str(raw)
                .map_err(|_| "BPS native tool arguments are invalid JSON")?,
            Value::Object(_) => item["arguments"].clone(),
            _ => return Err("BPS native tool arguments are missing".into()),
        };
        let code = required_string(&arguments, "code")?;
        let envelope: Value =
            serde_json::from_str(code).map_err(|_| "BPS client tool payload is invalid JSON")?;
        let name = required_string(&envelope, "tool")?;
        let tool = self
            .tools
            .get(name)
            .ok_or("BPS returned an undeclared client tool")?;
        if matches!(&self.choice, ToolChoice::Named(expected) if expected != name) {
            return Err("BPS returned a tool different from tool_choice".into());
        }
        let call_id = required_string(item, "call_id")?;
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("fc_{}", fingerprint(&call_id)));
        let mut output = json!({"id":id,"call_id":call_id,"type":if tool.custom {"custom_tool_call"} else {"function_call"},"name":tool.name,"status":"completed"});
        if let Some(namespace) = &tool.namespace {
            output["namespace"] = json!(namespace);
        }
        if tool.custom {
            output["input"] = json!(envelope
                .get("input")
                .and_then(Value::as_str)
                .ok_or("BPS custom tool input must be a string")?);
        } else {
            let arguments = envelope
                .get("arguments")
                .filter(|v| v.is_object())
                .ok_or("BPS client function arguments must be an object")?;
            output["arguments"] = json!(serde_json::to_string(arguments)
                .map_err(|_| "cannot encode BPS client arguments")?);
        }
        Ok(output)
    }
}

pub(super) fn history_call(item: &Value) -> Result<Value, String> {
    let name = qualified_name(item)?;
    let call_id = required_string(item, "call_id")?;
    let envelope = if string(item, "type") == "custom_tool_call" {
        json!({"tool":name,"input":item.get("input").and_then(Value::as_str).ok_or("BPS custom history input must be a string")?})
    } else {
        let arguments: Value = match &item["arguments"] {
            Value::String(raw) => serde_json::from_str(raw)
                .map_err(|_| "BPS historical function arguments are invalid JSON")?,
            Value::Object(_) => item["arguments"].clone(),
            _ => return Err("BPS historical function arguments are missing".into()),
        };
        if !arguments.is_object() {
            return Err("BPS historical function arguments must be an object".into());
        }
        json!({"tool":name,"arguments":arguments})
    };
    let outer = json!({"code":serde_json::to_string(&envelope).expect("tool envelope"),"summary":"CodexManager client tool","extended_summary":"Client tool request","destructive":false,"references":[]});
    Ok(
        json!({"type":"function_call","id":format!("fc_{}",fingerprint(&call_id)),"call_id":call_id,"name":"run_officejs","arguments":serde_json::to_string(&outer).expect("native arguments")}),
    )
}
