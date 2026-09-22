use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use super::{parse_body, Backend, Start};
use crate::chains::ChainStore;
use crate::ids;
use crate::ipc::{Request, Response};
use crate::lists;
use crate::profiles;
use crate::settings::Settings;
use crate::store;

impl Backend {
    pub(super) fn profiles_list(&self) -> Response {
        let s = &self.store;
        let no_autoswitch = lists::parse_id_list(&s.read_text(store::AUTOSWITCH_EXCLUDE));
        let no_speed = lists::parse_id_list(&s.read_text(store::SPEEDCHECK_EXCLUDE));
        let torrents = lists::parse_id_list(&s.read_text(store::TORRENT_ALLOW));
        let cc = crate::geo::countries(s);
        let list: Vec<Value> = profiles::list_ids(s)
            .into_iter()
            .filter_map(|id| {
                let p = profiles::load(s, &id)?;
                let text = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
                let name = Some(text("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
                Some(json!({
                    "id": id,
                    "type": profiles::profile_type(&p),
                    "name": name,
                    "group": text("group"),
                    "routing_mode": text("routing_mode"),
                    "autoswitch": !no_autoswitch.contains(&id),
                    "speedcheck": !no_speed.contains(&id),
                    "torrents": torrents.contains(&id),
                    "cc": cc.get(&id).cloned().unwrap_or_default(),
                }))
            })
            .collect();
        let chain = Settings::load(s).active_chain();
        Response::json(&json!({
            "profiles": list,
            "active": chain.last().cloned().unwrap_or_default(),
            "active_chain": chain,
        }))
    }

    pub(super) fn profile_get(&self, req: &Request) -> Result<Response> {
        let id = ids::sanitize(req.param("name").ok_or_else(|| anyhow!("name required"))?);
        let raw = profiles::load_raw(&self.store, &id).ok_or_else(|| anyhow!("profile not found"))?;
        Ok(Response { status: 200, body: raw })
    }

    /// В отличие от роутера, правка профиля из активной цепочки сразу
    /// пересобирает конфиг: иначе изменение молча ждало бы следующей активации.
    pub(super) async fn profile_save(&self, body: String) -> Result<()> {
        let id = profiles::save(&self.store, parse_body(&body)?)?;
        if Settings::load(&self.store).active_chain().contains(&id) {
            self.apply(None, Start::IfRunning).await?;
        }
        Ok(())
    }

    pub(super) fn profile_delete(&self, req: &Request) -> Result<()> {
        let id = ids::sanitize(req.param("name").ok_or_else(|| anyhow!("name required"))?);
        if Settings::load(&self.store).active_chain().contains(&id) {
            bail!("cannot delete active chain profile");
        }
        let used = ChainStore::load(&self.store).using(&id);
        if !used.is_empty() {
            bail!("профиль используется в цепочке: {}", used.join(", "));
        }
        profiles::delete(&self.store, &id)
    }

    pub(super) async fn profile_activate(&self, req: &Request) -> Result<()> {
        let id = ids::sanitize(req.param("name").ok_or_else(|| anyhow!("name required"))?);
        if !profiles::exists(&self.store, &id) {
            bail!("profile not found");
        }
        self.apply(Some(vec![id]), Start::Always).await
    }

    pub(super) fn profiles_export(&self) -> Response {
        let list: Vec<Value> = profiles::list_ids(&self.store)
            .iter()
            .filter_map(|id| profiles::load(&self.store, id))
            .collect();
        Response::json(&json!({
            "version": 1,
            "exported_at": humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
            "profiles": list,
        }))
    }

    pub(super) async fn chain_save(&self, body: String) -> Result<()> {
        let v = parse_body(&body)?;
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let hops: Vec<String> = v
            .get("hops")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        let mut chains = ChainStore::load(&self.store);
        let id = text("id");
        let old = chains.get(&id).map(|c| c.hops.clone());
        let chain = chains.upsert(&self.store, &id, &text("name"), &hops)?;
        chains.save(&self.store)?;

        let active = Settings::load(&self.store).active_chain();
        if old.as_ref() == Some(&active) {
            self.apply(Some(chain.hops), Start::IfRunning).await?;
        } else if lists::parse_route_map(&self.store.read_text(store::ROUTE_MAP))
            .iter()
            .any(|s| s.target == chain.id)
        {
            self.apply(None, Start::IfRunning).await?;
        }
        Ok(())
    }

    pub(super) fn chain_delete(&self, body: String) -> Result<()> {
        let v = parse_body(&body)?;
        let id = v.get("id").and_then(Value::as_str).unwrap_or("");
        let mut chains = ChainStore::load(&self.store);
        let hops = chains.get(id).ok_or_else(|| anyhow!("chain not found"))?.hops.clone();
        if hops == Settings::load(&self.store).active_chain() {
            bail!("цепочка сейчас активна");
        }
        chains.remove(id);
        chains.save(&self.store)?;
        Ok(())
    }

    /// Тело — сырой CSV хопов (не id цепочки), как шлёт панель.
    pub(super) async fn chain_activate(&self, body: String) -> Result<()> {
        let hops = ids::parse_csv(body.trim());
        if hops.is_empty() {
            bail!("id required");
        }
        if let Some(missing) = hops.iter().find(|h| !profiles::exists(&self.store, h)) {
            bail!("профиль {missing} не найден");
        }
        self.apply(Some(hops), Start::Always).await
    }

    /// Флаг профиля во внешнем списке id. `list_when` — значение параметра, при
    /// котором id попадает в список (для exclude-списков это `false`).
    pub(super) fn id_flag(
        &self,
        req: &Request,
        file: &str,
        param: &str,
        list_when: bool,
        count_key: &str,
        body: String,
    ) -> Result<Response> {
        let on = matches!(req.param(param), Some("1" | "true"));
        let changed = lists::parse_id_list(&body);
        let mut list = lists::parse_id_list(&self.store.read_text(file));
        if on == list_when {
            for id in changed {
                if !list.contains(&id) {
                    list.push(id);
                }
            }
        } else {
            list.retain(|id| !changed.contains(id));
        }
        let text: String = list.iter().map(|id| format!("{id}\n")).collect();
        self.store.write_text(file, &text)?;
        Ok(Response::json(&json!({ "ok": true, count_key: list.len() })))
    }

    pub(super) async fn torrent_set(&self, req: &Request, body: String) -> Result<Response> {
        let touched = lists::parse_id_list(&body);
        let resp = self.id_flag(req, store::TORRENT_ALLOW, "allow", true, "allowed", body)?;
        if Settings::load(&self.store).active_chain().iter().any(|h| touched.contains(h)) {
            self.apply(None, Start::IfRunning)
                .await
                .map_err(|e| anyhow!("флаг сохранён, но конфиг не собрался: {e:#}"))?;
        }
        Ok(resp)
    }

    pub(super) async fn torrent_status(&self) -> Response {
        let allow = lists::parse_id_list(&self.store.read_text(store::TORRENT_ALLOW));
        let chain = Settings::load(&self.store).active_chain();
        let blocked = chain.iter().find(|h| !allow.contains(h));
        let running = self.engine.pid().await.is_some();
        Response::json(&json!({
            "enforcing": running && blocked.is_some(),
            "profile": blocked.cloned().unwrap_or_default(),
        }))
    }
}
