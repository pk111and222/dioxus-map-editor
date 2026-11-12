//! A host-owned bilingual workspace built from the crate's public canvas handle.
use crate::{
    persistence::{load, write},
    Language,
};
use dioxus::prelude::*;
use dioxus_map_editor::{
    command::Command,
    document::*,
    editor::*,
    geometry::*,
    history::HistoryBudget,
    plugin::{resource, PluginHost, PluginId},
    plugins::{
        brushes::BrushesPlugin, formats::FormatsPlugin, map::MapPackPlugin, objects::ObjectsPlugin,
    },
    registry::Registry,
    scene,
    session::{MapSession, Standalone},
    ui::{MapCanvas, MapCanvasHandle, TextEditorLabels},
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Select,
    Land,
    Forest,
    Mountain,
    River,
    Road,
    Place,
    Text,
    Pan,
}
impl Mode {
    fn label(self, l: Language) -> &'static str {
        match self {
            Self::Select => l.text("Select", "选择"),
            Self::Land => l.text("Land", "陆地"),
            Self::Forest => l.text("Forest", "树林"),
            Self::Mountain => l.text("Mountains", "山脉"),
            Self::River => l.text("River", "河流"),
            Self::Road => l.text("Road", "道路"),
            Self::Place => l.text("Place", "地点"),
            Self::Text => l.text("Text", "文字"),
            Self::Pan => l.text("Pan", "抓手"),
        }
    }
    fn hint(self, l: Language) -> &'static str {
        match self {
            Self::Land | Self::River | Self::Road => l.text(
                "Drag to draw. Release to apply; undo removes the whole stroke.",
                "按住拖绘，松手生效；一次撤销整笔。",
            ),
            Self::Forest | Self::Mountain => l.text(
                "Drag to scatter symbols. Undo removes the whole stroke.",
                "按住拖动散布图章，一次撤销整笔。",
            ),
            Self::Place => l.text("Click to place a named location.", "点击放置命名地点。"),
            Self::Text => l.text("Click the map to add a label.", "点击地图添加文字。"),
            Self::Pan => l.text(
                "Drag to pan. Ctrl + scroll to zoom.",
                "拖动平移，Ctrl + 滚轮缩放。",
            ),
            Self::Select => l.text(
                "Click to select, drag to move, Shift to select more.",
                "点击选择，拖动移动，Shift 多选。",
            ),
        }
    }
    fn apply(self, t: &mut EditorTools) {
        t.selection.clear();
        t.brush = None;
        t.paint = Paint::default();
        t.layer = "relief".into();
        t.closed = false;
        t.smooth = true;
        match self {
            Self::Select => t.tool = EditorTool::Select,
            Self::Pan => t.tool = EditorTool::Pan,
            Self::Land => {
                dioxus_map_editor::map::MapPreset::Land.apply(t);
                t.smooth = true;
                t.layer = "land".into();
            }
            Self::River | Self::Road => {
                if self == Self::River {
                    dioxus_map_editor::map::MapPreset::River
                } else {
                    dioxus_map_editor::map::MapPreset::Road
                }
                .apply(t);
                t.layer = "routes".into();
            }
            Self::Forest | Self::Mountain => {
                let trees = self == Self::Forest;
                t.tool = EditorTool::Scatter;
                t.stamp = resource(if trees {
                    "builtin:pine"
                } else {
                    "builtin:ridge"
                });
                t.paint.role = if trees { "vegetation" } else { "mountain" }.into();
                t.scatter.size = if trees { [0.4, 0.7] } else { [0.75, 1.2] };
                t.scatter.spacing = if trees { 13. } else { 29. };
                t.scatter.spread = if trees { 22. } else { 8. };
                t.scatter.rotation = [-0.04, 0.04];
            }
            Self::Place => {
                t.tool = EditorTool::MapNode;
                t.layer = "places".into();
                t.stamp = resource("builtin:citadel");
                t.paint.role = "node".into();
            }
            Self::Text => {
                t.tool = EditorTool::Text;
                t.layer = "labels".into();
                t.paint.role = "label".into();
            }
        }
    }
}
#[component]
pub(crate) fn Studio(
    invalidation: u64,
    language: Language,
    text_overlay: Option<Callback<MapCanvasHandle, Element>>,
    text_properties: Option<Callback<MapCanvasHandle, Element>>,
) -> Element {
    let mut current = use_signal(|| None::<Signal<MapSession>>);
    let mut error = use_signal(String::new);
    use_future(move || async move {
        match load("atlas-studio".into()).await {
            Ok(s) => current.set(Some(Signal::new(s))),
            Err(e) => error.set(e.to_string()),
        }
    });
    rsx! {
        if let Some(session) = current() {
                Workspace { session, invalidation, language, text_overlay, text_properties }
        } else {
            div { class: "studio-loading", role: "status",
                if error().is_empty() {
                    {language.text("Opening map…", "正在打开地图…")}
                } else {
                    "{error}"
                }
            }
        }
    }
}
#[component]
fn Workspace(
    mut session: Signal<MapSession>,
    invalidation: u64,
    language: Language,
    text_overlay: Option<Callback<MapCanvasHandle, Element>>,
    text_properties: Option<Callback<MapCanvasHandle, Element>>,
) -> Element {
    let mut tools = use_signal(|| {
        let mut t = EditorTools::default();
        Mode::Forest.apply(&mut t);
        t.node_name = "New place".into();
        t
    });
    let mut registry = use_signal(Registry::default);
    let view = use_signal(Viewport::default);
    let mut handle = use_signal(|| None::<MapCanvasHandle>);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut exported = use_signal(|| None::<dioxus_map_editor::export::SvgExport>);
    let mut show_layers = use_signal(|| false);
    let mut blank_dialog = use_signal(|| false);
    let mut fitted = use_signal(|| false);
    let mut show_plugins = use_signal(|| false);
    let mut plugins = use_signal(PluginHost::default);
    let mut initialized = use_signal(|| false);
    use_drop(move || plugins.write().release());
    use_effect(move || {
        if initialized() {
            return;
        }
        initialized.set(true);
        let result = (|| -> dioxus_map_editor::error::MapResult<()> {
            let mut h = plugins.write();
            h.register(FormatsPlugin)?;
            h.register(BrushesPlugin)?;
            h.register(ObjectsPlugin)?;
            h.register(MapPackPlugin)?;
            for key in [
                "official:formats",
                "official:brushes",
                "official:objects",
                "official:map",
            ] {
                h.set_enabled(
                    &PluginId(key.into()),
                    true,
                    &mut session.write(),
                    &mut tools.write(),
                )?;
            }
            registry.set(h.registry().clone());
            Mode::Forest.apply(&mut tools.write());
            Ok(())
        })();
        if let Err(e) = result {
            message.set(e.to_string());
        }
    });
    let current = match tools.read().tool {
        EditorTool::Select => Mode::Select,
        EditorTool::Pan => Mode::Pan,
        EditorTool::Text => Mode::Text,
        EditorTool::MapNode | EditorTool::Anchor | EditorTool::Stamp => Mode::Place,
        EditorTool::Scatter if tools.read().paint.role == "mountain" => Mode::Mountain,
        EditorTool::Brush | EditorTool::Path if tools.read().paint.role == "river" => Mode::River,
        EditorTool::Brush | EditorTool::Path
            if ["road", "trail"].contains(&tools.read().paint.role.as_str()) =>
        {
            Mode::Road
        }
        EditorTool::Brush if tools.read().closed => Mode::Land,
        EditorTool::Scatter | EditorTool::Brush => Mode::Forest,
        EditorTool::Path => Mode::Land,
    };
    let count = session.read().snapshot().document.objects.len();
    let history = session.read().history();
    let dirty = session.read().is_dirty();
    let selected = tools.read().selection.len();
    let blocked_layer = session
        .read()
        .snapshot()
        .document
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| l.id == tools.read().layer && (l.locked || !l.visible))
        .map(|(i, l)| (i, l.clone()));
    let mut save = move || {
        if busy() {
            return;
        }
        if let Some(h) = handle() {
            h.cancel();
        }
        let pending = session.write().begin_save();
        match pending {
            Err(e) => message.set(e.to_string()),
            Ok(None) => message.set(language.text("All changes saved", "所有更改已保存").into()),
            Ok(Some(request)) => {
                busy.set(true);
                let limits = session.peek().limits();
                spawn(async move {
                    let receipt = write(request, limits, false).await;
                    let result = receipt.result.clone();
                    if session.write().complete_save(receipt) {
                        message.set(match result {
                            Ok(_) => language.text("Map saved", "地图已保存").into(),
                            Err(e) => e.to_string(),
                        });
                    }
                    busy.set(false);
                });
            }
        }
    };
    let mut edit_history = move |redo: bool| {
        if let Some(h) = handle() {
            h.cancel();
        }
        let result = if redo {
            session.write().redo(&mut Standalone)
        } else {
            session.write().undo(&mut Standalone)
        };
        if let Err(e) = result {
            message.set(e.to_string());
        }
    };
    rsx! {
        section {
            class: "studio",
            "aria-label": language.text("Map workspace", "地图创作工作区"),
            header { class: "studio-header",
                div { class: "studio-identity",
                    Icon { name: "map" }
                    span { {language.text("Mistward Isles", "雾岸群岛")} }
                    small { {language.text("Paper atlas", "自然纸绘")} }
                }
                div { class: "studio-header-actions",
                    button {
                        title: language.text("Undo", "撤销"),
                        "aria-label": language.text("Undo", "撤销"),
                        disabled: history.undo == 0,
                        onclick: move |_| edit_history(false),
                        Icon { name: "undo" }
                    }
                    button {
                        title: language.text("Redo", "重做"),
                        "aria-label": language.text("Redo", "重做"),
                        disabled: history.redo == 0,
                        onclick: move |_| edit_history(true),
                        Icon { name: "redo" }
                    }
                    button {
                        title: language.text("Fit map", "适应画布"),
                        "aria-label": language.text("Fit map", "适应画布"),
                        onclick: move |_| {
                            if let Some(h) = handle() {
                                h.fit_frame();
                            }
                        },
                        Icon { name: "fit" }
                    }
                    button {
                        title: language.text("Layers", "图层"),
                        "aria-label": language.text("Layers", "图层"),
                        "aria-pressed": show_layers(),
                        onclick: move |_| show_layers.toggle(),
                        Icon { name: "layers" }
                    }
                    button {
                        onclick: move |_| show_plugins.toggle(),
                        "aria-pressed": show_plugins(),
                        {language.text("Extensions", "扩展")}
                    }
                    button { onclick: move |_| blank_dialog.set(true),
                        {language.text("New map", "新建空白图")}
                    }
                    button {
                        onclick: move |_| {
                            let s = session.peek();
                            match dioxus_map_editor::export::export_svg(
                                s.snapshot(),
                                &registry.peek(),
                                &s.limits(),
                            ) {
                                Ok(result) => exported.set(Some(result)),
                                Err(e) => message.set(e.to_string()),
                            }
                        },
                        {language.text("Export SVG", "导出 SVG")}
                    }
                    button {
                        class: "studio-primary",
                        disabled: busy(),
                        onclick: move |_| save(),
                        if busy() {
                            {language.text("Saving…", "保存中…")}
                        } else {
                            {language.text("Save", "保存")}
                        }
                    }
                }
            }
            div { class: "studio-body",
                nav {
                    class: "studio-tools",
                    "aria-label": language.text("Map tools", "地图工具"),
                    for next in [
                        Mode::Select,
                        Mode::Land,
                        Mode::Forest,
                        Mode::Mountain,
                        Mode::River,
                        Mode::Road,
                        Mode::Place,
                        Mode::Text,
                        Mode::Pan,
                    ]
                    {
                        button {
                            "aria-pressed": current == next,
                            title: next.hint(language),
                            onclick: move |_| {
                                if let Some(h) = handle() {
                                    h.cancel();
                                }
                                next.apply(&mut tools.write());
                                message.set(String::new());
                            },
                            Icon {
                                name: match next {
                                    Mode::Select => "select",
                                    Mode::Land => "land",
                                    Mode::Forest => "forest",
                                    Mode::Mountain => "mountain",
                                    Mode::River => "river",
                                    Mode::Road => "road",
                                    Mode::Place => "place",
                                    Mode::Text => "text",
                                    Mode::Pan => "pan",
                                },
                            }
                            span { "{next.label(language)}" }
                        }
                    }
                }
                div { class: "studio-canvas-wrap",
                    MapCanvas {
                        session,
                        tools,
                        registry,
                        view,
                        label: language.text("Map canvas", "地图画布"),
                        invalidation,
                        show_controls: false,
                        show_status: false,
                        fit_on_resize: true,
                        text_labels: TextEditorLabels {
                            input: language.text("Map label", "地图文字").into(),
                            commit: language.text("Apply", "应用").into(),
                            cancel: language.text("Cancel", "取消").into(),
                        },
                        onerror: move |error: dioxus_map_editor::error::MapError| {
                            let s = session.read();
                            let t = tools.read();
                            let layer = s.snapshot().document.layers.iter().find(|l| l.id == t.layer);
                            let notice = match layer {
                                Some(l) if l.locked => language.text("This layer is locked. Unlock it before drawing.", "此图层已锁定，请先解锁再绘制。").into(),
                                Some(l) if !l.visible => language.text("This layer is hidden. Show it before drawing.", "此图层已隐藏，请先显示再绘制。").into(),
                                _ if error == dioxus_map_editor::error::MapError::ReadOnly => language.text("This document is currently read-only.", "当前文档不可编辑。").into(),
                                _ => error.to_string(),
                            };
                            message.set(notice);
                        },
                        text_overlay: move |canvas| rsx! {
                            if let Some(render) = text_overlay { {render.call(canvas)} } else { StudioTextEditor { canvas, language } }
                        },
                        onready: move |h: MapCanvasHandle| {
                            handle.set(Some(h));
                            if !fitted() {
                                fitted.set(true);
                                h.fit_frame();
                            }
                        },
                    }
                    div { class: "studio-canvas-hint",
                        span {
                            if tools.read().tool == EditorTool::Path {
                                {
                                    language
                                        .text(
                                            "Click to add nodes. Valid shapes apply immediately; Esc starts a new shape.",
                                            "点击增加节点，有效形状立即生效；Esc 开始新形状。",
                                        )
                                }
                            } else {
                                "{current.hint(language)}"
                            }
                        }
                    }
                }
                aside {
                    class: "studio-inspector",
                    "aria-label": language.text("Tool settings", "当前工具设置"),
                    if let Some((index, layer)) = blocked_layer {
                        div { class: "studio-layer-notice", role: "alert",
                            strong { {language.text("Drawing is blocked", "当前图层无法绘制")} }
                            p { "{layer.name}" }
                            p {
                                {
                                    if layer.locked {
                                        language
                                            .text(
                                                "This layer is locked. Unlock it before drawing.",
                                                "此图层已锁定，请先解锁再绘制。",
                                            )
                                    } else {
                                        language
                                            .text(
                                                "This layer is hidden. Show it before drawing.",
                                                "此图层已隐藏，请先显示再绘制。",
                                            )
                                    }
                                }
                            }
                            button {
                                class: "studio-primary",
                                onclick: move |_| {
                                    let mut next = layer.clone();
                                    next.locked = false;
                                    next.visible = true;
                                    if let Some(h) = handle() {
                                        h.execute(
                                            vec![
                                                Command::PutLayer {
                                                    layer: next,
                                                    index,
                                                },
                                            ],
                                        );
                                    }
                                    message.set(String::new());
                                },
                                {language.text("Enable drawing on this layer", "允许在此图层绘制")}
                            }
                        }
                    }
                    if show_plugins() {
                        crate::plugin_panel::PluginPanel {
                            session,
                            tools,
                            registry,
                            plugins,
                            handle,
                            language,
                        }
                    }
                    h2 { "{current.label(language)}" }
                    p { class: "studio-muted studio-caption",
                        {
                            language
                                .text(
                                    "Your world, one mark at a time.",
                                    "一笔一画，构建你的世界。",
                                )
                        }
                    }
                    if matches!(current, Mode::Forest | Mode::Mountain | Mode::Place) {
                        div { class: "studio-symbol-catalogue",
                            StampPreview { registry, resource: tools.read().stamp.clone() }
                            label { class: "studio-symbol-picker",
                                span { {language.text("Symbol catalogue", "图章资源库")} }
                                select {
                                    "aria-label": language.text("Symbol catalogue", "图章资源库"),
                                    value: registry.read().stamps().iter().position(|s|s.resource==tools.read().stamp).map(|i|i.to_string()).unwrap_or_default(),
                                    onchange: move |event| {
                                        if let Some(symbol) = event.value().parse::<usize>().ok().and_then(|i|registry.peek().stamps().get(i).cloned()) {
                                            if let Some(h)=handle(){h.cancel();}
                                            tools.write().stamp=symbol.resource;
                                        }
                                    },
                                    if registry.read().stamp(&tools.read().stamp).is_none() {
                                        option {value:"", {language.text("Missing symbol", "图章资源未启用")} }
                                    }
                                    for (i, symbol) in registry.read().stamps().iter().enumerate() {
                                        option { value:"{i}", "{symbol.name} · {symbol.resource.key} v{symbol.resource.version}" }
                                    }
                                }
                            }
                        }
                    }
                    if current == Mode::Land {
                        label { class: "studio-field",
                            span { {language.text("Draw mode", "绘制方式")} }
                            select {
                                "aria-label": language.text("Draw mode", "绘制方式"),
                                value: if tools.read().tool == EditorTool::Path { "points" } else { "drag" },
                                onchange: move |e| {
                                    if let Some(h) = handle() {
                                        h.cancel();
                                    }
                                    tools.write().tool = if e.value() == "points" {
                                        EditorTool::Path
                                    } else {
                                        EditorTool::Brush
                                    };
                                },
                                option { value: "drag", {language.text("Drag to draw", "按住拖绘")} }
                                option { value: "points",
                                    {language.text("Click points (auto apply)", "点击节点（自动生效）")}
                                }
                            }
                        }
                        label { class: "studio-field",
                            span { {language.text("Natural edge", "自然边缘强度")} }
                            input { r#type:"range", min:"0", max:"18", step:"1",
                                "aria-label":language.text("Natural edge", "自然边缘强度"),
                                value:tools.read().region.roughness,
                                oninput:move|e| {if let Ok(v)=e.value().parse(){tools.write().region.roughness=v;}}
                            }
                        }
                        label { class: "studio-field",
                            span { {language.text("Edge alignment", "边缘对齐")} }
                            select {
                                "aria-label":language.text("Edge alignment", "边缘对齐"),
                                value:match tools.read().region.alignment {dioxus_map_editor::region::EdgeAlignment::Free=>"free",dioxus_map_editor::region::EdgeAlignment::Snap=>"snap",dioxus_map_editor::region::EdgeAlignment::Shared=>"shared"},
                                onchange:move|e| {tools.write().region.alignment=match e.value().as_str(){"snap"=>dioxus_map_editor::region::EdgeAlignment::Snap,"shared"=>dioxus_map_editor::region::EdgeAlignment::Shared,_=>dioxus_map_editor::region::EdgeAlignment::Free};},
                                option{value:"free",{language.text("Free outline", "自由轮廓")}}
                                option{value:"snap",{language.text("Snap to edge", "吸附已有边缘")}}
                                option{value:"shared",{language.text("Share border arc", "共用一段边界")}}
                            }
                        }
                        if tools.read().region.alignment != dioxus_map_editor::region::EdgeAlignment::Free {
                            label {class:"studio-field",
                                span{{language.text("Snap distance", "吸附距离")}}
                                input{r#type:"range",min:"2",max:"40",step:"1","aria-label":language.text("Snap distance", "吸附距离"),value:tools.read().region.snap_distance,oninput:move|e|{if let Ok(v)=e.value().parse(){tools.write().region.snap_distance=v;}}}
                            }
                            label {class:"studio-field",
                                span{{language.text("Guide region", "参考区域")}}
                                select{
                                    "aria-label":language.text("Guide region", "参考区域"),
                                    value:tools.read().region.target.as_ref().map(|id|id.to_string()).unwrap_or_default(),
                                    onchange:move|e|{tools.write().region.target=if e.value().is_empty(){None}else{Some(ObjectId(e.value()))};},
                                    option{value:"",{language.text("All visible regions", "所有可见区域")}}
                                    for object in session.read().snapshot().document.objects.iter().filter(|o| matches!(o.geometry,Geometry::Path{closed:true,..})&&!o.extensions.contains_key("map:ribbon")) {
                                        option{value:"{object.id}","{object.id}"}
                                    }
                                }
                            }
                            p{class:"studio-muted",{language.text("Place two consecutive points near the same guide to copy its shorter border arc. Other edges keep your natural-edge setting. Shared borders are saved geometry; later edits do not link the regions.","相邻两点靠近同一参考区域，将复制两点之间较短的边界；其余边缘保留自然强度设置。共边保存为独立几何，后续编辑不会联动。")}}
                        }
                        if tools.read().tool == EditorTool::Path {
                            p { class: "studio-muted",
                                {
                                    language
                                        .text(
                                            "Three points make a region; each next click applies immediately. Esc starts a new shape.",
                                            "三个节点形成区域，之后每次点击立即更新；Esc 开始新区域。",
                                        )
                                }
                            }
                        }
                    }
                    if matches!(current, Mode::Forest | Mode::Mountain) {
                        label { class: "studio-field",
                            span { {language.text("Spread", "笔刷范围")} }
                            input {
                                r#type: "range",
                                min: "0",
                                max: "60",
                                "aria-label": language.text("Spread", "笔刷范围"),
                                value: tools.read().scatter.spread,
                                oninput: move |e| {
                                    if let Ok(v) = e.value().parse::<f64>() {
                                        let _ = tools.write().scatter.resize(v);
                                    }
                                },
                            }
                        }
                        label { class: "studio-field",
                            span { {language.text("Spacing", "种植间距")} }
                            input {
                                r#type: "range",
                                min: "5",
                                max: "50",
                                "aria-label": language.text("Spacing", "种植间距"),
                                value: tools.read().scatter.spacing,
                                oninput: move |e| {
                                    if let Ok(v) = e.value().parse::<f64>() {
                                        tools.write().scatter.spacing = v;
                                    }
                                },
                            }
                        }
                        label { class: "studio-field",
                            span { {language.text("Symbol size", "图章大小")} }
                            input {
                                r#type: "range",
                                min: "0.2",
                                max: "4",
                                step: "0.1",
                                "aria-label": language.text("Symbol size", "图章大小"),
                                value: tools.read().scatter.size[1],
                                oninput: move |e| {
                                    if let Ok(v) = e.value().parse::<f64>() {
                                        tools.write().scatter.size = [v * 0.65, v];
                                    }
                                },
                            }
                        }
                        label { class: "studio-field",
                            span { {language.text("Density", "种植密度")} }
                            input {
                                r#type: "range",
                                min: "0.1",
                                max: "1",
                                step: "0.05",
                                "aria-label": language.text("Density", "种植密度"),
                                value: tools.read().scatter.density,
                                oninput: move |e| {
                                    if let Ok(v) = e.value().parse::<f64>() {
                                        tools.write().scatter.density = v;
                                    }
                                },
                            }
                        }
                    }
                    if matches!(current, Mode::River | Mode::Road) {
                        label { class: "studio-field",
                            span { {language.text("Brush width", "笔刷宽度")} }
                            input {
                                r#type: "range",
                                min: "2",
                                max: "50",
                                step: "0.5",
                                "aria-label": language.text("Brush width", "笔刷宽度"),
                                value: tools.read().paint.width.unwrap_or(4.),
                                oninput: move |e| {
                                    if let Ok(v) = e.value().parse::<f64>() {
                                        tools.write().paint.width = Some(v);
                                    }
                                },
                            }
                        }
                    }
                    if current == Mode::Place {
                        label { class: "studio-field",
                            span { {language.text("Place name", "地点名称")} }
                            input {
                                "aria-label": language.text("Place name", "地点名称"),
                                value: tools.read().node_name.clone(),
                                oninput: move |e| tools.write().node_name = e.value(),
                            }
                        }
                    }
                    if current == Mode::Text {
                        if let Some(render) = text_properties {
                            if let Some(canvas) = handle() { {render.call(canvas)} }
                        } else {
                        label { class: "studio-field",
                            span { {language.text("Text size", "文字大小")} }
                            input {
                                r#type: "range",
                                min: "10",
                                max: "48",
                                "aria-label": language.text("Text size", "文字大小"),
                                value: tools.read().text_size,
                                oninput: move |e| {
                                    if let Ok(v) = e.value().parse::<f64>() {
                                        tools.write().text_size = v;
                                    }
                                },
                            }
                        }
                        }
                    }
                    if current == Mode::Select {
                        p {
                            {format!("{} {}", selected, language.text("selected", "个已选对象"))}
                        }
                        button {
                            disabled: selected == 0,
                            onclick: move |_| {
                                if let Some(h) = handle() {
                                    h.execute(delete_selection(&tools.peek().selection));
                                }
                            },
                            {language.text("Delete selection", "删除选择")}
                        }
                        if selected > 0 && session.read().snapshot().document.objects.iter().filter(|o|tools.read().selection.contains(&o.id)).all(|o|matches!(o.geometry,Geometry::Stamp{..})) {
                            label{class:"studio-field",
                                span{{language.text("Replace selected symbols", "替换选中实体")}}
                                select{
                                    "aria-label":language.text("Replace selected symbols", "替换选中实体"),
                                    value:"",
                                    onchange:move|e|{
                                        let symbol=e.value().parse::<usize>().ok().and_then(|i|registry.peek().stamps().get(i).map(|s|s.resource.clone()));
                                        if let Some(symbol)=symbol {
                                            let result=dioxus_map_editor::map::replace_symbols(session.peek().snapshot(),&tools.peek().selection,symbol,&registry.peek());
                                            match result {Ok(commands)=>{if let Some(h)=handle(){h.execute(commands);}},Err(e)=>message.set(e.to_string())}
                                        }
                                    },
                                    option{value:"",{language.text("Choose a symbol…", "选择新的实体…")}}
                                    for (i,symbol) in registry.read().stamps().iter().enumerate(){option{value:"{i}","{symbol.name} · {symbol.resource.key}"}}
                                }
                            }
                        }
                    }
                    div { class: "studio-style-note",
                        span { class: "studio-swatch" }
                        span { {language.text("Paper atlas", "自然纸绘")} }
                    }
                    if show_layers() {
                        div { class: "studio-layers",
                            for (index, layer) in session.read().snapshot().document.layers.iter().enumerate().rev() {
                                {
                                    let layer = layer.clone();
                                    let visible = layer.visible;
                                    let locked = layer.locked;
                                    let for_visibility = layer.clone();
                                    rsx! {
                                        div { class: "studio-layer",
                                            span { "{layer.name}" }
                                            button {
                                                "aria-label": format!("{} {}", language.text("Show layer", "显示图层"), layer.name),
                                                "aria-pressed": visible,
                                                onclick: move |_| {
                                                    let mut l = for_visibility.clone();
                                                    l.visible = !l.visible;
                                                    if let Some(h) = handle() {
                                                        h.execute(
                                                            vec![
                                                                Command::PutLayer {
                                                                    layer: l,
                                                                    index,
                                                                },
                                                            ],
                                                        );
                                                    }
                                                },
                                                Icon { name: if visible { "eye" } else { "hidden" } }
                                            }
                                            button {
                                                class: "studio-layer-lock",
                                                "aria-label": format!(
                                                    "{} {}",
                                                    if locked {
                                                        language.text("Unlock layer", "解锁图层")
                                                    } else {
                                                        language.text("Lock layer", "锁定图层")
                                                    },
                                                    layer.name,
                                                ),
                                                title: if locked { language.text("Locked — click to unlock", "已锁定，点击解锁") } else { language.text("Editable — click to lock", "可编辑，点击锁定") },
                                                "aria-pressed": locked,
                                                onclick: move |_| {
                                                    let mut l = layer.clone();
                                                    l.locked = !l.locked;
                                                    if let Some(h) = handle() {
                                                        h.execute(
                                                            vec![
                                                                Command::PutLayer {
                                                                    layer: l,
                                                                    index,
                                                                },
                                                            ],
                                                        );
                                                    }
                                                },
                                                Icon { name: if locked { "lock" } else { "unlock" } }
                                                span { class: "studio-lock-state",
                                                    {
                                                        if locked {
                                                            language.text("Locked", "已锁")
                                                        } else {
                                                            language.text("Open", "可绘")
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            footer {
                class: "studio-status",
                "data-count": count,
                "data-revision": session.read().stamp().revision,
                "data-dirty": dirty,
                span {
                    if dirty {
                        {language.text("Unsaved", "待保存")}
                    } else {
                        {language.text("Saved locally", "已本地保存")}
                    }
                }
                span { role: "status",
                    if message().is_empty() {
                        {format!("{} {}", count, language.text("objects", "个对象"))}
                    } else {
                        "{message}"
                    }
                }
                span { "{view.read().scale()*100.:.0}%" }
            }
            if blank_dialog() {
                div { class: "studio-dialog-backdrop",
                    div {
                        class: "studio-dialog",
                        role: "dialog",
                        "aria-modal": "true",
                        "aria-label": language.text("New map", "新建空白图"),
                        h2 { {language.text("Start a blank map?", "开始一张空白地图？")} }
                        p {
                            {
                                language
                                    .text(
                                        "Your current map is saved first. Clearing the canvas can be undone.",
                                        "先保存当前地图，清空后可以撤销恢复。",
                                    )
                            }
                        }
                        button { onclick: move |_| blank_dialog.set(false),
                            {language.text("Cancel", "取消")}
                        }
                        button {
                            disabled: busy(),
                            onclick: move |_| {
                                if let Some(h) = handle() {
                                    h.cancel();
                                }
                                busy.set(true);
                                spawn(async move {
                                    let pending = session.write().begin_save();
                                    let result = match pending {
                                        Ok(Some(request)) => {
                                            let receipt = write(request, session.peek().limits(), false).await;
                                            let result = receipt.result.clone();
                                            if session.write().complete_save(receipt) {
                                                result
                                            } else {
                                                Err(dioxus_map_editor::error::MapError::Stale)
                                            }
                                        }
                                        Ok(None) => Ok(()),
                                        Err(e) => Err(e),
                                    };
                                    if let Err(e) = result {
                                        message.set(e.to_string());
                                        busy.set(false);
                                        return;
                                    }
                                    if session.peek().is_dirty() {
                                        message
                                            .set(
                                                language
                                                    .text(
                                                        "New edits arrived while saving. Try again.",
                                                        "保存期间有新编辑，请重试。",
                                                    )
                                                    .into(),
                                            );
                                        busy.set(false);
                                        return;
                                    }
                                    let commands = session
                                        .peek()
                                        .snapshot()
                                        .document
                                        .objects
                                        .iter()
                                        .map(|o| Command::RemoveObject(o.id.clone()))
                                        .collect();
                                    match session.write().execute(commands) {
                                        Ok(_) => {
                                            tools.write().selection.clear();
                                            blank_dialog.set(false);
                                        }
                                        Err(e) => message.set(e.to_string()),
                                    }
                                    busy.set(false);
                                });
                            },
                            {language.text("Save and create", "保存并新建")}
                        }
                    }
                }
            }
            if let Some(result) = exported() {
                div { class: "studio-dialog-backdrop",
                    div {
                        class: "studio-dialog",
                        role: "dialog",
                        "aria-modal": "true",
                        "aria-label": language.text("Export map", "导出地图"),
                        h2 { {language.text("Export SVG", "导出 SVG")} }
                        for warning in &result.warnings {
                            p { "{warning}" }
                        }
                        a {
                            href: format!("data:image/svg+xml;charset=utf-8,{}", crate::percent_encode(&result.svg)),
                            download: "mistward-isles.svg",
                            {language.text("Download map", "下载地图")}
                        }
                        button { onclick: move |_| exported.set(None),
                            {language.text("Close", "关闭")}
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn StampPreview(registry: Signal<Registry>, resource: ResourceRef) -> Element {
    let r = registry.read();
    let paint = r.paint(
        &dioxus_map_editor::plugin::resource("builtin:atlas"),
        &Paint {
            role: if resource.key.contains("ridge") {
                "mountain"
            } else {
                "vegetation"
            }
            .into(),
            ..Default::default()
        },
    );
    let markup = r
        .stamp(&resource)
        .map(|s| {
            s.paths
                .iter()
                .map(|p| {
                    format!(
                        "<path d=\"{}\" fill=\"{}\"/>",
                        scene::path_data(&p.nodes, p.closed),
                        if p.filled { "currentColor" } else { "none" }
                    )
                })
                .collect::<String>()
        })
        .unwrap_or_default();
    rsx! {
        svg { view_box: "-24 -24 48 48", "aria-hidden": "true",
            g {
                transform: "scale(1 -1)",
                stroke: paint.color.clone(),
                color: paint.fill.unwrap_or(paint.color),
                stroke_width: paint.width,
                stroke_linecap: "round",
                stroke_linejoin: "round",
                dangerous_inner_html: markup,
            }
        }
    }
}
#[component]
fn Icon(name: String) -> Element {
    let path=match name.as_str(){"map"=>"M3 5 9 3 15 5 21 3 V19 L15 21 9 19 3 21 Z M9 3 V19 M15 5 V21","select"=>"M5 3 19 13 12 14 9 21 Z","land"=>"M3 17 5 11 9 12 12 5 17 8 21 17 Z M3 21 H21","forest"=>"M8 3 3 12 H6 L2 18 H14 L10 12 H13 Z M8 18 V22 M17 4 13 12 H15 L12 18 H22 L19 12 H21 Z M17 18 V22","mountain"=>"M2 20 10 4 17 20 Z M12 9 16 3 23 20 H17","river"=>"M4 4 C17 2 7 10 16 12 S8 20 20 21","road"=>"M7 2 3 22 M17 2 21 22 M12 3 V7 M12 10 V14 M12 17 V21","place"=>"M19 10 C19 16 12 22 12 22 S5 16 5 10 A7 7 0 0 1 19 10 Z M15 10 A3 3 0 1 1 9 10 A3 3 0 1 1 15 10","text"=>"M4 5 V3 H20 V5 M12 3 V21 M8 21 H16","pan"=>"M7 12 V5 Q9 3 10 5 V11 M10 11 V3 Q12 1 13 3 V11 M13 11 V4 Q15 2 16 4 V12 M16 12 V7 Q18 5 19 7 V16 Q19 22 12 22 Q8 22 5 17 L2 12 Q3 9 7 12 Z","undo"=>"M4 10 H14 Q21 10 21 17 M4 10 9 5 M4 10 9 15","redo"=>"M20 10 H10 Q3 10 3 17 M20 10 15 5 M20 10 15 15","fit"=>"M8 3 H3 V8 M16 3 H21 V8 M3 16 V21 H8 M21 16 V21 H16","layers"=>"M12 3 2 8 12 13 22 8 Z M2 12 12 17 22 12 M2 16 12 21 22 16","eye"=>"M2 12 Q12 1 22 12 Q12 23 2 12 Z","hidden"=>"M3 3 21 21 M2 12 Q8 4 15 7 M22 12 Q17 19 10 18","lock"=>"M6 10 H18 V21 H6 Z M8 10 V6 A4 4 0 0 1 16 6 V10",_=>"M6 10 H18 V21 H6 Z M8 10 V6 A4 4 0 0 1 16 6"};
    rsx! {
        svg {
            class: "studio-icon",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.5",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            path { d: path }
        }
    }
}
pub(crate) fn fresh_session() -> dioxus_map_editor::error::MapResult<MapSession> {
    MapSession::open(
        crate::atlas::sample()?,
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
}

/// Paper-style demo text panel built entirely from the public handle.
/// Replace it through `Demo::text_overlay` or use it directly in a custom canvas.
#[component]
pub fn StudioTextEditor(canvas: MapCanvasHandle, language: Language) -> Element {
    let Some(draft) = canvas.text_draft() else {
        return rsx! {};
    };
    let p = canvas.viewport().read().world_to_screen(draft.position);
    rsx! {
        div {
            class: "studio-text-position",
            style: "left:max(12px,min({p.x}px,calc(100% - 308px)));top:max(12px,min({p.y}px,calc(100% - 180px)));",
            div {
                class: "studio-text-card",
                role: "group",
                "aria-label": language.text("Edit map label", "编辑地图文字"),
                div { class: "studio-text-heading",
                    span { "Aa" }
                    strong { {language.text("Map label", "地图文字")} }
                }
                input {
                    key: "{draft.id}",
                    class: "studio-text-input",
                    initial_value: draft.text,
                    "aria-label": language.text("Map label", "地图文字"),
                    placeholder: language.text("Name a place or tell a story…", "为这片土地写下名字…"),
                    onmounted: move |event| async move {
                        let _ = event.set_focus(true).await;
                    },
                    oninput: move |event| canvas.set_text(event.value()),
                    oncompositionstart: move |_| canvas.set_composing(true),
                    oncompositionend: move |_| canvas.set_composing(false),
                    onkeyup: move |event| event.stop_propagation(),
                    onkeydown: move |event| {
                        event.stop_propagation();
                        if event.is_composing() || canvas.text_draft().is_some_and(|d| d.composing) {
                            return;
                        }
                        match event.key() {
                            Key::Enter => {
                                event.prevent_default();
                                canvas.commit_text();
                            }
                            Key::Escape => {
                                event.prevent_default();
                                canvas.cancel();
                            }
                            _ => {}
                        }
                    },
                }
                div { class: "studio-text-actions",
                    span {
                        {language.text("Enter to apply · Esc to cancel", "Enter 应用 · Esc 取消")}
                    }
                    button { onclick: move |_| canvas.cancel(), {language.text("Cancel", "取消")} }
                    button {
                        class: "studio-primary",
                        onclick: move |_| canvas.commit_text(),
                        {language.text("Apply", "应用")}
                    }
                }
            }
        }
    }
}
