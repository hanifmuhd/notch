# Spike S2: JAKIM prayer zones (2026-11-04)

Source: the zone `<select id="inputZone">` options on https://www.e-solat.gov.my/index.php?siteId=24&pageId=24, fetched 2026-11-04.
Each option reads `CODE - places`. 60 zones, stored in `src-tauri/src/prayer_zones.rs` as data.
Re-run this if JAKIM adds or renames zones; the API takes the code (e.g. WLY01).
