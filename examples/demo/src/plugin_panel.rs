//! Host-owned extension controls; the library never dictates these divs or translations.
use crate::Language;
use dioxus::prelude::*;
use dioxus_map_editor::{
    command::Command,
    document::*,
    editor::{EditorTool, EditorTools},
    plugin::{resource, PluginHost},
    plugins::{
        formats::ImportCandidate,
        objects::{edit_compass, Compass},
    },
    registry::Registry,
    session::MapSession,
    ui::MapCanvasHandle,
};

#[component]
pub(crate) fn PluginPanel(
    mut session: Signal<MapSession>,
    mut tools: Signal<EditorTools>,
    mut registry: Signal<Registry>,
    mut plugins: Signal<PluginHost>,
    handle: Signal<Option<MapCanvasHandle>>,
    language: Language,
) -> Element {
    let mut message = use_signal(String::new);
    let mut importing = use_signal(|| false);
    let mut raw = use_signal(String::new);
    let mut candidate = use_signal(|| None::<ImportCandidate>);
    let mut download = use_signal(|| None::<dioxus_map_editor::plugin::ExportData>);
    let providers = plugins.read().plugins();
    let selected = tools.read().selection.clone();
    let compass=session.read().snapshot().document.objects.iter().enumerate().find(|(_,o)|selected.contains(&o.id)&&matches!(&o.geometry,Geometry::Custom{object_type,..}if *object_type==resource("official:compass"))).map(|(i,o)|(i,o.clone()));
    rsx! {
        section {
            class: "studio-extensions",
            "aria-label": language.text("Plugin manager", "插件管理"),
            h3 { {language.text("Extensions", "扩展")} }
            for (info, enabled) in providers {
                {
                    let id = info.id.clone();
                    let label = match id.0.as_str() {
                        "official:formats" => language.text("File formats", "文件格式"),
                        "official:brushes" => language.text("Foliage brush", "绿植画笔"),
                        "official:objects" => language.text("Compass rose", "罗盘对象"),
                        "official:map" => language.text("Paper atlas pack", "纸绘资源包"),
                        _ => &info.name,
                    }
                        .to_string();
                    rsx! {
                        button {
                            "aria-label": format!("{} {}", language.text("Enable plugin", "启用插件"), id.0),
                            "aria-pressed": enabled,
                            onclick: move |_| {
                                if let Some(h) = handle() {
                                    h.cancel();
                                }
                                let result = plugins
                                    .write()
                                    .set_enabled(&id, !enabled, &mut session.write(), &mut tools.write());
                                match result {
                                    Ok(()) => {
                                        registry.set(plugins.peek().registry().clone());
                                        message
                                            .set(
                                                if enabled {
                                                    {
                                                        language
                                                            .text(
                                                                "Disabled; map content is retained",
                                                                "已停用，保留现有地图内容",
                                                            )
                                                    }
                                                } else {
                                                    { language.text("Enabled", "已启用") }
                                                }
                                                    .into(),
                                            );
                                    }
                                    Err(e) => message.set(e.to_string()),
                                }
                            },
                            if enabled {
                                "● "
                            } else {
                                "○ "
                            }
                            "{label}"
                        }
                    }
                }
            }
            if registry.read().brush(&resource("official:foliage")).is_some() {
                button {
                    onclick: move |_| {
                        if let Some(h) = handle() {
                            h.cancel();
                        }
                        let mut t = tools.write();
                        t.tool = EditorTool::Brush;
                        t.brush = Some(resource("official:foliage"));
                        t.stamp = resource("builtin:pine");
                        t.paint = Paint {
                            role: "vegetation".into(),
                            ..Paint::default()
                        };
                        message
                            .set(
                                language
                                    .text(
                                        "Plugin brush: drag to plant, Esc to cancel",
                                        "插件画笔：拖动种植，Esc 取消",
                                    )
                                    .into(),
                            );
                    },
                    {language.text("Use foliage brush", "使用插件画笔")}
                }
            }
            if registry.read().action("official:add-compass").is_some() {
                button {
                    onclick: move |_| {
                        if let Some(h) = handle() {
                            h.cancel();
                        }
                        let commands = {
                            let s = session.peek();
                            let f = s.snapshot().document.frame;
                            registry
                                .peek()
                                .action("official:add-compass")
                                .ok_or_else(|| dioxus_map_editor::error::MapError::Missing(
                                    "official:add-compass".into(),
                                ))
                                .and_then(|a| {
                                    a
                                        .commands(
                                            s.snapshot(),
                                            &serde_json::json!(
                                                { "x" : f.min.x + f.width * 0.5, "y" : f.min.y + f.height *
                                                0.5 }
                                            ),
                                        )
                                })
                        };
                        match commands {
                            Ok(commands) => {
                                let id = commands
                                    .iter()
                                    .find_map(|c| {
                                        if let Command::PutObject { object, .. } = c {
                                            Some(object.id.clone())
                                        } else {
                                            None
                                        }
                                    });
                                let result = session.write().execute(commands);
                                match result {
                                    Ok(_) => {
                                        let mut t = tools.write();
                                        t.tool = EditorTool::Select;
                                        t.brush = None;
                                        t.selection = id.into_iter().collect();
                                        message
                                            .set(
                                                language
                                                    .text(
                                                        "Compass added. Drag or edit its properties.",
                                                        "已添加罗盘，拖动或调整属性",
                                                    )
                                                    .into(),
                                            );
                                    }
                                    Err(e) => message.set(e.to_string()),
                                }
                            }
                            Err(e) => message.set(e.to_string()),
                        }
                    },
                    {language.text("Add compass", "添加罗盘")}
                }
            }
            if let Some((index, object)) = compass {
                if registry.read().object_codec(&resource("official:compass")).is_some() {
                    {
                        let value = if let Geometry::Custom { payload, .. } = &object.geometry {
                            serde_json::from_value::<Compass>(*payload.clone()).ok()
                        } else {
                            None
                        };
                        rsx! {
                            if let Some(value) = value {
                                label { class: "studio-field",
                                    span { {language.text("Compass radius", "罗盘半径")} }
                                    input {
                                        r#type: "number",
                                        min: "4",
                                        max: "200",
                                        "aria-label": language.text("Compass radius", "罗盘半径"),
                                        value: value.radius,
                                        onchange: move |e| {
                                            if let Ok(radius) = e.value().parse::<f64>() {
                                                let result = edit_compass(
                                                        &object,
                                                        Compass {
                                                            radius,
                                                            inset: value.inset,
                                                        },
                                                    )
                                                    .and_then(|object| {
                                                        session
                                                            .write()
                                                            .execute(
                                                                vec![
                                                                    Command::PutObject {
                                                                        object,
                                                                        index,
                                                                    },
                                                                ],
                                                            )
                                                    });
                                                if let Err(e) = result {
                                                    message.set(e.to_string());
                                                }
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if registry.read().format(&resource("official:json")).is_some() {
                button {
                    onclick: move |_| {
                        importing.set(true);
                        candidate.set(None);
                    },
                    {language.text("Import JSON", "导入 JSON")}
                }
                button {
                    onclick: move |_| {
                        let s = session.peek();
                        let result = registry
                            .peek()
                            .format(&resource("official:json"))
                            .ok_or_else(|| dioxus_map_editor::error::MapError::Missing(
                                "official:json".into(),
                            ))
                            .and_then(|f| f.encode(s.snapshot(), &registry.peek(), &s.limits()));
                        match result {
                            Ok(data) => download.set(Some(data)),
                            Err(e) => message.set(e.to_string()),
                        }
                    },
                    {language.text("Export JSON", "导出 JSON")}
                }
            }
            if registry.read().has_style(&resource("official:atlas")) {
                button {
                    onclick: move |_| {
                        if let Some(h) = handle() {
                            h.cancel();
                        }
                        dioxus_map_editor::plugins::map::apply_preset(
                            dioxus_map_editor::map::MapPreset::Road,
                            &mut tools.write(),
                        );
                    },
                    {language.text("Pack road", "资源包道路")}
                }
                button {
                    onclick: move |_| {
                        if let Some(h) = handle() {
                            h.cancel();
                        }
                        dioxus_map_editor::plugins::map::apply_forest(&mut tools.write());
                    },
                    {language.text("Pack forest", "资源包树林")}
                }
                button {
                    onclick: move |_| {
                        if let Some(h) = handle() {
                            h.execute(vec![Command::SetStyle(resource("official:atlas"))]);
                        }
                    },
                    {language.text("Apply atlas style", "使用资源包风格")}
                }
            }
            p { role: "status", "{message}" }
            if importing() && registry.read().format(&resource("official:json")).is_some() {
                div { class: "studio-dialog-backdrop",
                    div {
                        class: "studio-dialog",
                        role: "dialog",
                        "aria-modal": "true",
                        "aria-label": language.text("Import JSON map", "导入 JSON 地图"),
                        h2 { {language.text("Import JSON", "导入 JSON")} }
                        p {
                            {
                                language
                                    .text(
                                        "Save your changes first. Import is undoable and retains the target map ID. Host bindings need their original host.",
                                        "先保存当前更改。导入保留当前地图 ID，可撤销；宿主地点绑定需要原宿主提供。",
                                    )
                            }
                        }
                        input {
                            r#type: "file",
                            accept: ".json,application/json",
                            "aria-label": language.text("Choose map JSON file", "选择地图 JSON 文件"),
                            onchange: move |e| async move {
                                if let Some(file) = e.files().first() {
                                    if file.size() > session.peek().limits().max_bytes as u64 {
                                        message
                                            .set(
                                                language
                                                    .text(
                                                        "File exceeds the import size limit",
                                                        "文件超过导入大小限制",
                                                    )
                                                    .into(),
                                            );
                                        return;
                                    }
                                    match file.read_string().await {
                                        Ok(text) => {
                                            raw.set(text);
                                            candidate.set(None);
                                        }
                                        Err(error) => message.set(error.to_string()),
                                    }
                                }
                            },
                        }
                        textarea {
                            "aria-label": language.text("Map JSON source", "地图 JSON 内容"),
                            value: raw(),
                            oninput: move |e| {
                                raw.set(e.value());
                                candidate.set(None);
                            },
                        }
                        button {
                            onclick: move |_| {
                                match ImportCandidate::parse(&raw(), &session.peek(), &registry.peek()) {
                                    Ok(value) => {
                                        message
                                            .set(
                                                format!(
                                                    "{} {}",
                                                    language.text("Valid objects:", "有效对象数："),
                                                    value.document().objects.len(),
                                                ),
                                            );
                                        candidate.set(Some(value));
                                    }
                                    Err(e) => {
                                        message.set(e.to_string());
                                        candidate.set(None);
                                    }
                                }
                            },
                            {language.text("Validate import", "校验导入")}
                        }
                        if let Some(value) = candidate() {
                            p {
                                {
                                    format!(
                                        "{} {}",
                                        language.text("Objects to import:", "准备导入对象数："),
                                        value.document().objects.len(),
                                    )
                                }
                            }
                            button {
                                onclick: move |_| {
                                    if let Some(h) = handle() {
                                        h.cancel();
                                    }
                                    match value.clone().apply(&mut session.write()) {
                                        Ok(()) => {
                                            candidate.set(None);
                                            importing.set(false);
                                            if let Some(h) = handle() {
                                                h.fit_frame();
                                            }
                                            message
                                                .set(
                                                    language
                                                        .text(
                                                            "Imported. Undo is available.",
                                                            "已导入，可撤销",
                                                        )
                                                        .into(),
                                                );
                                        }
                                        Err(e) => message.set(e.to_string()),
                                    }
                                },
                                {language.text("Confirm import", "确认导入")}
                            }
                        }
                        p { role: "status", "{message}" }
                        button {
                            onclick: move |_| {
                                candidate.set(None);
                                importing.set(false);
                            },
                            {language.text("Cancel import", "取消导入")}
                        }
                    }
                }
            }
            if let Some(data) = download() {
                div { class: "studio-dialog-backdrop",
                    div {
                        class: "studio-dialog",
                        role: "dialog",
                        "aria-modal": "true",
                        "aria-label": language.text("Export JSON map", "导出 JSON 地图"),
                        h2 { {language.text("Export JSON", "导出 JSON")} }
                        for warning in &data.warnings {
                            p { "{warning}" }
                        }
                        a {
                            download: "map.json",
                            href: format!(
                                "data:{};charset=utf-8,{}",
                                data.media_type,
                                crate::percent_encode(&data.text),
                            ),
                            {language.text("Download JSON", "下载 JSON")}
                        }
                        button { onclick: move |_| download.set(None),
                            {language.text("Close export", "关闭导出")}
                        }
                    }
                }
            }
        }
    }
}
