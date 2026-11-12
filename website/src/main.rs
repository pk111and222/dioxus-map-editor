//! Static bilingual project website; all UI and the live map run in Rust/Wasm.
mod lifecycle;
use dioxus::prelude::*;
use map_editor_demo::{Demo, Language};
const SITE_CSS: &str = include_str!("../assets/site.css");
const API_CODE: &str = include_str!("snippets/canvas.rs.txt");
const INSTALL:&str="[dependencies]\ndioxus = { version = \"=0.7.10\", features = [\"web\"] }\ndioxus-map-editor = { version = \"0.1.0\", features = [\"official-plugins\"] }";
fn main() {
    dioxus::launch(App);
}
#[component]
fn App() -> Element {
    let mut language = use_signal(|| Language::English);
    let mut tab = use_signal(|| 0usize);
    let epoch = use_signal(|| 0u64);
    #[cfg(target_arch = "wasm32")]
    use_hook(move || std::rc::Rc::new(lifecycle::WebLifecycle::install(epoch)));
    let l = language();
    rsx! {
        document::Title {
            {
                l.text(
                    "Dioxus Map Editor — Draw worlds in Rust",
                    "Dioxus Map Editor — 用 Rust 绘制世界",
                )
            }
        }
        document::Link {
            href: asset!("/assets/favicon.svg"),
            r#type: "image/svg+xml",
            rel: "icon",
        }
        document::Meta {
            name: "description",
            content: l.text(
                "An embeddable Rust and Dioxus SVG map editor. Draw fantasy worlds, compose your own UI, and extend with typed plugins.",
                "可嵌入的 Rust 与 Dioxus SVG 地图编辑器，支持幻想地图绘制、自定义 UI 和类型化插件。",
            ),
        }
        style { {SITE_CSS} }
        div { class: "site", lang: if l == Language::English { "en" } else { "zh-CN" },
            header { class: "site-nav",
                a { class: "wordmark", href: "#",
                    span { class: "brand-mark", "◈" }
                    span {
                        "dioxus"
                        strong { "map editor" }
                    }
                }
                nav { "aria-label": l.text("Main navigation", "主导航"),
                    a { href: "#features", {l.text("Features", "功能")} }
                    a { href: "#playground", {l.text("Playground", "在线体验")} }
                    a { href: "#api", {l.text("API & guides", "API 与指南")} }
                    a {
                        href: "https://github.com/pk111and222/dioxus-map-editor",
                        target: "_blank",
                        rel: "noreferrer",
                        "GitHub ↗"
                    }
                }
                button {
                    class: "language-toggle",
                    "aria-label": "Switch language / 切换语言",
                    onclick: move |_| {
                        language
                            .set(
                                if language() == Language::English {
                                    Language::Chinese
                                } else {
                                    Language::English
                                },
                            )
                    },
                    {l.text("中文", "English")}
                }
            }
            main {
                section { class: "hero",
                    div { class: "hero-copy",
                        p { class: "eyebrow", "RUST · DIOXUS · SVG" }
                        h1 {
                            {l.text("A small canvas.", "一方画布，")}
                            br {}
                            em { {l.text("A world of your own.", "自成世界。")} }
                        }
                        p { class: "hero-description",
                            {
                                l.text(
                                    "Put coastlines, forests and imagined places on paper. An embeddable map editor for Rust applications, with a UI that stays yours.",
                                    "把海岸、森林与想象中的地点绘在纸上。面向 Rust 应用的可嵌入地图编辑器，界面始终由你定义。",
                                )
                            }
                        }
                        div { class: "hero-actions",
                            a { class: "primary-link", href: "#playground",
                                {l.text("Draw a map", "开始绘制")}
                                span { "↗" }
                            }
                            a { class: "quiet-link", href: "#api",
                                {l.text("Build with it", "接入项目")}
                                span { "→" }
                            }
                        }
                        p { class: "hero-note",
                            {
                                l.text(
                                    "Open source · MIT · Fully AI-generated, human regression-tested",
                                    "开源 · MIT · 全 AI 生成，已进行人工回归测试",
                                )
                            }
                        }
                    }
                    div { class: "hero-art",
                        div { class: "art-topline",
                            span { "FIELD NOTES / 001" }
                            span { "THE MISTWARD ISLES" }
                        }
                        img {
                            src: asset!("/assets/atlas.svg"),
                            alt: l.text(
                                "Original paper atlas with natural coastlines, mountains, forests and named places",
                                "原创纸绘地图，包含自然海岸、山林和命名地点",
                            ),
                        }
                        div { class: "art-caption",
                            span {
                                {
                                    l.text(
                                        "An editable world, not a flattened image.",
                                        "每个细节都可编辑，而非一张静态图片。",
                                    )
                                }
                            }
                            span { "35° N / 121° E" }
                        }
                    }
                }
                section { id: "features", class: "features",
                    div { class: "section-intro",
                        p { class: "eyebrow", "01 / THE TOOLKIT" }
                        h2 {
                            {
                                l.text(
                                    "From a first mark to a finished map.",
                                    "从第一笔，到一张地图。",
                                )
                            }
                        }
                    }
                    div { class: "feature-grid",
                        for (number, en, zh, body_en, body_zh) in [
                            (
                                "01",
                                "Draw the landscape",
                                "绘制地貌",
                                "Smooth regions, rivers and roads. Scatter original trees and mountains with a live preview.",
                                "平滑区域、河流与道路，实时预览并散布原创树木和山峰图章。",
                            ),
                            (
                                "02",
                                "Keep every edit",
                                "保留每一次编辑",
                                "Atomic commands, undo and redo, versioned JSON, and standalone SVG exports.",
                                "原子命令、撤销重做、版本化 JSON，以及自包含 SVG 导出。",
                            ),
                            (
                                "03",
                                "Make the UI yours",
                                "界面由你定义",
                                "Controlled canvas, public handles, custom panels, overlays, styles and compiled Rust plugins.",
                                "受控画布、公开 handle、自定义属性面板、覆盖层、风格和 Rust 编译期插件。",
                            ),
                        ]
                        {
                            article {
                                span { class: "feature-number", "{number}" }
                                h3 { "{l.text(en,zh)}" }
                                p { "{l.text(body_en,body_zh)}" }
                            }
                        }
                    }
                }
                section { id: "playground", class: "playground",
                    div { class: "section-intro split",
                        div {
                            p { class: "eyebrow", "02 / THE PLAYGROUND" }
                            h2 { {l.text("Take the scenic route.", "亲手描绘，慢慢成图。")} }
                        }
                        p {
                            {
                                l.text(
                                    "A real editor, running entirely in your browser. Your changes stay here; use Save before reloading, or export a copy.",
                                    "真正的编辑器，完全在浏览器运行。数据保存在本机；刷新前请先保存，也可以导出副本。",
                                )
                            }
                        }
                    }
                    div { class: "playground-frame",
                        Demo { invalidation: epoch(), language: l }
                    }
                    div { class: "playground-notes",
                        span {
                            {
                                l.text(
                                    "Ctrl + scroll to zoom · hold Space to pan · Ctrl/Cmd + Z to undo",
                                    "Ctrl + 滚轮缩放 · 按住空格平移 · Ctrl/Cmd + Z 撤销",
                                )
                            }
                        }
                        span {
                            {
                                l.text(
                                    "No account. No upload. Local browser storage.",
                                    "无需账号，无需上传，浏览器本地存储。",
                                )
                            }
                        }
                    }
                }
                section { id: "api", class: "api-section",
                    div { class: "section-intro",
                        p { class: "eyebrow", "03 / FOR BUILDERS" }
                        h2 {
                            {
                                l.text("Your app. Your canvas. Your rules.", "你的应用、画布与规则。")
                            }
                        }
                        p {
                            {
                                l.text(
                                    "Use the default editor, or compose every div yourself. The document, history and save pipeline stay the same.",
                                    "使用默认编辑器，或自行组合每一个 div。共享同一份文档、历史和保存流程。",
                                )
                            }
                        }
                    }
                    div { class: "api-layout",
                        div { class: "api-guide",
                            div {
                                class: "guide-tabs",
                                role: "tablist",
                                "aria-label": l.text("API guides", "API 指南"),
                                for (i, en, zh) in [
                                    (0, "Start", "快速开始"),
                                    (1, "Canvas", "受控画布"),
                                    (2, "Data & IO", "数据与存储"),
                                    (3, "Plugins", "插件"),
                                ]
                                {
                                    button {
                                        role: "tab",
                                        "aria-selected": tab() == i,
                                        "aria-controls": "guide-content",
                                        id: format!("guide-tab-{i}"),
                                        onclick: move |_| tab.set(i),
                                        "{l.text(en,zh)}"
                                    }
                                }
                            }
                            div {
                                id: "guide-content",
                                role: "tabpanel",
                                "aria-labelledby": format!("guide-tab-{}", tab()),
                                if tab() == 0 {
                                    h3 { {l.text("Add it to Cargo.toml", "添加 Cargo 依赖")} }
                                    p {
                                        {
                                            l.text(
                                                "Rust 1.85+ and Dioxus 0.7.10. The UI feature does not pick a renderer; your application enables web or desktop.",
                                                "需要 Rust 1.85+ 与 Dioxus 0.7.10。ui feature 不选择渲染器，由应用启用 web 或 desktop。",
                                            )
                                        }
                                    }
                                    CodeBlock { text: INSTALL.to_string() }
                                    p {
                                        {
                                            l.text(
                                                "The initial release is 0.1.0. Use a Git dependency below for the latest development revision.",
                                                "首个版本为 0.1.0。如需最新开发版本，可使用下面的 Git 依赖。",
                                            )
                                        }
                                    }
                                    CodeBlock {
                                        text: "dioxus-map-editor = { git = \"https://github.com/pk111and222/dioxus-map-editor\", features = [\"official-plugins\"] }"
                                            .to_string(),
                                    }
                                } //github.com/pk111and222/dioxus-map-editor\", features = [\"official-plugins\"] }".to_string()}}
                                if tab() == 1 {
                                    h3 { "MapCanvas / MapCanvasHandle" }
                                    p {
                                        {
                                            l.text(
                                                "Own session, tools, registry and viewport as signals. onready gives you a handle to cancel gestures, finish paths, fit the frame or execute commands. Panels and translations belong to your host.",
                                                "用 signal 控制会话、工具、资源和视口。onready 返回 handle，可取消手势、结束路径、适应画布或提交命令；面板与翻译由宿主提供。",
                                            )
                                        }
                                    }
                                    CodeBlock { text: API_CODE.to_string() }
                                    p {
                                        {
                                            l.text(
                                                "Pass invalidation from native focus/resize events and ancestor scroll/layout changes. See the website lifecycle adapter for the browser implementation.",
                                                "窗口失焦、尺寸变化及祖先滚动/布局变化时递增 invalidation，浏览器适配见官网源码。",
                                            )
                                        }
                                    }
                                }
                                if tab() == 2 {
                                    h3 { "MapSession / MapStore" }
                                    p {
                                        {
                                            l.text(
                                                "MapDocument is the source of truth. Command batches commit atomically and produce one undo entry. IO adapters implement MapStore; the core never chooses your file paths or backend.",
                                                "MapDocument 是内容真源，命令批次原子提交并产生一条历史。IO 适配器实现 MapStore，核心不决定文件路径或后端。",
                                            )
                                        }
                                    }
                                    CodeBlock { text: include_str!("snippets/session.rs.txt").to_string() }
                                    p {
                                        {
                                            l.text(
                                                "For asynchronous saves: begin_save, persist the request, then complete_save. Receipts are checked against the session token and revision. Bound markers refer to host-owned places.",
                                                "异步保存使用 begin_save、持久化请求、complete_save；回执校验会话 token 和 revision。绑定标记引用宿主所有的地点。",
                                            )
                                        }
                                    }
                                }
                                if tab() == 3 {
                                    h3 { "PluginHost / Registry" }
                                    p {
                                        {
                                            l.text(
                                                "Enable only the providers you need. Plugins register brushes, object codecs, formats, actions, styles and host-rendered UI contributions. Edits use the same command history.",
                                                "只启用所需插件。插件注册画笔、对象协议、格式、操作、风格和宿主渲染的 UI 声明，编辑仍走同一命令历史。",
                                            )
                                        }
                                    }
                                    CodeBlock { text: include_str!("snippets/plugins.rs.txt").to_string() }
                                    p {
                                        {
                                            l.text(
                                                "Disabling a provider cancels its preview and retains saved payloads. Missing custom objects show a recoverable placeholder. Call sync after switching sessions, release on unmount.",
                                                "停用插件取消预览并保留数据。缺失自定义对象显示可恢复占位；切换会话后调用 sync，卸载时调用 release。",
                                            )
                                        }
                                    }
                                }
                            }
                        }
                        aside { class: "api-index",
                            h3 { {l.text("Public API map", "公开 API 导航")} }
                            for (module, en, zh) in [
                                ("document", "Versioned content", "版本化内容"),
                                ("session", "History & transactions", "历史与事务"),
                                ("ui", "Canvas & host handles", "画布与宿主接口"),
                                ("map", "Terrain & map intents", "地貌与地图意图"),
                                ("plugin", "Typed extension protocols", "类型化扩展协议"),
                                ("export", "Self-contained SVG", "自包含 SVG"),
                            ]
                            {
                                a {
                                    href: format!(
                                        "https://docs.rs/dioxus-map-editor/latest/dioxus_map_editor/{module}/index.html",
                                    ),
                                    code { "{module}" }
                                    span { "{l.text(en,zh)} ↗" }
                                }
                            }
                            p {
                                {
                                    l.text(
                                        "Generated API reference becomes available on docs.rs after publication. Build it locally with cargo doc --all-features --open.",
                                        "发布后 docs.rs 将提供生成的完整 API。本地可运行 cargo doc --all-features --open。",
                                    )
                                }
                            }
                        }
                    }
                }
                section { class: "project-note",
                    p { class: "eyebrow", "A NOTE ON AUTHORSHIP" }
                    h2 {
                        {l.text("AI-generated. Human regression-tested.", "AI 生成，人工回归。")}
                    }
                    p {
                        {
                            l.text(
                                "This project is fully AI-generated and has undergone human regression testing. If you object to AI-generated software, please do not use it. Merge requests containing AI-generated code are welcome; no AI disclosure label is required.",
                                "本项目全部由 AI 生成，已进行人工回归测试。如果你对此有意见，请勿使用。接受 AI 生成代码的 MR / PR，无需额外标注 AI 来源。",
                            )
                        }
                    }
                    p { class: "muted",
                        {
                            l.text(
                                "Human testing covers the macOS drawing, IME, boundary/re-entry and save/reopen flows. Windows/Linux manual coverage and native quantitative performance are still pending.",
                                "人工测试覆盖 macOS 绘图、输入法、边界与重入、保存重开。Windows/Linux 人工验证与原生定量性能仍待完成。",
                            )
                        }
                    }
                }
            }
            footer { class: "site-footer",
                span { "© Dioxus Map Editor · MIT" }
                div {
                    a { href: "https://github.com/pk111and222/dioxus-map-editor",
                        "Source ↗"
                    }
                    a { href: "https://crates.io/crates/dioxus-map-editor", "crates.io ↗" }
                    a { href: "https://github.com/pk111and222/dioxus-map-editor/blob/main/README_ZH.md",
                        "中文文档 ↗"
                    }
                }
            }
        }
    }
}
#[component]
fn CodeBlock(text: String) -> Element {
    rsx! {
        pre { tabindex: "0",
            code { "{text}" }
        }
    }
}
