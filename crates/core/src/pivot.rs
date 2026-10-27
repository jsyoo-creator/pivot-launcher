//! PIVOT: our add-ons and the versions of their Craft app they work with.
//!
//! Each add-on release carries a `compat.json` next to its installer:
//!
//! ```json
//! { "base": "photocraft", "requires": ">=0.16.0, <0.18.0" }
//! ```
//!
//! The launcher then holds back two kinds of update, so a working setup never breaks:
//! - a Craft app update that an installed add-on hasn't been checked against yet, and
//! - an add-on update that doesn't fit the Craft app version that's installed.

use std::collections::HashMap;

use semver::VersionReq;
use serde::{Deserialize, Serialize};

use crate::catalog::CraftApp;
use crate::version::Version;

/// The release asset that holds an add-on's compatibility range.
pub const COMPAT_ASSET: &str = "compat.json";
/// Upper bound on `compat.json` (it is a few dozen bytes).
pub const COMPAT_MAX_BYTES: u64 = 4 * 1024;

/// Which versions of its Craft app an add-on release works with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compat {
    /// The Craft app's id (`photocraft`).
    pub base: String,
    /// A semver range, e.g. `>=0.16.0, <0.18.0`.
    pub requires: String,
}

impl Compat {
    /// Reads `compat.json`. `None` when it is malformed or names another Craft app.
    pub fn parse(text: &str, app: &CraftApp) -> Option<Compat> {
        let c: Compat = serde_json::from_str(text).ok()?;
        (Some(c.base.as_str()) == app.addon_of && VersionReq::parse(&c.requires).is_ok()).then_some(c)
    }

    /// Whether this range accepts `v`. Pre-releases are matched on their numbers, so a range that
    /// accepts 0.17.0 also accepts 0.17.0-rc.1.
    pub fn allows(&self, v: &Version) -> bool {
        let Ok(req) = VersionReq::parse(&self.requires) else { return false };
        if req.matches(v) {
            return true;
        }
        let plain = Version::new(v.major, v.minor, v.patch);
        !v.pre.is_empty() && req.matches(&plain)
    }
}

/// What the launcher knows when it decides whether an install or update may go ahead.
pub struct Gate<'a> {
    /// Installed version of every app, by id.
    pub installed: &'a HashMap<String, Version>,
    /// The `compat.json` each installed add-on was installed with, by add-on id.
    pub installed_compat: &'a HashMap<String, Compat>,
    /// The `compat.json` of each add-on's latest release, by add-on id.
    pub latest_compat: &'a HashMap<String, Compat>,
}

impl Gate<'_> {
    /// `Some(reason)` when installing `target` of `app` must wait; the reason is shown to the user.
    pub fn veto(&self, app: &CraftApp, target: Option<&Version>) -> Option<String> {
        if let Some(base) = app.addon_of {
            let base_name = CraftApp::by_id(base).map_or(base, |b| b.name);
            let Some(compat) = self.latest_compat.get(app.id) else {
                return Some(format!("{}: compat.json을 아직 못 읽어서 설치를 미뤘어요", app.name));
            };
            let Some(base_ver) = self.installed.get(base) else {
                return Some(format!("{base_name}을(를) 먼저 설치해 주세요"));
            };
            if !compat.allows(base_ver) {
                return Some(format!(
                    "이 버전은 {base_name} {}에서 확인되지 않았어요 (지원 {})",
                    base_ver, compat.requires
                ));
            }
            return None;
        }
        let target = target?;
        for addon in crate::catalog::ADDONS.iter().filter(|a| a.addon_of == Some(app.id)) {
            if !self.installed.contains_key(addon.id) {
                continue;
            }
            match self.installed_compat.get(addon.id) {
                Some(c) if c.allows(target) => {}
                Some(c) => {
                    return Some(format!(
                        "PIVOT {}이(가) {} {}를 아직 확인하지 않았어요 (지원 {}) — 호환 버전이 나오면 업데이트돼요",
                        addon.name, app.name, target, c.requires
                    ));
                }
                None => {
                    return Some(format!(
                        "PIVOT {}의 호환 정보가 없어 {} 업데이트를 미뤘어요",
                        addon.name, app.name
                    ));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::parse;

    fn photocraft() -> &'static CraftApp {
        CraftApp::by_id("photocraft").unwrap()
    }
    fn genfill() -> &'static CraftApp {
        CraftApp::by_id("photocraft-genfill").unwrap()
    }
    fn compat(r: &str) -> Compat {
        Compat { base: "photocraft".into(), requires: r.into() }
    }

    #[test]
    fn parses_compat_for_its_base_only() {
        let ok = r#"{"base":"photocraft","requires":">=0.16.0, <0.18.0"}"#;
        assert_eq!(Compat::parse(ok, genfill()), Some(compat(">=0.16.0, <0.18.0")));
        assert!(Compat::parse(r#"{"base":"filmcraft","requires":">=0.1.0"}"#, genfill()).is_none());
        assert!(Compat::parse(r#"{"base":"photocraft","requires":"not a range"}"#, genfill()).is_none());
        assert!(Compat::parse("garbage", genfill()).is_none());
    }

    #[test]
    fn range_accepts_prereleases_by_numbers() {
        let c = compat(">=0.16.0, <0.18.0");
        assert!(c.allows(&parse("0.17.2").unwrap()));
        assert!(c.allows(&parse("0.17.0-rc.1").unwrap()));
        assert!(!c.allows(&parse("0.18.0").unwrap()));
        assert!(!c.allows(&parse("0.15.9").unwrap()));
    }

    #[test]
    fn holds_a_craft_update_an_installed_addon_has_not_checked() {
        let installed = HashMap::from([("photocraft".to_string(), parse("0.16.0").unwrap()), ("photocraft-genfill".to_string(), parse("1.0.0").unwrap())]);
        let ic = HashMap::from([("photocraft-genfill".to_string(), compat(">=0.16.0, <0.18.0"))]);
        let lc = HashMap::new();
        let g = Gate { installed: &installed, installed_compat: &ic, latest_compat: &lc };
        assert!(g.veto(photocraft(), parse("0.17.1").as_ref()).is_none());
        assert!(g.veto(photocraft(), parse("0.18.0").as_ref()).is_some());
        // Without the add-on installed, the Craft app updates freely.
        let only_pc = HashMap::from([("photocraft".to_string(), parse("0.16.0").unwrap())]);
        let g2 = Gate { installed: &only_pc, installed_compat: &ic, latest_compat: &lc };
        assert!(g2.veto(photocraft(), parse("0.99.0").as_ref()).is_none());
    }

    #[test]
    fn holds_an_addon_that_does_not_fit_the_installed_craft_app() {
        let installed = HashMap::from([("photocraft".to_string(), parse("0.16.0").unwrap())]);
        let ic = HashMap::new();
        let fits = HashMap::from([("photocraft-genfill".to_string(), compat(">=0.16.0, <0.18.0"))]);
        let newer = HashMap::from([("photocraft-genfill".to_string(), compat(">=0.18.0"))]);
        assert!(Gate { installed: &installed, installed_compat: &ic, latest_compat: &fits }.veto(genfill(), None).is_none());
        assert!(Gate { installed: &installed, installed_compat: &ic, latest_compat: &newer }.veto(genfill(), None).is_some());
        let none = HashMap::new();
        assert!(Gate { installed: &none, installed_compat: &ic, latest_compat: &fits }.veto(genfill(), None).is_some());
    }
}
