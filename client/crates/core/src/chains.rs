//! Цепочки — `chains.json`: `{"chains":[{id,name,hops,created}]}`. Активность
//! цепочки, как и на роутере, определяется совпадением хопов с
//! `settings.active_chain`: id цепочки в настройках не хранится.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::ids;
use crate::profiles;
use crate::store::{self, Store};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub hops: Vec<String>,
    #[serde(default)]
    pub created: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChainStore {
    #[serde(default)]
    pub chains: Vec<Chain>,
}

impl ChainStore {
    pub fn load(store: &Store) -> Self {
        store
            .read_json(store::CHAINS)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, store: &Store) -> std::io::Result<()> {
        store.write_json(store::CHAINS, &serde_json::to_value(self)?)
    }

    pub fn get(&self, id: &str) -> Option<&Chain> {
        self.chains.iter().find(|c| c.id == id)
    }

    /// Точку в id роутер допускает, но цели маршрута её вырезают — такая
    /// цепочка молча не работала бы в карте маршрутов. Здесь её не пускаем.
    pub fn upsert(&mut self, store: &Store, id: &str, name: &str, hops: &[String]) -> Result<Chain> {
        if id.is_empty() || ids::sanitize(id) != id {
            bail!("id invalid: only a-z A-Z 0-9 _ -");
        }
        if profiles::exists(store, id) {
            bail!("id цепочки совпадает с id профиля");
        }
        let hops = ids::normalize_hops(hops);
        if hops.is_empty() {
            bail!("в цепочке нет ни одного профиля");
        }
        if let Some(missing) = hops.iter().find(|h| !profiles::exists(store, h)) {
            bail!("профиль {missing} не найден");
        }
        let created = self.get(id).map(|c| c.created).unwrap_or_else(store::now_epoch);
        let chain = Chain {
            id: id.into(),
            name: if name.trim().is_empty() { id.into() } else { name.trim().into() },
            hops,
            created,
        };
        match self.chains.iter_mut().find(|c| c.id == id) {
            Some(slot) => *slot = chain.clone(),
            None => self.chains.push(chain.clone()),
        }
        Ok(chain)
    }

    pub fn remove(&mut self, id: &str) -> Option<Chain> {
        let pos = self.chains.iter().position(|c| c.id == id)?;
        Some(self.chains.remove(pos))
    }

    /// Имена цепочек, в которых участвует профиль.
    pub fn using(&self, profile: &str) -> Vec<String> {
        self.chains
            .iter()
            .filter(|c| c.hops.iter().any(|h| h == profile))
            .map(|c| c.name.clone())
            .collect()
    }
}
