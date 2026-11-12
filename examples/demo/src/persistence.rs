//! Session loading and storage adapters for the shared map workspace.
use crate::store::DemoStore;
use dioxus_map_editor::{document::*, error::*, history::HistoryBudget, session::*, storage::*};

pub(crate) async fn load(id: MapId) -> MapResult<MapSession> {
    let limits = Limits::default();
    #[cfg(not(target_arch = "wasm32"))]
    let saved = tokio::task::spawn_blocking(move || DemoStore.load(&id, &limits).map(|s| (id, s)))
        .await
        .map_err(|e| MapError::Storage(e.to_string()))??;
    #[cfg(target_arch = "wasm32")]
    let saved = {
        let value = DemoStore.load(&id, &limits)?;
        (id, value)
    };
    if saved.0 .0 == "atlas-studio" && saved.1.is_none() {
        return crate::studio::fresh_session();
    }
    let snapshot = saved
        .1
        .clone()
        .map(StoredMap::into_snapshot)
        .unwrap_or_else(|| MapSnapshot::standalone(MapDocument::new(saved.0)));
    MapSession::open(snapshot, saved.1, limits, HistoryBudget::default())
}
pub(crate) async fn write(request: SaveRequest, limits: Limits, fail: bool) -> SaveReceipt {
    // Keep persistence outside the document and pointer handlers.
    if fail {
        return request.receipt(Err(MapError::Storage(
            "演示保存失败；草稿与历史保留".into(),
        )));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let worker = request.clone();
        let result = tokio::task::spawn_blocking(move || DemoStore.save(&worker, &limits))
            .await
            .map_err(|e| MapError::Storage(e.to_string()))
            .and_then(|r| r);
        request.receipt(result)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let result = DemoStore.save(&request, &limits);
        request.receipt(result)
    }
}
